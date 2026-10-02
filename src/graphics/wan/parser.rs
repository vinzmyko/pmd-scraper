//! Parser for WAN sprite format
//!
//! This module provides functions to parse WAN files from binary data,
//! supporting both character and effect WAN variants.

use std::{
    collections::HashMap,
    io::{Cursor, Read, Seek, SeekFrom},
};

use crate::{
    binary_utils::{read_i16_le, read_u16_le, read_u32_le, read_u8},
    graphics::{
        wan::{
            model::{
                Animation, FrameOffset, ImgPiece, MetaFrame, MetaFramePiece, SequenceFrame, WanFile,
            },
            AnimationStructure, MetaFramePieceArgs, PaletteList, WanError,
        },
        WanType,
    },
};

/// Parse WAN file from SIR0 content that has already been extracted
pub fn parse_wan_from_sir0_content(
    content: &[u8],
    data_pointer: u32,
    wan_type: WanType,
) -> Result<WanFile, WanError> {
    let mut cursor = Cursor::new(content);
    cursor.seek(SeekFrom::Start(data_pointer as u64))?;

    match wan_type {
        WanType::Character => parse_character_wan(&mut cursor, content.len() as u64),
        WanType::Effect => parse_effect_wan(content, data_pointer),
    }
}

pub fn parse_character_wan(
    cursor: &mut Cursor<&[u8]>,
    buffer_size: u64,
) -> Result<WanFile, WanError> {
    // Store current position to check for minimal header
    let start_pos = cursor.position();

    // Make sure we have enough bytes for the basic header
    if start_pos + 8 > buffer_size {
        return Err(WanError::InvalidDataStructure(format!(
            "Not enough bytes for WAN header. Position: {}, Buffer size: {}",
            start_pos, buffer_size
        )));
    }

    let ptr_anim_info = read_u32_le(cursor).map_err(WanError::Io)?;
    let ptr_image_data_info = read_u32_le(cursor).map_err(WanError::Io)?;

    // Validate pointers
    if ptr_anim_info == 0 || ptr_image_data_info == 0 {
        return Err(WanError::InvalidDataStructure(
            "Null pointer in WAN header".to_string(),
        ));
    }

    if ptr_anim_info as u64 >= buffer_size || ptr_image_data_info as u64 >= buffer_size {
        return Err(WanError::InvalidDataStructure(format!(
            "Pointer out of bounds. anim_info: {:#x}, image_data_info: {:#x}, buffer size: {}",
            ptr_anim_info, ptr_image_data_info, buffer_size
        )));
    }

    // Should be 1 for character sprites
    let img_type = read_u16_le(cursor).map_err(WanError::Io)?;
    if img_type != 1 {
        return Err(WanError::InvalidDataStructure(format!(
            "Expected image type 1 for character sprite, got {}",
            img_type
        )));
    }

    // Skip unknown value
    read_u16_le(cursor).map_err(WanError::Io)?;

    // Read image data info with bounds checking
    if ptr_image_data_info as u64 >= buffer_size {
        return Err(WanError::InvalidDataStructure(format!(
            "Image data info pointer out of bounds: {:#x}, buffer size: {}",
            ptr_image_data_info, buffer_size
        )));
    }

    cursor
        .seek(SeekFrom::Start(ptr_image_data_info as u64))
        .map_err(WanError::Io)?;

    let ptr_image_data_table = read_u32_le(cursor).map_err(WanError::Io)?;
    let ptr_palette_info = read_u32_le(cursor).map_err(WanError::Io)?;

    // Validate pointers
    if ptr_image_data_table == 0 || ptr_palette_info == 0 {
        return Err(WanError::InvalidDataStructure(
            "Null pointer in Image Data Info".to_string(),
        ));
    }

    // Bounds check
    if ptr_image_data_table as u64 >= buffer_size || ptr_palette_info as u64 >= buffer_size {
        return Err(WanError::InvalidDataStructure(format!(
            "Pointer out of bounds. image_data_table: {:#x}, palette_info: {:#x}, buffer size: {}",
            ptr_image_data_table, ptr_palette_info, buffer_size
        )));
    }

    // Skip unknown values Unk#13, Is256ColourSpr, Unk#11
    read_u16_le(cursor).map_err(WanError::Io)?; // Unk#13 - ALWAYS 0
    read_u16_le(cursor).map_err(WanError::Io)?; // Is256ColourSpr - ALWAYS 0
    read_u16_le(cursor).map_err(WanError::Io)?; // Unk#11 - ALWAYS 1 unless empty

    // Read number of images
    let num_imgs = read_u16_le(cursor).map_err(WanError::Io)?;

    // Read palette info with bounds checking
    cursor
        .seek(SeekFrom::Start(ptr_palette_info as u64))
        .map_err(WanError::Io)?;

    let ptr_palette_data_block = read_u32_le(cursor).map_err(WanError::Io)?;

    // Bounds check palette data block
    if ptr_palette_data_block as u64 >= buffer_size {
        return Err(WanError::InvalidDataStructure(format!(
            "Palette data block pointer out of bounds: {:#x}, buffer size: {}",
            ptr_palette_data_block, buffer_size
        )));
    }

    // Skip unknown values (Unk#3, colours_per_row, Unk#4, Unk#5)
    read_u16_le(cursor).map_err(WanError::Io)?; // Unk#3 - ALWAYS 0
    let _colours_per_row_num = read_u16_le(cursor).map_err(WanError::Io)?;
    read_u16_le(cursor).map_err(WanError::Io)?; // Unk#4 - ALWAYS 0
    read_u16_le(cursor).map_err(WanError::Io)?; // Unk#5 - ALWAYS 255

    let palette_data = match read_palette_data(
        cursor,
        ptr_palette_data_block as u64,
        ptr_image_data_table as u64,
        16,
    ) {
        Ok(data) => data,
        Err(e) => {
            println!("  - Warning: Failed to read palette data: {:?}", e);
            println!("  - Using default palette");
            vec![vec![(0, 0, 0, 0); 16]]
        }
    };

    // Read image data table
    cursor
        .seek(SeekFrom::Start(ptr_image_data_table as u64))
        .map_err(WanError::Io)?;

    // Read pointers to image data
    let mut ptr_imgs = Vec::with_capacity(num_imgs as usize);
    for _ in 0..num_imgs {
        let ptr = read_u32_le(cursor).map_err(WanError::Io)?;
        ptr_imgs.push(ptr);
    }

    let img_data = match read_image_data(cursor, &ptr_imgs, buffer_size) {
        Ok(data) => data,
        Err(e) => {
            println!("  - Warning: Failed to read image data: {:?}", e);
            println!("  - Using empty image data");
            Vec::new()
        }
    };

    if ptr_anim_info as u64 >= buffer_size - 16 {
        // Need at least 16 bytes for header
        println!("  - Warning: Animation info is missing or invalid");
        return Ok(WanFile {
            img_data,
            frame_data: Vec::new(),
            animations: AnimationStructure::Character(Vec::new()),
            body_part_offset_data: Vec::new(),
            custom_palette: palette_data,
            effect_specific_palette: None,
            wan_type: WanType::Character,
            palette_offset: 0,
            tile_lookup_8bpp: None,
            max_sequences_per_group: 0,
            offset_table_size: 0,
        });
    }

    // Read animation info
    cursor
        .seek(SeekFrom::Start(ptr_anim_info as u64))
        .map_err(WanError::Io)?;

    let ptr_meta_frames_ref_table = read_u32_le(cursor).map_err(WanError::Io)?;
    let ptr_offsets_table = read_u32_le(cursor).map_err(WanError::Io)?;
    let ptr_anim_group_table = read_u32_le(cursor).map_err(WanError::Io)?;

    // Bounds check these pointers
    if ptr_meta_frames_ref_table as u64 >= buffer_size
        || ptr_offsets_table as u64 >= buffer_size
        || ptr_anim_group_table as u64 >= buffer_size
    {
        return Err(WanError::InvalidDataStructure(format!(
            "Animation pointers out of bounds. meta_frames: {:#x}, offsets: {:#x}, anim_group: {:#x}, buffer size: {}",
            ptr_meta_frames_ref_table, ptr_offsets_table, ptr_anim_group_table, buffer_size
        )));
    }

    let anim_groups_num = read_u16_le(cursor).map_err(WanError::Io)?;

    // Skip unknown values (Unk#6 through Unk#10)
    for _ in 0..5 {
        read_u16_le(cursor).map_err(WanError::Io)?;
    }

    // Read animation groups
    let (animation_groups, anim_sequences) = match read_animation_groups(
        cursor,
        ptr_anim_group_table as u64,
        anim_groups_num as usize,
    ) {
        Ok(result) => result,
        Err(e) => {
            println!("  - Warning: Failed to read animation groups: {:?}", e);
            (Vec::new(), Vec::new())
        }
    };

    // Read meta frames
    let (meta_frames, pointer_count) = match read_meta_frames(
        cursor,
        ptr_meta_frames_ref_table as u64,
        ptr_offsets_table as u64,
    ) {
        Ok(result) => result,
        Err(e) => {
            println!("  - Warning: Failed to read meta frames: {:?}", e);
            (Vec::new(), 0)
        }
    };

    // Read offset data (use pointer_count, not meta_frames.len(), because
    // the double-push hack inflates meta_frames but the ROM offset table
    // has exactly one entry per pointer)
    let offset_data = match read_offset_data(cursor, ptr_offsets_table as u64, pointer_count) {
        Ok(offsets) => offsets,
        Err(e) => {
            println!("  - Warning: Failed to read offset data: {:?}", e);
            Vec::new()
        }
    };

    // Read animation sequences
    let animation_data =
        match read_animation_sequence_character(cursor, &animation_groups, &anim_sequences) {
            Ok(data) => data,
            Err(e) => {
                println!("  - Warning: Failed to read animation sequences: {:?}", e);
                Vec::new()
            }
        };

    let frame_data = meta_frames;

    Ok(WanFile {
        img_data,
        frame_data,
        animations: AnimationStructure::Character(animation_data),
        body_part_offset_data: offset_data,
        custom_palette: palette_data,
        effect_specific_palette: None,
        wan_type: WanType::Character,
        palette_offset: 0,
        tile_lookup_8bpp: None,
        max_sequences_per_group: 8,
        offset_table_size: pointer_count,
    })
}

