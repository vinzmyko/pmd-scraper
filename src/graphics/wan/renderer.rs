//! Renderer for WAN sprite frames
//!
//! This module provides functionality to render individual frames from WAN files
//! into RGBA images, handling position offsets, flipping, and palette mapping.

use crate::graphics::wan::{
    model::{MetaFramePiece, WanFile},
    AnimationStructure, WanError, TEX_SIZE,
};

use image::{imageops, Rgba, RgbaImage};

// This constant is used by the canvas calculation logic
const CENTRE_X: i16 = 256;
const CENTRE_Y: i16 = 512;

/// Extract a single frame from a WAN file
pub fn extract_frame(wan: &WanFile, frame_idx: usize) -> Result<RgbaImage, WanError> {
    if frame_idx >= wan.frame_data.len() {
        return Err(WanError::OutOfBounds(format!(
            "Frame index {} out of bounds (max: {})",
            frame_idx,
            wan.frame_data.len() - 1
        )));
    }

    let frame_data = &wan.frame_data[frame_idx];
    if frame_data.pieces.is_empty() {
        return Ok(RgbaImage::new(8, 8));
    }

    let frame_bounds = get_frame_bounds(wan, frame_idx)?;
    let width = (frame_bounds.2 - frame_bounds.0).max(1);
    let height = (frame_bounds.3 - frame_bounds.1).max(1);

    let mut image = RgbaImage::new(width as u32, height as u32);

    for (i, piece) in frame_data.pieces.iter().enumerate() {
        let pal_num = piece.palette_index as usize;
        if pal_num >= wan.custom_palette.len() {
            println!(
                "Warning: Skipping piece {} in frame {} with invalid palette index {}",
                i, frame_idx, pal_num
            );
            continue;
        }
        let palette = &wan.custom_palette[pal_num];

        let dimensions = piece.get_dimensions();
        let pos_x = piece.get_bounds().0 - frame_bounds.0;
        let pos_y = piece.get_bounds().1 - frame_bounds.1;

        render_piece(
            wan,
            piece,
            &mut image,
            (pos_x as i32, pos_y as i32),
            (dimensions.0 * TEX_SIZE, dimensions.1 * TEX_SIZE),
            palette,
        )?;
    }

    Ok(image)
}

/// Get the canvas box for a specific animation index.
/// Returns None if the animation doesn't exist or is empty.
pub fn get_effect_animation_canvas_box(
    wan_file: &WanFile,
    animation_index: usize,
) -> Result<Option<(i16, i16, i16, i16)>, WanError> {
    let animation = match &wan_file.animations {
        AnimationStructure::Effect(groups) => groups.first().and_then(|group| {
            let clamped_index = if animation_index >= group.len() {
                0
            } else {
                animation_index
            };
            group.get(clamped_index)
        }),
        AnimationStructure::Character(_) => {
            return Err(WanError::InvalidDataStructure(
                "Character animation structure not supported for effect rendering".to_string(),
            ));
        }
    };

    let animation = match animation {
        Some(anim) => anim,
        None => return Ok(None),
    };

    if animation.frames.is_empty() {
        return Ok(None);
    }

    let max_bounds = get_animation_bounds(wan_file, animation)?;
    if max_bounds.2 <= max_bounds.0 || max_bounds.3 <= max_bounds.1 {
        return Ok(None);
    }

    Ok(Some(round_up_box(max_bounds)))
}

