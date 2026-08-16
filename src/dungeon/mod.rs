pub mod colvec;
pub mod dungeon_names;
pub mod ripples;
pub mod shadows;
pub mod tileset;
pub mod traps;
pub mod weather;

use image::Rgba;

/// Parse RGBX palette (4 bytes per color). Index 0 = transparent.
/// Shared by shadows (997) and ripples (997).
pub fn parse_rgbx_palette(data: &[u8]) -> Vec<Rgba<u8>> {
    data.chunks(4)
        .enumerate()
        .map(|(i, c)| Rgba([c[0], c[1], c[2], if i == 0 { 0 } else { 255 }]))
        .collect()
}