fn read_animation_sequence_character(
    cursor: &mut Cursor<&[u8]>,
    animation_groups: &[Vec<u32>],
    _anim_sequences: &[u32],
) -> Result<Vec<Vec<Animation>>, WanError> {
    let buffer_size = cursor.get_ref().len() as u64;
    let mut all_animations = Vec::new();

    // The character animation structure is a set of groups, and each group
    // contains pointers to animations for different directions
    for group in animation_groups {
        let mut group_animations = Vec::new();

        for &ptr in group {
            if ptr == 0 {
                group_animations.push(Animation::empty());
                continue;
            }
            if ptr as u64 >= buffer_size {
                group_animations.push(Animation::empty());
                continue;
            }

            cursor.seek(SeekFrom::Start(ptr as u64))?;
            let mut sequence_frames = Vec::new();

            loop {
                if cursor.position() + 12 > buffer_size {
                    break;
                }

                // Character sequence frames are 12 bytes long
                let frame_dur = read_u8(cursor)?;
                if frame_dur == 0 {
                    cursor.seek(SeekFrom::Current(11))?;
                    break;
                }
                let flag = read_u8(cursor)?;
                let frame_index = read_u16_le(cursor)?;
                let spr_off_x = read_i16_le(cursor)?;
                let spr_off_y = read_i16_le(cursor)?;
                let sdw_off_x = read_i16_le(cursor)?;
                let sdw_off_y = read_i16_le(cursor)?;

                sequence_frames.push(SequenceFrame::new(
                    frame_index,
                    frame_dur as u16,
                    flag,
                    (spr_off_x, spr_off_y),
                    (sdw_off_x, sdw_off_y),
                ));
            }
            group_animations.push(Animation::new(sequence_frames));
        }
        all_animations.push(group_animations);
    }

    Ok(all_animations)
}