/// Renders a complete animation sequence to a single horizontal sprite sheet image.
/// If `fixed_canvas_box` is provided, uses those dimensions instead of calculating from bounds.
/// If `offset_filter` is provided, only pieces with that `draw_order_offset` are drawn.
pub fn render_effect_animation_sheet_with_canvas(
    wan_file: &WanFile,
    animation_index: usize,
    fixed_canvas_box: Option<(i16, i16, i16, i16)>,
    offset_filter: Option<i8>,
) -> Result<Option<(RgbaImage, u32, u32)>, WanError> {
    // Animation_index is a sequence index into group 0 ONLY
    let animation = match &wan_file.animations {
        AnimationStructure::Effect(groups) => {
            // ROM always uses group 0, animation_index is the sequence index
            groups.first().and_then(|group| {
                // Clamp out-of-bounds to 0, matching ROM behavior
                let clamped_index = if animation_index >= group.len() {
                    eprintln!(
                        "Warning: animation_index {} out of bounds (max {}), clamping to 0",
                        animation_index,
                        group.len().saturating_sub(1)
                    );
                    0
                } else {
                    animation_index
                };
                group.get(clamped_index)
            })
        }
        AnimationStructure::Character(_) => {
            return Err(WanError::InvalidDataStructure(
                "Character animation structure not supported for effect rendering".to_string(),
            ));
        }
    }
    .ok_or_else(|| {
        WanError::OutOfBounds(format!(
            "Animation index {} is out of bounds",
            animation_index
        ))
    })?;

    if animation.frames.is_empty() {
        return Ok(None);
    }

    // Use fixed canvas box if provided, otherwise calculate from this animation's bounds
    let canvas_box = match fixed_canvas_box {
        Some(box_dims) => box_dims,
        None => {
            let max_bounds = get_animation_bounds(wan_file, animation)?;
            if max_bounds.2 <= max_bounds.0 || max_bounds.3 <= max_bounds.1 {
                return Ok(None);
            }
            round_up_box(max_bounds)
        }
    };

    let frame_width = (canvas_box.2 - canvas_box.0).max(1) as u32;
    let frame_height = (canvas_box.3 - canvas_box.1).max(1) as u32;

    let mut rendered_frames = Vec::new();
    for seq_frame in animation.frames.iter() {
        let meta_frame_index = seq_frame.frame_index as usize;

        if meta_frame_index < wan_file.frame_data.len() {
            let frame_image = render_meta_frame_on_canvas(
                wan_file,
                meta_frame_index,
                canvas_box,
                (seq_frame.offset.0, seq_frame.offset.1),
                offset_filter,
            )?;
            rendered_frames.push(frame_image);
        } else {
            rendered_frames.push(RgbaImage::new(frame_width, frame_height));
        }
    }

    if rendered_frames.is_empty() {
        return Ok(None);
    }

    let sprite_sheet = combine_frames_horizontally(&rendered_frames)?;

    Ok(Some((sprite_sheet, frame_width, frame_height)))
}

/// Renders a complete animation sequence to a single horizontal sprite sheet image
pub fn render_effect_animation_sheet(
    wan_file: &WanFile,
    animation_index: usize,
) -> Result<Option<(RgbaImage, u32, u32)>, WanError> {
    render_effect_animation_sheet_with_canvas(wan_file, animation_index, None, None)
}

/// Calculates the maximum bounding box that encloses every frame in an animation sequence
fn get_animation_bounds(
    wan: &WanFile,
    animation: &crate::graphics::wan::model::Animation,
) -> Result<(i16, i16, i16, i16), WanError> {
    let mut combined_bounds = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
    let mut has_visible_pieces = false;

    for seq_frame in animation.frames.iter() {
        let meta_frame_index = seq_frame.frame_index as usize;

        if meta_frame_index >= wan.frame_data.len() {
            continue;
        }

        let meta_frame = &wan.frame_data[meta_frame_index];

        if meta_frame.pieces.is_empty() {
            continue;
        }

        for piece in meta_frame.pieces.iter() {
            let (width_blocks, height_blocks) = piece.get_dimensions();
            let width_px = (width_blocks * TEX_SIZE) as i16;
            let height_px = (height_blocks * TEX_SIZE) as i16;

            let off_x = seq_frame.offset.0;
            let off_y = seq_frame.offset.1;
            let piece_rect = (
                piece.x_offset + off_x,
                piece.y_offset + off_y,
                piece.x_offset + width_px + off_x,
                piece.y_offset + height_px + off_y,
            );

            combined_bounds.0 = combined_bounds.0.min(piece_rect.0);
            combined_bounds.1 = combined_bounds.1.min(piece_rect.1);
            combined_bounds.2 = combined_bounds.2.max(piece_rect.2);
            combined_bounds.3 = combined_bounds.3.max(piece_rect.3);
            has_visible_pieces = true;
        }
    }

    if !has_visible_pieces {
        return Ok((0, 0, 0, 0));
    }

    Ok(combined_bounds)
}

