use std::{fs, io, path::Path};

use crate::{
    containers::binpack::BinPack,
    data::tileset_properties::TilesetProperty,
    dungeon::{
        self,
        tileset::{self, render},
    },
    phases::PhaseId,
    progress::ProgressReporter,
    rom::Rom,
};
const MAX_TILESET_ID: usize = 170;

/// Reads and parses `dungeon.bin` once, for sharing between extractors.
pub fn open_dungeon_bin(rom: &Rom) -> io::Result<BinPack> {
    let file_id = rom
        .fnt
        .get_file_id("DUNGEON/dungeon.bin")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "dungeon.bin not found"))?;

    let data = rom
        .fat
        .get_file_data(file_id as usize, &rom.data)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Failed to extract dungeon.bin")
        })?;

    println!("Parsing dungeon.bin...");
    let binpack = BinPack::from_bytes(data)?;
    println!("dungeon.bin contains {} files", binpack.len());

    Ok(binpack)
}

pub fn extract_dungeon_tilesets(
    binpack: &BinPack,
    tileset_ids: Option<Vec<usize>>,
    output_dir: &Path,
    reporter: &mut ProgressReporter,
    properties: Option<&[TilesetProperty]>,
) -> io::Result<()> {
    let ids: Vec<usize> = match tileset_ids {
        Some(ids) => ids.into_iter().filter(|&id| id < MAX_TILESET_ID).collect(),
        None => (0..MAX_TILESET_ID)
            .filter(|id| !(144..170).contains(id))
            .collect(),
    };

    fs::create_dir_all(output_dir)?;
    render::write_layout_json(output_dir)?;

    let mut all_metadata = Vec::new();

    reporter.begin(PhaseId::DungeonTileset, ids.len());

    for &tileset_id in ids.iter() {
        println!("Extracting tileset {}...", tileset_id);

        let property = properties.and_then(|p| p.get(tileset_id));

        match tileset::extract_tileset(binpack, tileset_id) {
            Ok(tileset) => match render::render_tileset(&tileset, output_dir, property) {
                Ok(meta) => {
                    let status = if meta.animated { "animated" } else { "static" };
                    println!("  -> {} ({})", meta.filename, status);
                    all_metadata.push(meta);
                }
                Err(e) => eprintln!("  -> Error rendering tileset {}: {}", tileset_id, e),
            },
            Err(e) => {
                eprintln!("  -> Error extracting tileset {}: {}", tileset_id, e);
            }
        }

        reporter.advance();
    }

    render::write_tilesets_json(&all_metadata, output_dir)?;

    Ok(())
}

/// Shadows, ripples, weather assets and trap icons.
pub fn extract_dungeon_extras(
    binpack: &BinPack,
    dungeon_dir: &Path,
    reporter: &mut ProgressReporter,
) -> io::Result<()> {
    reporter.begin(PhaseId::DungeonExtras, 4);

    println!("Extracting shadows...");
    if let Err(e) = dungeon::shadows::extract_shadows(binpack, &dungeon_dir.join("shadows")) {
        eprintln!("  -> Error extracting shadows: {}", e);
    }
    reporter.advance();

    println!("Extracting water ripples...");
    if let Err(e) = dungeon::ripples::extract_ripples(binpack, &dungeon_dir.join("ripples")) {
        eprintln!("  -> Error extracting ripples: {}", e);
    }
    reporter.advance();

    // 3D overlay textures + colvec colour table
    println!("Extracting weather assets...");
    if let Err(e) = dungeon::weather::extract_weather_assets(binpack, &dungeon_dir.join("weather"))
    {
        eprintln!("  -> Error extracting weather assets: {}", e);
    }
    reporter.advance();

    // ImgItm container, same format as item icons
    println!("Extracting trap icons...");
    if let Err(e) = dungeon::traps::extract_traps(binpack, &dungeon_dir.join("traps")) {
        eprintln!("  -> Error extracting trap icons: {}", e);
    }
    reporter.advance();

    Ok(())
}