fn parse_effect_wan(data: &[u8], ptr_wan: u32) -> Result<WanFile, WanError> {
    let mut cursor = Cursor::new(data);
    cursor.seek(SeekFrom::Start(ptr_wan as u64))?;

    let ptr_anim_info = read_u32_le(&mut cursor)?;
    let ptr_image_data_info = read_u32_le(&mut cursor)?;

    if ptr_image_data_info == 0 && ptr_anim_info == 0 {
        return Err(WanError::InvalidDataStructure(
            "Null AnimInfo and ImageDataInfo pointers".to_string(),
        ));
    }

    let mut custom_palette = vec![];
    let mut img_data = vec![];
    let mut is_256_colour_val = 0;
    let mut palette_offset = 0;

    if ptr_image_data_info > 0 {
        cursor.seek(SeekFrom::Start(ptr_image_data_info as u64))?;
        let ptr_image_data_table = read_u32_le(&mut cursor)?;
        let ptr_palette_info = read_u32_le(&mut cursor)?;
        read_u16_le(&mut cursor)?;
        is_256_colour_val = read_u16_le(&mut cursor)?;
        read_u16_le(&mut cursor)?;
        let nb_imgs = read_u16_le(&mut cursor)?;
        if ptr_palette_info > 0 {
            cursor.seek(SeekFrom::Start(ptr_palette_info as u64))?;
            let ptr_palette_data_block = read_u32_le(&mut cursor)?;
            read_u16_le(&mut cursor)?;
            read_u16_le(&mut cursor)?;
            read_u16_le(&mut cursor)?;
            let unk5 = read_u16_le(&mut cursor)?;
            palette_offset = unk5 % 16;
            custom_palette = read_effect_palette_data(
                &mut cursor,
                ptr_palette_data_block as u64,
                ptr_palette_info as u64,
                is_256_colour_val as usize,
            )?;
        }
        if ptr_image_data_table > 0 {
            cursor.seek(SeekFrom::Start(ptr_image_data_table as u64))?;
            let mut ptr_imgs = Vec::with_capacity(nb_imgs as usize);
            for _ in 0..nb_imgs {
                ptr_imgs.push(read_u32_le(&mut cursor)?);
            }
            if is_256_colour_val != 0 {
                // Create one ImgPiece per image chunk
                for &ptr_img in &ptr_imgs {
                    if ptr_img == 0 {
                        img_data.push(ImgPiece { img_px: vec![] });
                        continue;
                    }
                    cursor.seek(SeekFrom::Start(ptr_img as u64))?;
                    let mut chunk_pixels = Vec::new();
                    loop {
                        if cursor.position() + 12 > data.len() as u64 {
                            break;
                        }
                        let ptr_pix_src = read_u32_le(&mut cursor)?;
                        let amt = read_u16_le(&mut cursor)?;
                        if ptr_pix_src == 0 && amt == 0 {
                            break;
                        }
                        cursor.seek(SeekFrom::Current(6))?;
                        if (ptr_pix_src as u64) < data.len() as u64 && amt > 0 {
                            let current_pos = cursor.position();
                            cursor.seek(SeekFrom::Start(ptr_pix_src as u64))?;
                            let read_size = (amt as usize).min(data.len() - ptr_pix_src as usize);
                            chunk_pixels.extend_from_slice(
                                &data[ptr_pix_src as usize..ptr_pix_src as usize + read_size],
                            );
                            cursor.seek(SeekFrom::Start(current_pos))?;
                        }
                    }
                    img_data.push(ImgPiece {
                        img_px: chunk_pixels,
                    }); // One piece per chunk
                }
            } else {
                for &ptr_img in &ptr_imgs {
                    if ptr_img == 0 {
                        img_data.push(ImgPiece { img_px: vec![] });
                        continue;
                    }
                    cursor.seek(SeekFrom::Start(ptr_img as u64))?;
                    let mut tile_pixels = Vec::new();
                    loop {
                        if cursor.position() + 12 > data.len() as u64 {
                            break;
                        }
                        let ptr_pix_src = read_u32_le(&mut cursor)?;
                        let amt = read_u16_le(&mut cursor)?;
                        if ptr_pix_src == 0 && amt == 0 {
                            break;
                        }
                        cursor.seek(SeekFrom::Current(6))?;
                        if (ptr_pix_src as u64) < data.len() as u64 && amt > 0 {
                            let current_pos = cursor.position();
                            cursor.seek(SeekFrom::Start(ptr_pix_src as u64))?;
                            let read_size = (amt as usize).min(data.len() - ptr_pix_src as usize);
                            tile_pixels.extend_from_slice(
                                &data[ptr_pix_src as usize..ptr_pix_src as usize + read_size],
                            );
                            cursor.seek(SeekFrom::Start(current_pos))?;
                        }
                    }
                    img_data.push(ImgPiece {
                        img_px: tile_pixels,
                    });
                }
            }
        }
    }

    let tile_lookup_8bpp = if is_256_colour_val != 0 {
        Some(build_8bpp_tile_lookup(&img_data))
    } else {
        None
    };

    let mut frame_data = vec![];
    let mut max_sequences_per_group: u16 = 1;
    let mut animation_groups: Vec<Vec<Animation>> = Vec::new();

    if ptr_anim_info > 0 {
        cursor.seek(SeekFrom::Start(ptr_anim_info as u64))?;
        let ptr_meta_frames_ref_table = read_u32_le(&mut cursor)?;
        read_u32_le(&mut cursor)?;
        let ptr_anim_group_table = read_u32_le(&mut cursor)?;
        let nb_anim_groups = read_u16_le(&mut cursor)?;

        // Parse the animation group table to get the list of animation sequence pointers
        // Seek to start of anim group table to get the meta frame table boundary
        cursor.seek(SeekFrom::Start(ptr_anim_group_table as u64))?;
        let meta_frames_end_ptr = read_u32_le(&mut cursor)?;

        // Parse the meta-frames.
        cursor.seek(SeekFrom::Start(ptr_meta_frames_ref_table as u64))?;
        frame_data =
            read_effect_meta_frames(&mut cursor, meta_frames_end_ptr, is_256_colour_val != 0)?;

        // Parse animation groups - store per-group for proper ROM behavior
        // ROM uses animation_index as sequence index into group 0 ONLY
        cursor.seek(SeekFrom::Start(ptr_anim_group_table as u64))?;
        for _ in 0..nb_anim_groups {
            let anim_loc = read_u32_le(&mut cursor)?;
            let anim_length = read_u16_le(&mut cursor)?;
            read_u16_le(&mut cursor)?; // skip loop_start

            let mut group_animations = Vec::new();

            if anim_loc > 0 && anim_length > 0 {
                let current_pos = cursor.position();
                cursor.seek(SeekFrom::Start(anim_loc as u64))?;

                if anim_length > max_sequences_per_group {
                    max_sequences_per_group = anim_length;
                }

                for _ in 0..anim_length {
                    let seq_ptr = read_u32_le(&mut cursor)?;
                    if seq_ptr > 0 {
                        let inner_pos = cursor.position();
                        let sequence = read_animation_sequence(&mut cursor, seq_ptr)?;
                        group_animations.push(sequence);
                        cursor.seek(SeekFrom::Start(inner_pos))?;
                    } else {
                        group_animations.push(Animation::empty());
                    }
                }

                cursor.seek(SeekFrom::Start(current_pos))?;
            }

            animation_groups.push(group_animations);
        }
    }

    Ok(WanFile {
        img_data,
        frame_data,
        animations: AnimationStructure::Effect(animation_groups),
        body_part_offset_data: vec![],
        custom_palette,
        effect_specific_palette: None,
        wan_type: WanType::Effect,
        palette_offset,
        tile_lookup_8bpp,
        max_sequences_per_group,
        offset_table_size: 0,
    })
}

