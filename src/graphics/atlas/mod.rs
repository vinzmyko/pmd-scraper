//! Sprite Atlas Generation
//!
//! This module provides functionality for creating sprite atlases from frame collections,
//! optimising memory usage and rendering performance.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use image::{ImageError, RgbaImage};
use oxipng::{self};
use serde_json;

use crate::graphics::wan::{WanError, WanFile};

pub mod analyser;
pub mod generator;
pub mod metadata;

/// Configuration options for atlas image
#[derive(Debug, Clone)]
pub struct AtlasConfig {
    pub offset_padding: u8,
    pub min_frame_width: u32,
    pub min_frame_height: u32,
    pub deduplicate_frames: bool,
    pub debug: bool,
    pub use_indexed_colour: bool,
    pub use_4bit_depth: bool,
}

impl Default for AtlasConfig {
    fn default() -> Self {
        Self {
            offset_padding: 4,
            min_frame_width: 32,
            min_frame_height: 32,
            deduplicate_frames: true,
            debug: false,
            use_indexed_colour: true,
            use_4bit_depth: true,
        }
    }
}

/// Per-species values copied from monster.md into the atlas JSON.
#[derive(Debug, Clone, Copy)]
pub struct MonsterTraits {
    pub shadow_size: u8,
    pub body_size: u8,
}

/// The final result of the atlas generation process
#[derive(Debug)]
pub struct AtlasResult {
    pub _dimensions: (u32, u32),
    pub _frame_dimensions: (u32, u32),
    pub image_path: PathBuf,
    pub _metadata_path: PathBuf,
}

#[derive(Debug)]
pub enum AtlasError {
    Io(io::Error),
    Image(ImageError),
    Wan(WanError),
    Json(serde_json::Error),
    NoFramesFound,
    NoWanFilesProvided,
    MetadataError(String),
}

impl From<io::Error> for AtlasError {
    fn from(err: io::Error) -> Self {
        AtlasError::Io(err)
    }
}
impl From<ImageError> for AtlasError {
    fn from(err: ImageError) -> Self {
        AtlasError::Image(err)
    }
}
impl From<WanError> for AtlasError {
    fn from(err: WanError) -> Self {
        AtlasError::Wan(err)
    }
}
impl From<serde_json::Error> for AtlasError {
    fn from(err: serde_json::Error) -> Self {
        AtlasError::Json(err)
    }
}

impl std::fmt::Display for AtlasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AtlasError::Io(err) => write!(f, "I/O error: {}", err),
            AtlasError::Image(err) => write!(f, "Image error: {}", err),
            AtlasError::Wan(err) => write!(f, "WAN error: {}", err),
            AtlasError::Json(err) => write!(f, "JSON error: {}", err),
            AtlasError::NoFramesFound => write!(f, "No valid frames found for atlas generation"),
            AtlasError::NoWanFilesProvided => write!(f, "No WAN files provided for processing"),
            AtlasError::MetadataError(msg) => write!(f, "Metadata generation error: {}", msg),
        }
    }
}

