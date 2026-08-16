//! Trap Icon Extraction
//!
//! Trap icons live in `dungeon.bin` as `traps.trp.img`, the same `ImgItm` container format as item icons 
//! but with 24x24 cells rather than 16x16.
//!
//! TODO: The second palette is assumed to be the revealed/highlighted variant, but need to investigate.

use std::{collections::BTreeMap, fs, io, path::Path};

use serde::Serialize;

use crate::{
    containers::binpack::BinPack,
    graphics::grid_atlas,
    item_data_extractor::{load_img, palettes_hex},
};

/// `dungeon.bin` index of `traps.trp.img`.
const TRAPS_IMG_INDEX: usize = 1033;

const ATLAS_FILENAME: &str = "traps_atlas.png";

/// Mirrors the item manifest shape so the client can share one loader.
#[derive(Serialize)]
struct TrapManifest {
    atlas: &'static str,
    cell: u32,
    cols: usize,
    rows: usize,
    palettes: Vec<Vec<String>>,
    entries: BTreeMap<String, TrapEntry>,
}

#[derive(Serialize)]
struct TrapEntry {
    sprite: usize,
    palette: usize,
    /// Atlas coordinates of this trap's cell, in pixels.
    x: u32,
    y: u32,
}

pub fn extract_traps(binpack: &BinPack, output_dir: &Path) -> io::Result<()> {
    let img = load_img(binpack, TRAPS_IMG_INDEX)?;
    println!(
        "  Trap icons: {} sprites x {} palettes, {}px cells",
        img.sprite_count,
        img.palettes.len(),
        img.cell
    );

    fs::create_dir_all(output_dir)?;

    let atlas = grid_atlas::build(&img.render_all(), img.palettes.len(), img.cell, img.cell);
    atlas
        .save(output_dir.join(ATLAS_FILENAME))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    println!("  -> {} ({}x{})", ATLAS_FILENAME, atlas.width(), atlas.height());

    let entries: BTreeMap<String, TrapEntry> = (0..img.sprite_count)
        .map(|sprite| {
            (
                format!("trap_{:02}", sprite),
                TrapEntry {
                    sprite,
                    palette: 0,
                    x: 0,
                    y: sprite as u32 * img.cell,
                },
            )
        })
        .collect();

    let manifest = TrapManifest {
        atlas: ATLAS_FILENAME,
        cell: img.cell,
        cols: img.palettes.len(),
        rows: img.sprite_count,
        palettes: palettes_hex(&img),
        entries,
    };

    let manifest_path = output_dir.join("traps.json");
    serde_json::to_writer_pretty(fs::File::create(&manifest_path)?, &manifest)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    println!("  -> traps.json ({} entries)", manifest.entries.len());

    Ok(())
}