fn build_8bpp_tile_lookup(img_data: &[ImgPiece]) -> HashMap<usize, usize> {
    let mut lookup = HashMap::new();
    let mut current_byte_pos = 0;
    let block_size = 128; // 2 * TEX_SIZE * TEX_SIZE for 8bpp

    for (chunk_idx, piece) in img_data.iter().enumerate() {
        // Map tile position to chunk index
        let tile_num = current_byte_pos / block_size;
        lookup.insert(tile_num, chunk_idx);

        current_byte_pos += piece.img_px.len();
        // Apply padding to align to next 128-byte boundary
        if current_byte_pos % block_size != 0 {
            current_byte_pos = ((current_byte_pos / block_size) + 1) * block_size;
        }
    }
    lookup
}

fn read_animation_sequence(cursor: &mut Cursor<&[u8]>, ptr: u32) -> Result<Animation, WanError> {
    let original_pos = cursor.position();
    cursor.seek(SeekFrom::Start(ptr as u64))?;
    let mut frames = Vec::new();
    loop {
        if cursor.position() + 12 > cursor.get_ref().len() as u64 {
            break;
        }

        let _pos = cursor.position();
        let frame_dur = read_u8(cursor)?;
        let flag = read_u8(cursor)?;

        if frame_dur == 0 {
            break;
        }

        let frame_index = read_u16_le(cursor)?;
        let spr_off_x = read_u16_le(cursor)?;
        let spr_off_y = read_u16_le(cursor)?;
        read_u16_le(cursor)?;
        read_u16_le(cursor)?;

        frames.push(SequenceFrame::new(
            frame_index,
            frame_dur as u16,
            flag,
            (spr_off_x as i16, spr_off_y as i16),
            (0, 0),
        ));
    }
    cursor.seek(SeekFrom::Start(original_pos))?;
    Ok(Animation::new(frames))
}