/// Renders a single meta frame onto a canvas
fn render_meta_frame_on_canvas(
    wan: &WanFile,
    meta_frame_index: usize,
    canvas_box: (i16, i16, i16, i16),
    offset: (i16, i16),
    offset_filter: Option<i8>,
) -> Result<RgbaImage, WanError> {
    let canvas_width = (canvas_box.2 - canvas_box.0).max(1) as u32;
    let canvas_height = (canvas_box.3 - canvas_box.1).max(1) as u32;
    let mut image = RgbaImage::new(canvas_width, canvas_height);

    let frame_data = &wan.frame_data[meta_frame_index];

    for piece in &frame_data.pieces {
        if offset_filter.is_some_and(|o| piece.draw_order_offset() != o) {
            continue;
        }
        let pal_num = piece.palette_index as usize;

        if pal_num >= wan.custom_palette.len() {
            return Err(WanError::OutOfBounds(format!(
                "Palette index {} is out of bounds for meta-frame {} (palette count: {}).",
                pal_num,
                meta_frame_index,
                wan.custom_palette.len()
            )));
        }

        let palette = &wan.custom_palette[pal_num];
        let dimensions = piece.get_dimensions();
        let pos_x = piece.x_offset + offset.0 - canvas_box.0;
        let pos_y = piece.y_offset + offset.1 - canvas_box.1;

        render_piece(
            wan,
            piece,
            &mut image,
            (pos_x as i32, pos_y as i32),
            (dimensions.0 * TEX_SIZE, dimensions.1 * TEX_SIZE),
            palette,
        )?;
    }

    Ok(image)
}

/// Modifies the bounding box to be centred and have dimensions that are multiples of 8
fn round_up_box(bounds: (i16, i16, i16, i16)) -> (i16, i16, i16, i16) {
    fn round_up_to_mult(n: i16, m: i16) -> i16 {
        if n <= 0 {
            return m;
        }
        let sub_int = n - 1;
        (sub_int / m + 1) * m
    }

    let width = (CENTRE_X - bounds.0).max(bounds.2 - CENTRE_X) * 2;
    let height = (CENTRE_Y - bounds.1).max(bounds.3 - CENTRE_Y) * 2;

    let new_width = round_up_to_mult(width, 8);
    let new_height = round_up_to_mult(height, 8);

    let start_x = CENTRE_X - new_width / 2;
    let start_y = CENTRE_Y - new_height / 2;

    (start_x, start_y, start_x + new_width, start_y + new_height)
}

/// Stitches a vector of images into a single horizontal strip
fn combine_frames_horizontally(frames: &[RgbaImage]) -> Result<RgbaImage, WanError> {
    if frames.is_empty() {
        return Err(WanError::InvalidDataStructure(
            "Cannot combine zero frames.".to_string(),
        ));
    }

    let frame_width = frames[0].width();
    let frame_height = frames[0].height();
    let sheet_width = frame_width * frames.len() as u32;

    let mut sheet = RgbaImage::new(sheet_width, frame_height);

    for (i, frame) in frames.iter().enumerate() {
        if frame.width() != frame_width || frame.height() != frame_height {
            return Err(WanError::InvalidDataStructure(
                "All frames must have the same dimensions to be combined.".to_string(),
            ));
        }
        imageops::overlay(&mut sheet, frame, (i as u32 * frame_width) as i64, 0);
    }

    Ok(sheet)
}

