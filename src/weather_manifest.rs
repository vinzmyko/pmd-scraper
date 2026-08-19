//! The client-facing contract stitching together the weather
//! visual outputs: the per-weather colour table (colvec), the 3D overlay
//! textures, the precipitation effect sprites, and the code scroll/alpha constants .

use std::{io, path::Path};

use serde::Serialize;

/// Fixed-point scroll step magnitude (1/256 px) — ROM constant 0x60.
const STEP: i32 = 0x60;

#[derive(Serialize)]
pub struct WeatherManifest {
    /// Directory (relative to output root) holding the overlay + colvec textures.
    pub texture_dir: &'static str,
    pub render_constants: RenderConstants,
    pub drift_modes: Vec<DriftMode>,
    pub colvec: ColvecInfo,
    pub weathers: Vec<WeatherEntry>,
}

#[derive(Serialize)]
pub struct RenderConstants {
    pub tile_size_px: u32,
    /// Scroll steps are 1/256 px; px-per-frame = step / scroll_step_divisor.
    pub scroll_step_divisor: u32,
    /// Scroll accumulator wraps over one tile (0x8000 = 128 * 256).
    pub scroll_wrap: u32,
    /// Steady overlay alpha out of `alpha_max` (~25%).
    pub alpha: u8,
    pub alpha_max: u8,
    /// Frames to fade the overlay in when it appears.
    pub fade_in_frames: u32,
}

#[derive(Serialize)]
pub struct DriftMode {
    pub mode: u8,
    /// Per-frame scroll, 1/256 px.
    pub dx: i32,
    pub dy: i32,
    pub label: &'static str,
}

#[derive(Serialize)]
pub struct ColvecInfo {
    pub texture: &'static str,
    pub width: u32,
    pub height: u32,
    /// Per-channel transfer LUT, NOT a tint multiply.
    pub apply: &'static str,
}

#[derive(Serialize)]
pub struct WeatherEntry {
    pub weather_id: u8,
    pub name: &'static str,
    /// Row in colvec.png for this weather's colour table (== weather_id).
    pub colvec_row: u8,
    /// Effect id for the announced mid-floor change (resolve via asset_index.json).
    pub precip_effect_change: Option<u16>,
    /// Effect id for floor-entry / boss (differs from change only for rain).
    pub precip_effect_entry: Option<u16>,
    /// 3D overlay texture in `texture_dir` — fog/sandstorm only.
    pub overlay_texture: Option<&'static str>,
    /// Index into `drift_modes` (fog/sandstorm = 3 = East).
    pub drift_mode: Option<u8>,
}

pub fn build() -> WeatherManifest {
    let drift_modes = vec![
        DriftMode {
            mode: 0,
            dx: 0,
            dy: 0,
            label: "None",
        },
        DriftMode {
            mode: 1,
            dx: 0,
            dy: STEP,
            label: "South",
        },
        DriftMode {
            mode: 2,
            dx: STEP,
            dy: STEP,
            label: "South-East",
        },
        DriftMode {
            mode: 3,
            dx: STEP,
            dy: 0,
            label: "East",
        },
        DriftMode {
            mode: 4,
            dx: STEP,
            dy: -STEP,
            label: "North-East",
        },
        DriftMode {
            mode: 5,
            dx: 0,
            dy: -STEP,
            label: "North",
        },
        DriftMode {
            mode: 6,
            dx: -STEP,
            dy: -STEP,
            label: "North-West",
        },
        DriftMode {
            mode: 7,
            dx: -STEP,
            dy: 0,
            label: "West",
        },
        DriftMode {
            mode: 8,
            dx: -STEP,
            dy: STEP,
            label: "South-West",
        },
        DriftMode {
            mode: 9,
            dx: 0,
            dy: 0,
            label: "Sine (inert)",
        },
    ];

    let weathers = vec![
        WeatherEntry {
            weather_id: 0,
            name: "clear",
            colvec_row: 0,
            precip_effect_change: None,
            precip_effect_entry: None,
            overlay_texture: None,
            drift_mode: None,
        },
        WeatherEntry {
            weather_id: 1,
            name: "sunny",
            colvec_row: 1,
            precip_effect_change: Some(331),
            precip_effect_entry: Some(331),
            overlay_texture: None,
            drift_mode: None,
        },
        WeatherEntry {
            weather_id: 2,
            name: "sandstorm",
            colvec_row: 2,
            precip_effect_change: Some(239),
            precip_effect_entry: Some(239),
            overlay_texture: Some("sandstorm_1005.png"),
            drift_mode: Some(3),
        },
        WeatherEntry {
            weather_id: 3,
            name: "cloudy",
            colvec_row: 3,
            precip_effect_change: None,
            precip_effect_entry: None,
            overlay_texture: None,
            drift_mode: None,
        },
        WeatherEntry {
            weather_id: 4,
            name: "rain",
            colvec_row: 4,
            precip_effect_change: Some(16),
            precip_effect_entry: Some(440),
            overlay_texture: None,
            drift_mode: None,
        },
        WeatherEntry {
            weather_id: 5,
            name: "hail",
            colvec_row: 5,
            precip_effect_change: Some(20),
            precip_effect_entry: Some(20),
            overlay_texture: None,
            drift_mode: None,
        },
        WeatherEntry {
            weather_id: 6,
            name: "fog",
            colvec_row: 6,
            precip_effect_change: None,
            precip_effect_entry: None,
            overlay_texture: Some("fog_1001.png"),
            drift_mode: Some(3),
        },
        WeatherEntry {
            weather_id: 7,
            name: "snow",
            colvec_row: 7,
            precip_effect_change: Some(223),
            precip_effect_entry: Some(223),
            overlay_texture: None,
            drift_mode: None,
        },
    ];

    WeatherManifest {
        texture_dir: "DUNGEON/weather",
        render_constants: RenderConstants {
            tile_size_px: 128,
            scroll_step_divisor: 256,
            scroll_wrap: 0x8000,
            alpha: 0x40,
            alpha_max: 255,
            fade_in_frames: 16,
        },
        drift_modes,
        colvec: ColvecInfo {
            texture: "colvec.png",
            width: 256,
            height: 8,
            apply: "out_c = lut(in_c, weather_id).c  (row = weather_id, column = input value, per channel)",
        },
        weathers,
    }
}

pub fn build_and_save(output_dir: &Path) -> io::Result<()> {
    let manifest = build();

    // Warn-only sanity: the textures this manifest points at should already
    // exist (produced by the dungeon extractor). Effect sprites aren't checked
    // here — they resolve through asset_index.json.
    let tex_dir = output_dir.join(manifest.texture_dir);
    for f in std::iter::once(manifest.colvec.texture)
        .chain(manifest.weathers.iter().filter_map(|w| w.overlay_texture))
    {
        if !tex_dir.join(f).exists() {
            eprintln!(
                "  -> Warning: weather manifest references missing texture {}",
                f
            );
        }
    }

    let path = output_dir.join("weather.json");
    crate::json_out::write(&path, &manifest)?;
    println!("Wrote weather manifest to {}", path.display());
    Ok(())
}