fn read_effect_meta_frames(
    cursor: &mut Cursor<&[u8]>,
    end_ptr: u32,
    is_256_colour_file: bool,
) -> Result<Vec<MetaFrame>, WanError> {
    let mut ptrs = Vec::new();
    let end_pos = end_ptr as u64;
    let read_end = end_pos.min(cursor.get_ref().len() as u64);

    while cursor.position() < read_end {
        if cursor.position() + 4 > read_end {
            break;
        }
        let ptr = read_u32_le(cursor)?;
        ptrs.push(ptr);
    }

    let mut meta_frames = Vec::with_capacity(ptrs.len());

    for ptr in ptrs {
        if ptr as u64 >= cursor.get_ref().len() as u64 {
            meta_frames.push(MetaFrame { pieces: vec![] });
            continue;
        }

        cursor.seek(SeekFrom::Start(ptr as u64))?;
        let mut pieces = Vec::new();

        loop {
            if cursor.position() + 10 > cursor.get_ref().len() as u64 {
                break;
            }

            let piece_start = cursor.position() as usize;
            let data: &[u8] = *cursor.get_ref();
            let b = &data[piece_start..piece_start + 10];
            let raw = [0, 2, 4, 6, 8].map(|i| u16::from_le_bytes([b[i], b[i + 1]]));

            cursor.read_exact(&mut [0u8; 3])?;
            let _draw_value = read_u8(cursor)?;
            let y_data = read_u16_le(cursor)?;
            let x_data = read_u16_le(cursor)?;
            let tile_num = read_u8(cursor)?;
            let palette_data = read_u8(cursor)?;

            let y_offset = (y_data % 1024) as i16;
            let x_offset = (x_data % 512) as i16;

            let dim_data = x_data / 2048;
            let is_last = (dim_data % 2) == 1;
            let dim_type = y_data / 16384;
            let h_flip = (dim_data / 2 % 2) == 1;
            let v_flip = (dim_data / 4 % 2) == 1;
            let resolution_idx = (dim_type * 4 + dim_data / 8) as usize;
            let palette_index = (palette_data / 16) as u8;

            pieces.push(MetaFramePiece::new(MetaFramePieceArgs {
                tile_num: tile_num as u16,
                palette_index,
                h_flip,
                v_flip,
                x_offset,
                y_offset,
                resolution_idx,
                is_256_colour: is_256_colour_file,
                raw,
            }));

            if is_last {
                break;
            }
        }

        meta_frames.push(MetaFrame { pieces });
    }
    Ok(meta_frames)
}

/// Parse WAN file focusing only on palette data (for special palette-only files like effect.bin[292])
pub fn parse_wan_palette_only(content: &[u8], data_pointer: u32) -> Result<WanFile, WanError> {
    let mut cursor = Cursor::new(content);
    let buffer_size = content.len() as u64;
    cursor.seek(SeekFrom::Start(data_pointer as u64))?;

    let _ptr_anim_info = read_u32_le(&mut cursor)?;
    let ptr_image_data_info = read_u32_le(&mut cursor)?;

    if ptr_image_data_info == 0 || (ptr_image_data_info as u64) >= buffer_size {
        return Err(WanError::InvalidDataStructure(format!(
            "Invalid ptr_image_data_info (0x{:X})",
            ptr_image_data_info
        )));
    }

    cursor.seek(SeekFrom::Start(ptr_image_data_info as u64))?;
    let _ptr_image_data_table = read_u32_le(&mut cursor)?;
    let ptr_palette_info = read_u32_le(&mut cursor)?;
    read_u16_le(&mut cursor)?; // unk13
    let is_256_colour_val = read_u16_le(&mut cursor)?;

    if ptr_palette_info == 0 || (ptr_palette_info as u64) >= buffer_size {
        return Err(WanError::InvalidDataStructure(format!(
            "Invalid ptr_palette_info (0x{:X})",
            ptr_palette_info
        )));
    }

    cursor.seek(SeekFrom::Start(ptr_palette_info as u64))?;
    let ptr_palette_data_block = read_u32_le(&mut cursor)?;
    read_u16_le(&mut cursor)?; // Unk3
    read_u16_le(&mut cursor)?; // total_colours_header
    read_u16_le(&mut cursor)?; // Unk4
    let palette_offset_info = read_u16_le(&mut cursor)?;

    if ptr_palette_data_block == 0 || (ptr_palette_data_block as u64) >= buffer_size {
        return Err(WanError::InvalidDataStructure(format!(
            "Invalid ptr_palette_data_block (0x{:X})",
            ptr_palette_data_block
        )));
    }

    let palette_end_ptr = content.len() as u64;

    let palette_data = read_effect_palette_data(
        &mut cursor,
        ptr_palette_data_block as u64,
        palette_end_ptr,
        is_256_colour_val as usize,
    )?;

    if palette_data.is_empty() {
        return Err(WanError::InvalidDataStructure(
            "Palette data parsed as empty in palette-only mode".to_string(),
        ));
    }

    Ok(WanFile {
        img_data: Vec::new(),
        frame_data: Vec::new(),
        animations: AnimationStructure::Effect(Vec::new()),
        body_part_offset_data: Vec::new(),
        custom_palette: palette_data,
        effect_specific_palette: None,
        wan_type: WanType::Effect,
        palette_offset: palette_offset_info,
        tile_lookup_8bpp: None,
        max_sequences_per_group: 0,
        offset_table_size: 0,
    })
}

