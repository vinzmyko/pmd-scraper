use std::{
    collections::HashMap,
    convert::TryInto,
    path::{Path, PathBuf},
};

use image::RgbaImage;
use oxipng::{self, InFile, OutFile};

use crate::containers::{compression::at4px::At4pxContainer, ContainerHandler};

/// Represents a single portrait image from the KAO file
#[derive(Clone, Debug)]
pub struct Portrait {
    palette: Vec<[u8; 3]>,    // RGB colours
    compressed_data: Vec<u8>, // AT4PX compressed data
    _original_size: usize,
}

impl Portrait {
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        if data.len() < KAO_IMG_PAL_SIZE {
            return Err("Data too short for portrait".to_string());
        }

        let mut palette = Vec::with_capacity(16);
        for i in 0..16 {
            let offset = i * 3;
            palette.push([data[offset], data[offset + 1], data[offset + 2]]);
        }

        // Get container size and deserialise
        let (container_size, _) =
            At4pxContainer::get_container_size_and_deserialise(&data[KAO_IMG_PAL_SIZE..])
                .map_err(|e| format!("Failed to parse AT4PX container: {}", e))?;

        let compressed_data = data[KAO_IMG_PAL_SIZE..KAO_IMG_PAL_SIZE + container_size].to_vec();
        let _original_size = KAO_IMG_PAL_SIZE + container_size;

        Ok(Portrait {
            palette,
            compressed_data,
            _original_size,
        })
    }

    pub fn to_rgba_image(&self) -> Result<RgbaImage, String> {
        let at4px_container = At4pxContainer::deserialise(&self.compressed_data)
            .map_err(|e| format!("Failed to create AT4PX container: {}", e))?;

        let decompressed = at4px_container.decompress()?;

        const IMG_DIM: u32 = 40;
        const TILE_DIM: usize = 8;
        const GRID_DIM: usize = 5;
        const PIXELS_PER_TILE: usize = TILE_DIM * TILE_DIM;
        const TOTAL_PIXELS: usize = (IMG_DIM * IMG_DIM) as usize;

        let mut image_buffer = vec![0u8; (IMG_DIM * IMG_DIM * 4) as usize];

        let mut tile_positions = Vec::with_capacity(GRID_DIM * GRID_DIM);
        for tile_id in 0..(GRID_DIM * GRID_DIM) {
            let tile_x = (tile_id % GRID_DIM) as u32;
            let tile_y = (tile_id / GRID_DIM) as u32;
            tile_positions.push((tile_x * TILE_DIM as u32, tile_y * TILE_DIM as u32));
        }

        let expected_pixels = TOTAL_PIXELS;
        let actual_pixels = decompressed.len() * 2; // Each byte has 2 pixels
        let pixel_count = std::cmp::min(expected_pixels, actual_pixels);

        // Process decompressed data (each byte contains two 4-bit pixels)
        for (byte_idx, &byte) in decompressed.iter().enumerate().take(pixel_count / 2) {
            let colour_idx1 = byte & 0xF; // Low nibble
            let colour_idx2 = (byte >> 4) & 0xF; // High nibble

            for i in 0..2 {
                let idx = byte_idx * 2 + i;

                // Calculate which tile this pixel belongs to
                let tile_id = idx / PIXELS_PER_TILE;

                if tile_id >= tile_positions.len() {
                    continue; // So no out of bounds error
                }

                // Get pre-calculated tile position
                let (tile_x, tile_y) = tile_positions[tile_id];

                // Calculate position within the current tile
                let idx_in_tile = idx - (PIXELS_PER_TILE * tile_id);
                let in_tile_x = (idx_in_tile % TILE_DIM) as u32;
                let in_tile_y = (idx_in_tile / TILE_DIM) as u32;

                let final_x = tile_x + in_tile_x;
                let final_y = tile_y + in_tile_y;

                if final_x >= IMG_DIM || final_y >= IMG_DIM {
                    continue; // So no out of bounds error
                }

                // Get colour index based on which nibble we're processing
                let colour_idx = if i == 0 { colour_idx1 } else { colour_idx2 } as usize;

                if colour_idx >= self.palette.len() {
                    continue;
                }

                // Calculate position in the RGBA buffer, 4 bytes per pixel
                let buffer_pos = ((final_y * IMG_DIM + final_x) * 4) as usize;

                // Copy colour data to buffer
                let colour = &self.palette[colour_idx];
                image_buffer[buffer_pos] = colour[0];
                image_buffer[buffer_pos + 1] = colour[1];
                image_buffer[buffer_pos + 2] = colour[2];
                image_buffer[buffer_pos + 3] = 255; // Alpha
            }
        }

        RgbaImage::from_raw(IMG_DIM, IMG_DIM, image_buffer)
            .ok_or_else(|| "Failed to create image from buffer".to_string())
    }
}