/// Render an individual piece of a frame to the image
fn render_piece(
    wan: &WanFile,
    piece: &MetaFramePiece,
    image: &mut RgbaImage,
    pos: (i32, i32),
    dimensions: (usize, usize),
    palette: &[(u8, u8, u8, u8)],
) -> Result<bool, WanError> {
    let (width, height) = dimensions;
    if width == 0 || height == 0 || palette.is_empty() {
        return Ok(false);
    }

    let mut piece_img = RgbaImage::new(width as u32, height as u32);
    let mut has_visible_pixels = false;

    let is_256_colour_mode = piece.is_256_colour;
    let tile_num = piece.tile_num as usize;

    let pixel_buffer: &[u8] = if is_256_colour_mode {
        // Use the pre-computed lookup
        if let Some(ref lookup) = wan.tile_lookup_8bpp {
            if let Some(&chunk_idx) = lookup.get(&tile_num) {
                wan.img_data.get(chunk_idx).map_or(&[], |p| &p.img_px)
            } else {
                println!("    - ERROR: Tile {} not found in lookup", tile_num);
                &[]
            }
        } else {
            // Fallback to old method if no lookup available
            // this shouldn't happen for effect WANs
            &[]
        }
    } else {
        // For 4bpp, each tile is its own ImgPiece
        wan.img_data.get(tile_num).map_or(&[], |p| &p.img_px)
    };

    if pixel_buffer.is_empty() {
        return Ok(false);
    }

    let tiles_x = width / TEX_SIZE;
    let tiles_y = height / TEX_SIZE;

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            for y in 0..TEX_SIZE {
                for x in 0..TEX_SIZE {
                    let pixel_index_in_tile = y * TEX_SIZE + x;
                    let tile_index_in_piece = ty * tiles_x + tx;

                    let pal_idx = if is_256_colour_mode {
                        let byte_index_in_buffer = tile_index_in_piece * 64 + pixel_index_in_tile;
                        if byte_index_in_buffer >= pixel_buffer.len() {
                            continue;
                        }
                        pixel_buffer[byte_index_in_buffer] as usize
                    } else {
                        let byte_index_in_buffer =
                            tile_index_in_piece * 32 + (pixel_index_in_tile / 2);
                        if byte_index_in_buffer >= pixel_buffer.len() {
                            continue;
                        }
                        let byte = pixel_buffer[byte_index_in_buffer];
                        if pixel_index_in_tile % 2 == 0 {
                            (byte & 0x0F) as usize
                        } else {
                            (byte >> 4) as usize
                        }
                    };

                    if pal_idx > 0 && pal_idx < palette.len() {
                        let colour = palette[pal_idx];
                        if colour.3 > 0 {
                            let final_x = (tx * TEX_SIZE + x) as u32;
                            let final_y = (ty * TEX_SIZE + y) as u32;
                            piece_img.put_pixel(
                                final_x,
                                final_y,
                                Rgba([colour.0, colour.1, colour.2, colour.3]),
                            );
                            has_visible_pixels = true;
                        }
                    }
                }
            }
        }
    }

    if !has_visible_pixels {
        return Ok(false);
    }
    if piece.h_flip {
        piece_img = image::imageops::flip_horizontal(&piece_img);
    }
    if piece.v_flip {
        piece_img = image::imageops::flip_vertical(&piece_img);
    }
    imageops::overlay(image, &piece_img, pos.0 as i64, pos.1 as i64);

    Ok(has_visible_pixels)
}

/// Get the bounds of a frame
pub fn get_frame_bounds(wan: &WanFile, frame_idx: usize) -> Result<(i16, i16, i16, i16), WanError> {
    if frame_idx >= wan.frame_data.len() {
        return Err(WanError::OutOfBounds(format!(
            "Frame index {} out of bounds (max: {})",
            frame_idx,
            wan.frame_data.len() - 1
        )));
    }

    let frame = &wan.frame_data[frame_idx];
    if frame.pieces.is_empty() {
        return Ok((0, 0, 0, 0));
    }

    let mut bounds = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
    let mut has_pieces = false;

    for piece in &frame.pieces {
        let (width_blocks, height_blocks) = piece.get_dimensions();
        let width_px = (width_blocks * TEX_SIZE) as i16;
        let height_px = (height_blocks * TEX_SIZE) as i16;

        let piece_bounds = (
            piece.x_offset,
            piece.y_offset,
            piece.x_offset + width_px,
            piece.y_offset + height_px,
        );

        bounds.0 = bounds.0.min(piece_bounds.0);
        bounds.1 = bounds.1.min(piece_bounds.1);
        bounds.2 = bounds.2.max(piece_bounds.2);
        bounds.3 = bounds.3.max(piece_bounds.3);
        has_pieces = true;
    }

    if !has_pieces {
        return Ok((0, 0, 0, 0));
    }

    Ok(bounds)
}