/// Read palette data from the WAN file
fn read_palette_data(
    cursor: &mut Cursor<&[u8]>,
    ptr_palette_data_block: u64,
    end_ptr: u64,
    nb_colours_per_row: usize,
) -> Result<PaletteList, WanError> {
    debug_assert!(
        ptr_palette_data_block > 0,
        "Palette data block pointer is zero"
    );
    debug_assert!(end_ptr > 0, "End pointer is zero");
    debug_assert!(
        end_ptr > ptr_palette_data_block,
        "Invalid palette block range"
    );

    let _buffer_size = cursor.get_ref().len() as u64;

    cursor
        .seek(SeekFrom::Start(ptr_palette_data_block))
        .map_err(|e| {
            println!("ERROR: Failed to seek to palette data block");
            WanError::Io(e)
        })?;

    let total_colours = ((end_ptr - ptr_palette_data_block) / 4) as usize;

    let total_palettes = if nb_colours_per_row == 0 {
        0
    } else {
        total_colours / nb_colours_per_row
    };

    let mut custom_palette = Vec::with_capacity(total_palettes);

    for _ in 0..total_palettes {
        let mut palette = Vec::with_capacity(nb_colours_per_row);

        for _ in 0..nb_colours_per_row {
            let red = read_u8(cursor).map_err(|e| {
                println!("ERROR: Failed to read red component");
                WanError::Io(e)
            })?;

            let blue = read_u8(cursor).map_err(|e| {
                println!("ERROR: Failed to read blue component");
                WanError::Io(e)
            })?;

            let green = read_u8(cursor).map_err(|e| {
                println!("ERROR: Failed to read green component");
                WanError::Io(e)
            })?;

            let _ = read_u8(cursor).map_err(|e| {
                println!("ERROR: Failed to read alpha component");
                WanError::Io(e)
            })?;

            palette.push((red, blue, green, 255));
        }

        ensure_complete_palette(&mut palette);

        custom_palette.push(palette);
    }

    if custom_palette.is_empty() {
        println!("  No palettes found, creating default palette");
        let mut default_palette = vec![(0, 0, 0, 0)];
        ensure_complete_palette(&mut default_palette);
        custom_palette.push(default_palette);
    }

    Ok(custom_palette)
}

/// Ensure a palette has all 16 colours, without modifying existing colours
fn ensure_complete_palette(palette: &mut Vec<(u8, u8, u8, u8)>) {
    if palette.is_empty() {
        palette.push((0, 0, 0, 0));
    }

    if palette.len() < 16 {
        let default_colours: [(u8, u8, u8, u8); 15] = [
            (128, 128, 128, 255), // Gray
            (192, 192, 192, 255), // Light gray
            (96, 96, 96, 255),    // Dark gray
            (160, 160, 160, 255), // Medium gray
            (224, 224, 224, 255), // Very light gray
            (64, 64, 64, 255),    // Very dark gray
            (128, 96, 64, 255),   // Brown
            (192, 160, 128, 255), // Tan
            (64, 96, 128, 255),   // Blue-gray
            (224, 224, 192, 255), // Cream
            (96, 128, 96, 255),   // Moss green
            (160, 192, 160, 255), // Sage
            (128, 64, 64, 255),   // Brick red
            (192, 128, 128, 255), // Dusty rose
            (255, 255, 255, 255), // White
        ];

        let needed = 16 - palette.len();
        palette.extend(default_colours.iter().take(needed));

        while palette.len() < 16 {
            let val = (palette.len() as u8) * 16;
            palette.push((val, val, val, 255));
        }
    }

    while palette.len() > 16 {
        palette.pop();
    }
}

/// Read palette data for Effect WAN with special handling for 256-colour mode
fn read_effect_palette_data(
    cursor: &mut Cursor<&[u8]>,
    ptr_palette_data_block: u64,
    end_ptr: u64,
    is_256_colour: usize,
) -> Result<PaletteList, WanError> {
    cursor
        .seek(SeekFrom::Start(ptr_palette_data_block))
        .map_err(WanError::Io)?;

    let total_bytes = end_ptr - ptr_palette_data_block;
    let mut custom_palette = Vec::new();

    if is_256_colour == 1 || is_256_colour == 4 {
        // 256-colour (8bpp) mode
        let num_palettes = (total_bytes as usize / (16 * 4)).max(1);
        for _ in 0..num_palettes {
            let mut palette_row = vec![(0, 0, 0, 0); 256];

            for j in 0..16 {
                if cursor.position() + 4 > end_ptr {
                    break;
                }

                let r_raw = read_u8(cursor)?;
                let b_raw = read_u8(cursor)?;
                let g_raw = read_u8(cursor)?;
                let _padding = read_u8(cursor)?;

                let r = (((r_raw as u32 / 8 * 8) * 32) / 31).min(255) as u8;
                let g = (((g_raw as u32 / 8 * 8) * 32) / 31).min(255) as u8;
                let b = (((b_raw as u32 / 8 * 8) * 32) / 31).min(255) as u8;

                palette_row[16 + j] = (r, b, g, 255);
            }
            custom_palette.push(palette_row);
        }
    } else {
        // 16-colour (4bpp) mode
        let colours_per_row = 16;
        let bytes_per_row = colours_per_row * 4;
        let num_rows = (total_bytes as usize) / bytes_per_row;
        for _ in 0..num_rows {
            let mut palette_row = Vec::with_capacity(colours_per_row);
            for colour_idx in 0..colours_per_row {
                if cursor.position() + 4 > end_ptr {
                    break;
                }
                let r = read_u8(cursor)?;
                let b = read_u8(cursor)?; // Reads B then G
                let g = read_u8(cursor)?;
                let _padding = read_u8(cursor)?;

                let alpha = if colour_idx == 0 { 0 } else { 255 };
                palette_row.push((r, b, g, alpha));
            }
            if palette_row.len() == colours_per_row {
                custom_palette.push(palette_row);
            }
        }
    }

    Ok(custom_palette)
}

