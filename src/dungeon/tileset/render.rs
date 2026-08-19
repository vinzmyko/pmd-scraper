//! # Dungeon Tileset Rendering
//!
//! Converts parsed dungeon data into an organised 8×6×3 tileset image.

use std::{collections::BTreeMap, io, path::Path};

use image::{Rgba, RgbaImage};
use serde::Serialize;

use super::{dma::DmaType, dpci::DPCI_TILE_DIM, dpla::DplaColourEntry, DungeonTileset};
use crate::data::tileset_properties::TilesetProperty;

const N: u8 = 16;
const S: u8 = 1;
const E: u8 = 4;
const W: u8 = 64;
const NE: u8 = 8;
const NW: u8 = 32;
const SE: u8 = 2;
const SW: u8 = 128;
const ALL: u8 = N | S | E | W | NE | NW | SE | SW;

const CHUNK_PX: usize = DPCI_TILE_DIM * 3;
const COLS_PER_TILE_TYPE: usize = 8;
const ROWS_PER_TILE_TYPE: usize = 6;
const NUM_TILE_TYPES: usize = 3;

const TILE_TYPES: [(DmaType, &str); NUM_TILE_TYPES] = [
    (DmaType::Wall, "wall"),
    (DmaType::Secondary, "secondary"),
    (DmaType::Floor, "floor"),
];

const NUM_VARIANTS: usize = 3; // ROM has 3 variations per tile
const VARIANT_BLOCK_WIDTH: usize = COLS_PER_TILE_TYPE * CHUNK_PX;
const TILE_TYPE_SET_WIDTH: usize = VARIANT_BLOCK_WIDTH * NUM_VARIANTS;
const FRAME_WIDTH: usize = TILE_TYPE_SET_WIDTH * NUM_TILE_TYPES;
const FRAME_HEIGHT: usize = ROWS_PER_TILE_TYPE * CHUNK_PX;

/// 48 tile configs in 8×6 grid. -1 = empty cell.
const TILE_LAYOUT: [(&str, i16); 48] = [
    // Row 0: Inner corners
    ("full", ALL as i16),
    ("inner_SE", (ALL ^ SE) as i16),
    ("inner_SW", (ALL ^ SW) as i16),
    ("inner_SE_SW", (ALL ^ SE ^ SW) as i16),
    ("inner_NE", (ALL ^ NE) as i16),
    ("inner_NE_SE", (ALL ^ NE ^ SE) as i16),
    ("inner_NE_SW", (ALL ^ NE ^ SW) as i16),
    ("inner_NE_SE_SW", (ALL ^ NE ^ SE ^ SW) as i16),
    // Row 1: Inner corners (NW combos)
    ("inner_NW", (ALL ^ NW) as i16),
    ("inner_NW_SE", (ALL ^ NW ^ SE) as i16),
    ("inner_NW_SW", (ALL ^ NW ^ SW) as i16),
    ("inner_NW_SE_SW", (ALL ^ NW ^ SE ^ SW) as i16),
    ("inner_NE_NW", (ALL ^ NE ^ NW) as i16),
    ("inner_NE_NW_SE", (ALL ^ NE ^ NW ^ SE) as i16),
    ("inner_NE_NW_SW", (ALL ^ NE ^ NW ^ SW) as i16),
    ("inner_all_4", (N | S | E | W) as i16),
    // Row 2: N/S edges with inner corner variants
    ("edge_N", (S | E | W | SE | SW) as i16),
    ("edge_N_inner_SE", (S | E | W | SW) as i16),
    ("edge_N_inner_SW", (S | E | W | SE) as i16),
    ("edge_N_inner_both", (S | E | W) as i16),
    ("edge_S", (N | E | W | NE | NW) as i16),
    ("edge_S_inner_NE", (N | E | W | NW) as i16),
    ("edge_S_inner_NW", (N | E | W | NE) as i16),
    ("edge_S_inner_both", (N | E | W) as i16),
    // Row 3: E/W edges with inner corner variants
    ("edge_E", (N | S | W | NW | SW) as i16),
    ("edge_E_inner_NW", (N | S | W | SW) as i16),
    ("edge_E_inner_SW", (N | S | W | NW) as i16),
    ("edge_E_inner_both", (N | S | W) as i16),
    ("edge_W", (N | S | E | NE | SE) as i16),
    ("edge_W_inner_NE", (N | S | E | SE) as i16),
    ("edge_W_inner_SE", (N | S | E | NE) as i16),
    ("edge_W_inner_both", (N | S | E) as i16),
    // Row 4: Outer corners, corridors, T-junctions
    ("corner_NW", (S | E | SE) as i16),
    ("corner_NE", (S | W | SW) as i16),
    ("corner_SW", (N | E | NE) as i16),
    ("corner_SE", (N | W | NW) as i16),
    ("corridor_NS", (N | S) as i16),
    ("corridor_EW", (E | W) as i16),
    ("T_north", (S | E | W) as i16),
    ("T_south", (N | E | W) as i16),
    // Row 5: T-junctions, end caps, isolated
    ("T_east", (N | S | E) as i16),
    ("T_west", (N | S | W) as i16),
    ("end_N", N as i16),
    ("end_S", S as i16),
    ("end_E", E as i16),
    ("end_W", W as i16),
    ("isolated", 0),
    ("_empty", -1),
];

