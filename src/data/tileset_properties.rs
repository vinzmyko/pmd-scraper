use std::path::Path;

use serde::{Deserialize, Serialize};

/// 12-byte `tileset_property` struct in the overlay-10 TILESET_PROPERTIES table.
pub const TILESET_PROPERTY_STRIDE: usize = 12;
pub const TILESET_COUNT: usize = 170;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TilesetProperty {
    pub tileset_id: usize,
    /// Minimap background colour (0-8).
    pub map_color: i32,
    /// 3D mist drift mode: 0 = none, 1-6 = compass drift direction.
    pub weather_effect: u8,
    /// Water tileset flag (affects shadows, Dive, Drought Orb).
    pub is_water_tileset: bool,
}

/// Parse `count` entries from overlay-10 `data` starting at `base_offset`.
pub fn parse_tileset_properties(
    data: &[u8],
    base_offset: usize,
    count: usize,
) -> Result<Vec<TilesetProperty>, String> {
    let end = base_offset
        .checked_add(count * TILESET_PROPERTY_STRIDE)
        .ok_or_else(|| "tileset_property range overflow".to_string())?;
    if end > data.len() {
        return Err(format!(
            "tileset_property table out of bounds: end 0x{:X} > overlay size 0x{:X}",
            end,
            data.len()
        ));
    }

    let mut props = Vec::with_capacity(count);
    let mut nonbool_water = 0usize;

    for i in 0..count {
        let o = base_offset + i * TILESET_PROPERTY_STRIDE;

        let map_color = i32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        let weather_effect = data[o + 0x0A];
        let is_water_byte = data[o + 0x0B];

        if is_water_byte > 1 {
            nonbool_water += 1;
        }

        props.push(TilesetProperty {
            tileset_id: i,
            map_color,
            weather_effect,
            is_water_tileset: is_water_byte != 0,
        });
    }

    // Aggregate guard: is_water is strictly boolean in a correct table. A wrong
    // offset/region lands on random bytes, where most of these exceed 1.
    if nonbool_water > count / 10 {
        return Err(format!(
            "{}/{} entries have a non-boolean is_water byte; TILESET_PROPERTIES \
             offset/region is likely wrong",
            nonbool_water, count
        ));
    }

    Ok(props)
}

pub fn save_json(props: &[TilesetProperty], path: &Path) -> std::io::Result<()> {
    crate::json_out::write(path, props)
}