/// Read image data from the WAN file
fn read_image_data(
    cursor: &mut Cursor<&[u8]>,
    ptr_imgs: &[u32],
    _buffer_size: u64,
) -> Result<Vec<ImgPiece>, WanError> {
    let mut img_data = Vec::with_capacity(ptr_imgs.len());

    for (img_idx, &ptr_img) in ptr_imgs.iter().enumerate() {
        if let Err(e) = cursor.seek(SeekFrom::Start(ptr_img as u64)) {
            println!(
                "  - Warning: Failed to seek to image data for image #{}: {}",
                img_idx, e
            );
            img_data.push(ImgPiece { img_px: Vec::new() });
            continue;
        }

        let mut tile_pixels = Vec::new();
        let mut valid_data = false;

        loop {
            let ptr_pix_src = match read_u32_le(cursor) {
                Ok(val) => val,
                Err(e) => {
                    if tile_pixels.is_empty() {
                        println!(
                            "  - Warning: Failed to read pixel source pointer for image #{}: {}",
                            img_idx, e
                        );
                    }
                    break;
                }
            };

            let num_pixels_to_read = match read_u16_le(cursor) {
                Ok(val) => val,
                Err(e) => {
                    println!(
                        "  - Warning: Failed to read pixel amount for image #{}: {}",
                        img_idx, e
                    );
                    break;
                }
            };

            if ptr_pix_src == 0 && num_pixels_to_read == 0 {
                break;
            }

            if let Err(e) = read_u16_le(cursor) {
                println!(
                    "  - Warning: Failed to read unknown field for image #{}: {}",
                    img_idx, e
                );
                break;
            }

            if let Err(e) = read_u32_le(cursor) {
                println!(
                    "  - Warning: Failed to read z-sort value for image #{}: {}",
                    img_idx, e
                );
            };

            if ptr_pix_src == 0 {
                tile_pixels.extend(vec![0; num_pixels_to_read as usize]);
                valid_data = true;
            } else {
                let current_pos = cursor.position();

                if cursor.seek(SeekFrom::Start(ptr_pix_src as u64)).is_err() {
                    if let Err(seek_e) = cursor.seek(SeekFrom::Start(current_pos)) {
                        println!("  - Warning: Failed to restore position: {}", seek_e);
                    }
                    continue;
                }

                let mut buffer = vec![0; num_pixels_to_read as usize];
                match cursor.read_exact(&mut buffer) {
                    Ok(_) => {
                        tile_pixels.extend(buffer);
                        valid_data = true;
                    }
                    Err(e) => {
                        println!(
                            "  - Warning: Partial read for image #{} at position {}: {}",
                            img_idx,
                            cursor.position(),
                            e,
                        );
                        break;
                    }
                }

                if let Err(e) = cursor.seek(SeekFrom::Start(current_pos)) {
                    println!("  - Warning: Failed to restore position after reading pixels for image #{}: {}", 
                        img_idx, e);
                    break;
                }
            }
        }

        if valid_data && !tile_pixels.is_empty() {
            img_data.push(ImgPiece {
                img_px: tile_pixels,
            });
        } else {
            println!(
                "  - No valid pixel data for image #{}, adding empty placeholder",
                img_idx
            );
            img_data.push(ImgPiece { img_px: Vec::new() });
        }
    }
    Ok(img_data)
}