/// Creates a sprite atlas and associated metadata for a Pokemon
///
/// This function orchestrates the analysis, layout, generation, and metadata creation
/// based on the provided WAN files and configuration. Uses anchor-based positioning
/// where all frames share a common anchor point representing the entity's feet/ground position.
pub fn create_pokemon_atlas(
    wan_files: &HashMap<String, WanFile>,
    pokemon_id: usize, // monster.md
    dex_num: u16,
    traits: MonsterTraits,
    config: &AtlasConfig,
    output_dir: &Path,
    folder_name: &str,
) -> Result<AtlasResult, AtlasError> {
    if wan_files.is_empty() {
        return Err(AtlasError::NoWanFilesProvided);
    }

    let pokemon_dir = output_dir.join(folder_name);
    fs::create_dir_all(&pokemon_dir)?;

    // Analyse Frames
    println!(
        "Analysing frames for Pokemon #{:03} (Dex #{:03})...",
        pokemon_id, dex_num
    );
    let mut frame_analysis = analyser::analyse_frames(wan_files, dex_num)?;

    if frame_analysis.ordered_frames.is_empty() {
        return Err(AtlasError::NoFramesFound);
    }
    println!(
        "  Analysis complete: {} original frames found.",
        frame_analysis.total_original_frames
    );
    println!(
        "  Entity extents - Left: {}, Right: {}, Up: {}, Down: {}",
        frame_analysis.max_extent_left,
        frame_analysis.max_extent_right,
        frame_analysis.max_extent_up,
        frame_analysis.max_extent_down
    );

    // Calculate Optimal Frame Size (now based on entity origin extents)
    let (frame_width, frame_height) = analyser::calculate_optimal_size(&frame_analysis, config);
    println!(
        "  Optimal frame size calculated: {}x{}",
        frame_width, frame_height
    );

    // Calculate anchor point for logging
    let (anchor_x, anchor_y) =
        analyser::calculate_anchor_point(&frame_analysis, frame_width, frame_height);
    println!(
        "  Anchor point (entity origin): ({}, {})",
        anchor_x, anchor_y
    );

    // Prepare Frames for Atlas (now uses anchor-based positioning)
    let prepared_frames =
        generator::prepare_frames(&mut frame_analysis, frame_width, frame_height)?;
    println!("  Prepared {} frames for atlas.", prepared_frames.len());

    let (unique_frames, frame_mapping) = if config.deduplicate_frames {
        let (unique, mapping) = generator::deduplicate_frames(&prepared_frames);
        println!(
            "  Deduplication result: {} unique frames (reduced from {}).",
            unique.len(),
            prepared_frames.len()
        );
        (unique, mapping)
    } else {
        (
            prepared_frames,
            (0..frame_analysis.total_original_frames).collect(),
        )
    };

    // Create atlas layout (now includes anchor point)
    let atlas_layout = generator::create_atlas_layout(
        &frame_analysis,
        unique_frames.len(),
        frame_width,
        frame_height,
    );

    println!(
        "  Atlas layout created: {}x{} grid, {}x{} total pixels.",
        atlas_layout.frames_per_row,
        atlas_layout.rows,
        atlas_layout.dimensions.0,
        atlas_layout.dimensions.1
    );

    println!("  Generating atlas image...");
    let atlas_image = generator::generate_atlas(&unique_frames, &atlas_layout)?;

    println!("  Generating metadata...");
    let metadata = metadata::generate_metadata(
        wan_files,
        &frame_analysis,
        frame_width,
        frame_height,
        &atlas_layout,
        &frame_mapping,
        traits,
    )?;

    // Save Results
    let atlas_filename = format!("{:03}_atlas.png", dex_num);
    let atlas_path = pokemon_dir.join(&atlas_filename);
    let metadata_filename = format!("{:03}_atlas.json", dex_num);
    let metadata_path = pokemon_dir.join(&metadata_filename);

    println!("  Saving atlas image to {}...", atlas_path.display());

    if config.use_indexed_colour {
        if let Err(e) = save_indexed_atlas(&atlas_image, &atlas_path, config) {
            println!("  Warning: Failed to save with indexed palette: {}", e);
            atlas_image.save(&atlas_path)?;
        }
    } else {
        atlas_image.save(&atlas_path)?;
    }

    println!("  Saving metadata to {}...", metadata_path.display());
    metadata::save_metadata(&metadata, &metadata_path)?;

    if config.debug {
        println!("  Saving debug frames...");
        let debug_dir = pokemon_dir.join("debug_unique_frames");
        fs::create_dir_all(&debug_dir)?;
        for (i, frame) in unique_frames.iter().enumerate() {
            let frame_path = debug_dir.join(format!("unique_frame_{:04}.png", i));
            frame.save(&frame_path)?;
        }
        println!(
            "  Saved {} unique frames to {}",
            unique_frames.len(),
            debug_dir.display()
        );
    }

    println!(
        "Successfully generated atlas for Pokemon #{:03}.",
        pokemon_id
    );

    Ok(AtlasResult {
        _dimensions: atlas_layout.dimensions,
        _frame_dimensions: (frame_width, frame_height),
        image_path: atlas_path,
        _metadata_path: metadata_path,
    })
}

/// Save an atlas image using indexed colour for smaller file size
pub fn save_indexed_atlas(
    atlas_image: &RgbaImage,
    path: &Path,
    config: &AtlasConfig,
) -> Result<(), AtlasError> {
    // Save the atlas image at full quality
    let temp_path = path.with_extension("temp.png");
    atlas_image.save(&temp_path).map_err(AtlasError::Image)?;

    if config.use_4bit_depth {
        let mut options = oxipng::Options::from_preset(2);

        // Enable bit depth reduction for 4-bit output
        options.bit_depth_reduction = true;

        let in_path = temp_path.to_path_buf();
        let out_path = path.to_path_buf();

        // Optimise using InFile and OutFile with PathBuf values
        oxipng::optimize(
            &oxipng::InFile::Path(in_path),
            &oxipng::OutFile::Path(Some(out_path)),
            &options,
        )
        .map_err(|e| AtlasError::MetadataError(format!("PNG optimisation failed: {}", e)))?;

        // Remove temporary file
        if let Err(e) = std::fs::remove_file(&temp_path) {
            println!("  Warning: Failed to remove temporary file: {}", e);
        }
    } else if let Err(e) = std::fs::rename(&temp_path, path) {
        println!("  Warning: Failed to rename file: {}", e);
        if let Err(e) = std::fs::copy(&temp_path, path) {
            return Err(AtlasError::Io(e));
        }
        let _ = std::fs::remove_file(&temp_path);
    }

    Ok(())
}