/// Represents the entire KAO file containing multiple portraits
pub const KAO_PORTRAITS_PER_POKEMON: usize = 40;
pub const KAO_PORTRAIT_POINTER_SIZE: usize = 4;
pub const KAO_IMG_PAL_SIZE: usize = 48;
pub const _KAO_IMG_DIM: usize = 40;
pub const _KAO_TILE_DIM: usize = 8;
pub const _KAO_META_DIM: usize = 5;
pub const KAO_FIRST_TOC_OFFSET: usize = 160;

#[derive(Debug)]
pub struct KaoFile {
    data: Vec<u8>,
    toc_start_offset: usize,
    pokemon_count: usize,
}

impl KaoFile {
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, String> {
        // First 160 bytes are padding
        let toc_start_offset = KAO_FIRST_TOC_OFFSET;

        if data.len() < toc_start_offset + 4 {
            return Err("Data too short for KAO file".to_string());
        }

        // Read first portrait_pointer to determine TOC length
        let first_portrait_portrait_pointer = i32::from_le_bytes(
            data[toc_start_offset..toc_start_offset + 4]
                .try_into()
                .unwrap(),
        );

        let toc_size_bytes = (first_portrait_portrait_pointer as usize) - toc_start_offset;
        let pokemon_entry_size = KAO_PORTRAITS_PER_POKEMON * KAO_PORTRAIT_POINTER_SIZE;
        let pokemon_count = toc_size_bytes / pokemon_entry_size;

        Ok(KaoFile {
            data,
            toc_start_offset,
            pokemon_count,
        })
    }

    pub fn get_portrait(&self, index: usize, subindex: usize) -> Result<Option<Portrait>, String> {
        if index >= self.pokemon_count {
            return Err(format!(
                "Portrait index {} out of bounds (max {})",
                index, self.pokemon_count
            ));
        }
        if subindex >= KAO_PORTRAITS_PER_POKEMON {
            return Err(format!(
                "Subindex {} out of bounds (max {})",
                subindex, KAO_PORTRAITS_PER_POKEMON
            ));
        }

        let toc_entry_pos = self.toc_start_offset
            + (index * KAO_PORTRAITS_PER_POKEMON * KAO_PORTRAIT_POINTER_SIZE)
            + (subindex * KAO_PORTRAIT_POINTER_SIZE);

        if toc_entry_pos + 4 > self.data.len() {
            return Err("Invalid TOC entry position".to_string());
        }

        // Read pointer
        let portrait_pointer = i32::from_le_bytes(
            self.data[toc_entry_pos..toc_entry_pos + 4]
                .try_into()
                .unwrap(),
        );

        // Negative pointer means no portrait at this position
        if portrait_pointer < 0 {
            return Ok(None);
        }

        let portrait_pos = portrait_pointer as usize;
        if portrait_pos >= self.data.len() {
            return Err("Invalid portrait portrait_pointer".to_string());
        }

        Portrait::from_bytes(&self.data[portrait_pos..]).map(Some)
    }
}

pub enum AtlasType {
    Pokedex,
    Expressions,
}

pub const PORTRAIT_SIZE: u8 = 40;

pub fn create_portrait_atlas(
    kao_file: &KaoFile,
    atlas_type: &AtlasType,
    output_path: &PathBuf,
) -> Result<RgbaImage, String> {
    let max_portraits = match atlas_type {
        AtlasType::Pokedex => 552,
        AtlasType::Expressions => 535,
    };

    let total_portrait_count = count_portraits(kao_file, atlas_type);

    // Calculate optimal layout
    let frames_per_row = (total_portrait_count as f32).sqrt().ceil() as u32;
    let rows = (total_portrait_count as u32).div_ceil(frames_per_row);

    let atlas_width = frames_per_row * PORTRAIT_SIZE as u32;
    let atlas_height = rows * PORTRAIT_SIZE as u32;

    println!(
        "Creating atlas with dimensions: {}x{} for {} portraits",
        atlas_width, atlas_height, total_portrait_count
    );

    let mut atlas = RgbaImage::new(atlas_width, atlas_height);

    // Initialise to transparent
    for pixel in atlas.pixels_mut() {
        *pixel = image::Rgba([0, 0, 0, 0]);
    }

    let mut current_portrait_idx = 0;
    let mut portrait_metadata: HashMap<String, (usize, usize)> = HashMap::new();

    match atlas_type {
        AtlasType::Pokedex => {
            for pokemon_id in 0..max_portraits {
                if pokemon_id > 535 && pokemon_id < 551 {
                    continue;
                }

                if let Ok(Some(portrait)) = kao_file.get_portrait(pokemon_id as usize, 0) {
                    let grid_x = current_portrait_idx % frames_per_row;
                    let grid_y = current_portrait_idx / frames_per_row;

                    let x = grid_x * PORTRAIT_SIZE as u32;
                    let y = grid_y * PORTRAIT_SIZE as u32;

                    if let Ok(portrait_image) = portrait.to_rgba_image() {
                        copy_image_to_atlas(&mut atlas, &portrait_image, x as usize, y as usize);

                        portrait_metadata.insert(
                            format!("mon_{:03}", pokemon_id + 1),
                            (x as usize, y as usize),
                        );

                        current_portrait_idx += 1;
                    }
                }
            }
        }
        AtlasType::Expressions => {
            let emotion_indices = [2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 32, 34];
            let ignore_portraits = [143, 144, 177, 415];

            for pokemon_id in 0..max_portraits {
                if ignore_portraits.contains(&pokemon_id) {
                    continue;
                }

                let mut emotion_idx = 1;
                for &emotion_index in &emotion_indices {
                    if pokemon_id == 37
                        || pokemon_id == 146
                        || (pokemon_id == 64 && emotion_index > 4)
                    {
                        continue;
                    }

                    if let Ok(Some(portrait)) =
                        kao_file.get_portrait(pokemon_id as usize, emotion_index)
                    {
                        let grid_x = current_portrait_idx % frames_per_row;
                        let grid_y = current_portrait_idx / frames_per_row;

                        let x = grid_x * PORTRAIT_SIZE as u32;
                        let y = grid_y * PORTRAIT_SIZE as u32;

                        if let Ok(portrait_image) = portrait.to_rgba_image() {
                            copy_image_to_atlas(
                                &mut atlas,
                                &portrait_image,
                                x as usize,
                                y as usize,
                            );

                            portrait_metadata.insert(
                                format!("mon_{:03}_{}", pokemon_id + 1, emotion_idx),
                                (x as usize, y as usize),
                            );

                            emotion_idx += 1;
                            current_portrait_idx += 1;
                        }
                    }
                }
            }
        }
    }

    let metadata_output_path = output_path.with_extension("json");
    match save_metadata(&portrait_metadata, &metadata_output_path) {
        Ok(_) => {
            println!("Successfully saved portrait metadata");
        }
        Err(e) => {
            println!("Error saving metadata: {}", e);
        }
    }

    println!("Saving atlas to {}...", output_path.display());

    atlas
        .save(output_path)
        .map_err(|e| format!("Failed to save atlas image: {}", e))?;

    if let Err(e) = optimise_portrait_png(output_path) {
        println!("Warning: PNG optimisation failed: {}", e);
    } else {
        println!("PNG optimisation complete");
    }

    Ok(atlas)
}