fn read_meta_frames(
    cursor: &mut Cursor<&[u8]>,
    ptr_meta_frames_ref_table: u64,
    ptr_frames_ref_table_end: u64,
) -> Result<(Vec<MetaFrame>, usize), WanError> {
    cursor.seek(SeekFrom::Start(ptr_meta_frames_ref_table))?;
    let mut ptrs = Vec::new();
    while cursor.position() < ptr_frames_ref_table_end {
        if let Ok(ptr) = read_u32_le(cursor) {
            // Pushing when ptr == 0 means pushing null ptrs this allows us to get the starting
            // frame, but pads the blank frame to the ptrs vec
            ptrs.push(ptr);
        } else {
            break;
        }
    }

    let mut meta_frames = Vec::with_capacity(ptrs.len());
    for &ptr in &ptrs {
        if ptr == 0 {
            meta_frames.push(MetaFrame { pieces: vec![] });
        }

        if (ptr as u64) >= cursor.get_ref().len() as u64 {
            meta_frames.push(MetaFrame { pieces: vec![] });
            continue;
        }

        cursor.seek(SeekFrom::Start(ptr as u64))?;
        let mut pieces = Vec::new();
        loop {
            if cursor.position() + 10 > cursor.get_ref().len() as u64 {
                break;
            }
            let img_index = read_i16_le(cursor)?;
            let _unk0 = read_u16_le(cursor)?;
            let attr0 = read_u16_le(cursor)?;
            let attr1 = read_u16_le(cursor)?;
            let attr2 = read_u16_le(cursor)?;

            // Y is an 8-bit signed value
            let y_offset = (attr0 & 0xFF) as u8 as i8 as i16;

            // X is a 9-bit unsigned value that needs to be centred
            let x_raw = (attr1 & 0x1FF) as i16;
            let x_offset = x_raw - 256;

            let shape = (attr0 >> 14) & 0x3;
            let size = (attr1 >> 14) & 0x3;
            let resolution_idx = ((shape << 2) | size) as usize;

            let h_flip = (attr1 & super::flags::ATTR1_HFLIP_MASK) != 0;
            let v_flip = (attr1 & super::flags::ATTR1_VFLIP_MASK) != 0;
            let is_256_colour = (attr0 & super::flags::ATTR0_COL_PAL_MASK) != 0;
            let palette_index = ((attr2 & super::flags::ATTR2_PAL_NUMBER_MASK) >> 12) as u8;

            pieces.push(MetaFramePiece::new(MetaFramePieceArgs {
                tile_num: img_index as u16,
                palette_index,
                h_flip,
                v_flip,
                x_offset,
                y_offset,
                resolution_idx,
                is_256_colour,
                raw: [img_index as u16, _unk0, attr0, attr1, attr2],
            }));

            if (attr1 & super::flags::ATTR1_IS_LAST_MASK) != 0 {
                break;
            }
        }
        meta_frames.push(MetaFrame { pieces });
    }
    Ok((meta_frames, ptrs.len()))
}

/// Read offset data from the WAN file
fn read_offset_data(
    cursor: &mut Cursor<&[u8]>,
    ptr_offsets_table: u64,
    num_frames: usize,
) -> Result<Vec<FrameOffset>, WanError> {
    cursor
        .seek(SeekFrom::Start(ptr_offsets_table))
        .map_err(WanError::Io)?;

    let mut offset_data = Vec::with_capacity(num_frames);

    for _ in 0..num_frames {
        let head_x = read_i16_le(cursor).map_err(WanError::Io)?;
        let head_y = read_i16_le(cursor).map_err(WanError::Io)?;

        let lhand_x = read_i16_le(cursor).map_err(WanError::Io)?;
        let lhand_y = read_i16_le(cursor).map_err(WanError::Io)?;

        let rhand_x = read_i16_le(cursor).map_err(WanError::Io)?;
        let rhand_y = read_i16_le(cursor).map_err(WanError::Io)?;

        let centre_x = read_i16_le(cursor).map_err(WanError::Io)?;
        let centre_y = read_i16_le(cursor).map_err(WanError::Io)?;

        offset_data.push(FrameOffset::new(
            (head_x, head_y),
            (lhand_x, lhand_y),
            (rhand_x, rhand_y),
            (centre_x, centre_y),
        ));
    }

    Ok(offset_data)
}

/// Read animation groups from the WAN file
fn read_animation_groups(
    cursor: &mut Cursor<&[u8]>,
    ptr_anim_group_table: u64,
    num_anim_groups: usize,
) -> Result<(Vec<Vec<u32>>, Vec<u32>), WanError> {
    cursor
        .seek(SeekFrom::Start(ptr_anim_group_table))
        .map_err(WanError::Io)?;

    let mut anim_groups: Vec<Vec<u32>> = Vec::with_capacity(num_anim_groups);
    let mut anim_sequences: Vec<u32> = Vec::new();
    let buffer_size = cursor.get_ref().len() as u64;

    for _group_idx in 0..num_anim_groups {
        let anim_loc = read_u32_le(cursor).map_err(WanError::Io)?;
        let anim_length = read_u16_le(cursor).map_err(WanError::Io)?;

        // Skip unknown value
        read_u16_le(cursor).map_err(WanError::Io)?;

        let current_pos = cursor.position();

        // Skip empty groups or validate pointer bounds
        if anim_loc == 0
            || anim_length == 0
            || (anim_loc as u64) >= buffer_size
            || (anim_loc as u64) + ((anim_length as u64) * 4) > buffer_size
        {
            anim_groups.push(Vec::new());
            continue;
        }

        if cursor.seek(SeekFrom::Start(anim_loc as u64)).is_err() {
            anim_groups.push(Vec::new());
            // Restore position and continue with next group
            cursor.seek(SeekFrom::Start(current_pos))?;
            continue;
        }

        let mut anim_ptrs = Vec::with_capacity(anim_length as usize);
        for _dir_idx in 0..anim_length {
            // On read error, assume invalid pointer
            let anim_ptr = read_u32_le(cursor).unwrap_or_default();

            // Only store valid pointers
            if anim_ptr > 0 && (anim_ptr as u64) < buffer_size {
                anim_ptrs.push(anim_ptr);
                anim_sequences.push(anim_ptr);
            } else {
                anim_ptrs.push(0);
            }
        }

        anim_groups.push(anim_ptrs);

        // Restore position for next group header
        cursor.seek(SeekFrom::Start(current_pos))?;
    }

    Ok((anim_groups, anim_sequences))
}