#[derive(Serialize)]
pub struct TilesetMetadata {
    pub tileset_id: usize,
    pub filename: String,
    pub animated: bool,
    pub palette_10_frames: usize,
    pub palette_11_frames: usize,
    pub durations_palette_10: Vec<u16>,
    pub durations_palette_11: Vec<u16>,
    pub map_color: i32,
    pub weather_effect: u8,
    pub is_water_tileset: bool,
    /// Mist overlay texture to tile, or None.
    /// when this tileset has no mist (`weather_effect == 0`).
    pub mist_texture: Option<String>,
}

#[derive(Serialize)]
struct LayoutJson {
    chunk_size: usize,
    frame_width: usize,
    frame_height: usize,
    columns_per_tile_type: usize,
    rows_per_tile_type: usize,
    num_variants: usize,
    x_offset_per_variant: usize,
    x_offset_per_tile_type: usize,
    tile_types: Vec<String>,
    neighbour_bits: BTreeMap<String, u8>,
    tiles: Vec<LayoutTile>,
}

#[derive(Serialize)]
struct LayoutTile {
    name: String,
    index: usize,
    col: usize,
    row: usize,
    neighbour_bits: u8,
}

pub fn render_tileset(
    tileset: &DungeonTileset,
    output_dir: &Path,
    property: Option<&TilesetProperty>,
) -> Result<TilesetMetadata, io::Error> {
    let dungeon_name = crate::dungeon::dungeon_names::tileset_name(tileset.tileset_id);
    let name = format!("{:03}_{}", tileset.tileset_id, dungeon_name);

    let sheet = render_organised_sheet(tileset);
    sheet
        .save(output_dir.join(format!("{}.png", name)))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let (pal10_frames, pal11_frames) = animation_frame_counts(tileset);
    let animated = pal10_frames > 0 || pal11_frames > 0;

    if animated {
        let pal_tex = create_palette_texture(tileset, pal10_frames, pal11_frames);
        pal_tex
            .save(output_dir.join(format!("{}.pal.png", name)))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    }

    let (map_color, weather_effect, is_water_tileset) = match property {
        Some(p) => (p.map_color, p.weather_effect, p.is_water_tileset),
        None => (0, 0, false),
    };

    let mist_texture = if weather_effect != 0 {
        Some("mist_1031.png".to_string())
    } else {
        None
    };

    Ok(TilesetMetadata {
        tileset_id: tileset.tileset_id,
        filename: format!("{}.png", name),
        animated,
        palette_10_frames: pal10_frames,
        palette_11_frames: pal11_frames,
        durations_palette_10: tileset.dpla.colours[0..16]
            .iter()
            .map(|c| c.duration)
            .collect(),
        durations_palette_11: tileset.dpla.colours[16..32]
            .iter()
            .map(|c| c.duration)
            .collect(),
        map_color,
        weather_effect,
        is_water_tileset,
        mist_texture,
    })
}

pub fn write_layout_json(output_dir: &Path) -> Result<(), io::Error> {
    let mut neighbour_bits = BTreeMap::new();
    for (name, val) in [
        ("N", N),
        ("S", S),
        ("E", E),
        ("W", W),
        ("NE", NE),
        ("NW", NW),
        ("SE", SE),
        ("SW", SW),
    ] {
        neighbour_bits.insert(name.into(), val);
    }

    let tiles: Vec<LayoutTile> = TILE_LAYOUT
        .iter()
        .enumerate()
        .filter(|(_, (_, bits))| *bits >= 0)
        .map(|(i, (name, bits))| LayoutTile {
            name: name.to_string(),
            index: i,
            col: i % COLS_PER_TILE_TYPE,
            row: i / COLS_PER_TILE_TYPE,
            neighbour_bits: *bits as u8,
        })
        .collect();

    let layout = LayoutJson {
        chunk_size: CHUNK_PX,
        frame_width: FRAME_WIDTH,
        frame_height: FRAME_HEIGHT,
        columns_per_tile_type: COLS_PER_TILE_TYPE,
        rows_per_tile_type: ROWS_PER_TILE_TYPE,
        num_variants: NUM_VARIANTS,
        x_offset_per_variant: VARIANT_BLOCK_WIDTH,
        x_offset_per_tile_type: TILE_TYPE_SET_WIDTH,
        tile_types: TILE_TYPES
            .iter()
            .map(|(_, name)| name.to_string())
            .collect(),
        neighbour_bits,
        tiles,
    };

    crate::json_out::write(&output_dir.join("layout.json"), &layout)
}

pub fn write_tilesets_json(
    metadata: &[TilesetMetadata],
    output_dir: &Path,
) -> Result<(), io::Error> {
    crate::json_out::write(&output_dir.join("tilesets.json"), metadata)
}