fn copy_image_to_atlas(atlas: &mut RgbaImage, portrait: &RgbaImage, x: usize, y: usize) {
    for (p_x, p_y, pixel) in portrait.enumerate_pixels() {
        let atlas_x = (x + p_x as usize) as u32;
        let atlas_y = (y + p_y as usize) as u32;

        // Only copy if within bounds
        if atlas_x < atlas.width() && atlas_y < atlas.height() {
            atlas.put_pixel(atlas_x, atlas_y, *pixel);
        }
    }
}

fn save_metadata(metadata: &HashMap<String, (usize, usize)>, path: &PathBuf) -> Result<(), String> {
    crate::json_out::write(path, metadata).map_err(|e| format!("Failed to write metadata: {}", e))
}

/// Optimises a PNG file using oxipng for better compression
fn optimise_portrait_png(path: &Path) -> Result<(), String> {
    let temp_path = path.with_extension("temp.png");

    // If the file was already saved at this path, rename it to temp
    if path.exists() {
        std::fs::rename(path, &temp_path)
            .map_err(|e| format!("Failed to prepare temp file: {}", e))?;
    } else {
        return Err("Image file not found at expected path".to_string());
    }

    let mut options = oxipng::Options::from_preset(4);

    options.bit_depth_reduction = true;

    oxipng::optimize(
        &InFile::Path(temp_path.clone()),
        &OutFile::Path(Some(path.to_path_buf())),
        &options,
    )
    .map_err(|e| format!("PNG optimisation failed: {}", e))?;

    // Remove the temporary file
    if let Err(e) = std::fs::remove_file(&temp_path) {
        println!("  Warning: Failed to remove temporary file: {}", e);
    }

    Ok(())
}

fn count_portraits(kao_file: &KaoFile, atlas_type: &AtlasType) -> usize {
    let mut count = 0;
    let max_portraits = match atlas_type {
        AtlasType::Pokedex => 552,
        AtlasType::Expressions => 535,
    };

    match atlas_type {
        AtlasType::Pokedex => {
            for pokemon_id in 0..max_portraits {
                if pokemon_id > 535 && pokemon_id < 551 {
                    continue;
                }

                if let Ok(Some(_)) = kao_file.get_portrait(pokemon_id as usize, 0) {
                    count += 1;
                }
            }
        }
        AtlasType::Expressions => {
            let emotion_indices = [2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 32, 34];
            let ignore_portraits = [143, 144, 177, 415];

            for pokemon_id in 0..max_portraits {
                if ignore_portraits.contains(&pokemon_id) {
                    continue;
                }

                for &emotion_index in &emotion_indices {
                    if pokemon_id == 37
                        || pokemon_id == 146
                        || (pokemon_id == 64 && emotion_index > 4)
                    {
                        continue;
                    }

                    if let Ok(Some(_)) = kao_file.get_portrait(pokemon_id as usize, emotion_index) {
                        count += 1;
                    }
                }
            }
        }
    }

    count
}