fn render_organised_sheet(tileset: &DungeonTileset) -> RgbaImage {
    let mut img = RgbaImage::new(FRAME_WIDTH as u32, FRAME_HEIGHT as u32);

    // Iterate through TileTypes
    for (t_idx, (tile_type, _)) in TILE_TYPES.iter().enumerate() {
        // Calculate where this TileType's section begins in the image
        let tile_type_base_x = t_idx * TILE_TYPE_SET_WIDTH;

        // Iterate through the 3 Variants
        for v_idx in 0..NUM_VARIANTS {
            // Calculate where this specific Variant block begins
            let variant_offset_x = v_idx * VARIANT_BLOCK_WIDTH;

            // Iterate through the 47 layout tiles
            for (i, (_, neighbour_bits)) in TILE_LAYOUT.iter().enumerate() {
                if *neighbour_bits < 0 {
                    continue;
                }

                let col = i % COLS_PER_TILE_TYPE;
                let row = i / COLS_PER_TILE_TYPE;

                // Get the specific variant ID
                let chunk_mappings = tileset.dma.get(*tile_type, *neighbour_bits as u8);
                let chunk_id = chunk_mappings[v_idx] as usize;

                render_chunk_at(
                    &mut img,
                    tileset,
                    chunk_id,
                    tile_type_base_x + variant_offset_x + (col * CHUNK_PX),
                    row * CHUNK_PX,
                );
            }
        }
    }

    img
}

fn render_chunk_at(
    img: &mut RgbaImage,
    tileset: &DungeonTileset,
    chunk_id: usize,
    bx: usize,
    by: usize,
) {
    if chunk_id >= tileset.dpc.chunks.len() {
        return;
    }
    let chunk = &tileset.dpc.chunks[chunk_id];

    for (i, mapping) in chunk.iter().enumerate() {
        let tx = bx + (i % 3) * DPCI_TILE_DIM;
        let ty = by + (i / 3) * DPCI_TILE_DIM;

        let ti = mapping.tile_index as usize;
        if ti >= tileset.dpci.tiles.len() {
            continue;
        }

        let pixels = tileset.dpci.decode_tile(ti);
        let pal = if (mapping.palette_idx as usize) < 12 {
            &tileset.dpl.palettes[mapping.palette_idx as usize]
        } else {
            &tileset.dpl.palettes[0]
        };

        for py in 0..DPCI_TILE_DIM {
            for px in 0..DPCI_TILE_DIM {
                let sx = if mapping.flip_x { 7 - px } else { px };
                let sy = if mapping.flip_y { 7 - py } else { py };
                let ci = pixels[sy * DPCI_TILE_DIM + sx] as usize;

                let rgba = if ci == 0 {
                    Rgba([0, 0, 0, 0])
                } else {
                    let c = pal[ci];
                    Rgba([c.r, c.g, c.b, 255])
                };

                let ox = (tx + px) as u32;
                let oy = (ty + py) as u32;
                if ox < img.width() && oy < img.height() {
                    img.put_pixel(ox, oy, rgba);
                }
            }
        }
    }
}

fn create_palette_texture(
    tileset: &DungeonTileset,
    pal10_frames: usize,
    pal11_frames: usize,
) -> RgbaImage {
    let base_rows = 12usize;
    let height = base_rows + pal10_frames + pal11_frames;
    let mut img = RgbaImage::new(16, height as u32);

    for (pal_idx, pal) in tileset.dpl.palettes.iter().enumerate() {
        for (ci, col) in pal.iter().enumerate() {
            img.put_pixel(ci as u32, pal_idx as u32, Rgba([col.r, col.g, col.b, 255]));
        }
    }

    write_animation_rows(
        &mut img,
        &tileset.dpla.colours[0..16],
        base_rows,
        pal10_frames,
    );
    write_animation_rows(
        &mut img,
        &tileset.dpla.colours[16..32],
        base_rows + pal10_frames,
        pal11_frames,
    );

    img
}

fn write_animation_rows(
    img: &mut RgbaImage,
    entries: &[DplaColourEntry],
    start_row: usize,
    num_frames: usize,
) {
    for frame in 0..num_frames {
        for (ci, entry) in entries.iter().enumerate() {
            let rgb = if frame < entry.frames.len() {
                &entry.frames[frame]
            } else if let Some(last) = entry.frames.last() {
                last
            } else {
                continue;
            };
            img.put_pixel(
                ci as u32,
                (start_row + frame) as u32,
                Rgba([rgb.r, rgb.g, rgb.b, 255]),
            );
        }
    }
}

fn animation_frame_counts(tileset: &DungeonTileset) -> (usize, usize) {
    let pal10 = detect_real_animation(&tileset.dpla.colours[0..16]);
    let pal11 = detect_real_animation(&tileset.dpla.colours[16..32]);
    (pal10, pal11)
}

fn detect_real_animation(entries: &[DplaColourEntry]) -> usize {
    let max_frames = entries
        .iter()
        .map(|c| c.num_frames as usize)
        .max()
        .unwrap_or(0);
    if max_frames <= 1 {
        return 0;
    }
    for entry in entries {
        if entry.frames.len() >= 2 && entry.frames[0] != entry.frames[1] {
            return max_frames;
        }
    }
    0
}
