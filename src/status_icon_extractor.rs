//! Loads overlay 29, parse the SMA file, and extract the status icons.

use std::{
    fs,
    io::{self, Cursor, Seek, SeekFrom},
    path::Path,
};

use image::{Rgba, RgbaImage};
use serde::Serialize;

use crate::{
    binary_utils::{read_u16_le, read_u32_le, read_u8},
    containers::sir0::Sir0,
    phases::PhaseId,
    progress::ProgressReporter,
    rom::Rom,
};

const TEX_SIZE: usize = 8;

#[derive(Debug, Clone)]
struct SmaFile {
    anim_data: Vec<SmaAnimation>,
    /// Image data expanded to one nibble per element (palette indices 0-15).
    img_data: Vec<u8>,
    /// 16 palettes of 16 colours each. (R, G, B, A) — index 0 is transparent.
    custom_palette: Vec<Vec<(u8, u8, u8, u8)>>,
}

#[derive(Debug, Clone)]
struct SmaAnimation {
    block_width: u8,
    block_height: u8,
    byte_offset: u16,
    frame_count: u16,
}

fn parse_sma(raw_data: &[u8]) -> io::Result<SmaFile> {
    let sir0 = Sir0::from_bytes(raw_data)?;
    let content = &sir0.content;
    let data_pointer = sir0.data_pointer;

    let mut cursor = Cursor::new(content.as_slice());
    cursor.seek(SeekFrom::Start(data_pointer as u64))?;

    // Content header: 8 × u32 = 32 bytes
    let _unk1 = read_u32_le(&mut cursor)?;
    let ptr_anim_data = read_u32_le(&mut cursor)?;
    let num_animations = read_u32_le(&mut cursor)?;
    let ptr_img_data = read_u32_le(&mut cursor)?;
    let _unk2 = read_u32_le(&mut cursor)?;
    let ptr_palette_data = read_u32_le(&mut cursor)?;
    let _unk3 = read_u32_le(&mut cursor)?;
    let _unk4 = read_u32_le(&mut cursor)?;

    let content_len = content.len();
    if ptr_anim_data as usize >= content_len
        || ptr_img_data as usize >= content_len
        || ptr_palette_data as usize >= content_len
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "SMA pointer out of bounds: anim={:#x} img={:#x} pal={:#x} len={:#x}",
                ptr_anim_data, ptr_img_data, ptr_palette_data, content_len
            ),
        ));
    }

    // Palette: region spans from ptr_palette_data to the content header (data_pointer)
    let palette = {
        let total_bytes = (data_pointer as usize).saturating_sub(ptr_palette_data as usize);
        let colours_per_row = 16usize;
        let total_palettes = total_bytes / 4 / colours_per_row;
        let mut palettes = Vec::with_capacity(total_palettes);
        let mut pos = ptr_palette_data as usize;

        for _ in 0..total_palettes {
            let mut palette = Vec::with_capacity(colours_per_row);
            for colour_idx in 0..colours_per_row {
                if pos + 4 > content_len {
                    break;
                }
                // File byte order: R, B, G, padding
                let r = content[pos];
                let b = content[pos + 1];
                let g = content[pos + 2];
                let a = if colour_idx == 0 { 0 } else { 255 };
                palette.push((r, b, g, a));
                pos += 4;
            }
            palettes.push(palette);
        }

        if palettes.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SMA contains no palette data",
            ));
        }
        palettes
    };

    // Image data: expand each byte to two nibbles (low first, high second)
    let img_data = {
        let byte_count = (ptr_palette_data as usize).saturating_sub(ptr_img_data as usize);
        let mut nibbles = Vec::with_capacity(byte_count * 2);
        for &byte in &content[ptr_img_data as usize..ptr_img_data as usize + byte_count] {
            nibbles.push(byte & 0x0F);
            nibbles.push(byte >> 4);
        }
        nibbles
    };

    // Animation entries: 12 bytes each
    let anim_data = {
        cursor.seek(SeekFrom::Start(ptr_anim_data as u64))?;
        let mut anims = Vec::with_capacity(num_animations as usize);
        for _ in 0..num_animations {
            let block_width = read_u8(&mut cursor)?;
            let block_height = read_u8(&mut cursor)?;
            let _unk5 = read_u16_le(&mut cursor)?;
            let byte_offset = read_u16_le(&mut cursor)?;
            let _padding = read_u16_le(&mut cursor)?;
            let frame_count = read_u16_le(&mut cursor)?;
            let _unk6 = read_u16_le(&mut cursor)?;
            anims.push(SmaAnimation {
                block_width,
                block_height,
                byte_offset,
                frame_count,
            });
        }
        anims
    };

    Ok(SmaFile {
        anim_data,
        img_data,
        custom_palette: palette,
    })
}

fn render_status_frame(
    sma: &SmaFile,
    anim_idx: usize,
    palette_idx: usize,
    frame_idx: usize,
) -> Option<RgbaImage> {
    let anim = sma.anim_data.get(anim_idx)?;
    if anim.block_width == 0 || anim.block_height == 0 {
        return None;
    }

    let palette = sma.custom_palette.get(palette_idx)?;
    let width = anim.block_width as usize;
    let height = anim.block_height as usize;
    let pixels_per_frame = TEX_SIZE * TEX_SIZE * width * height;
    let start = anim.byte_offset as usize * 2 + frame_idx * pixels_per_frame;

    if start + pixels_per_frame > sma.img_data.len() {
        return None;
    }

    let px_w = (width * TEX_SIZE) as u32;
    let px_h = (height * TEX_SIZE) as u32;
    let mut img = RgbaImage::new(px_w, px_h);

    for by in 0..height {
        for bx in 0..width {
            let block_index = by * width + bx;
            let tex_pos = block_index * TEX_SIZE * TEX_SIZE;

            for py in 0..TEX_SIZE {
                for px in 0..TEX_SIZE {
                    let pal_element = sma.img_data[start + tex_pos + py * TEX_SIZE + px] as usize;
                    let colour = if pal_element == 0 || pal_element >= palette.len() {
                        Rgba([0, 0, 0, 0])
                    } else {
                        let (r, g, b, a) = palette[pal_element];
                        Rgba([r, g, b, a])
                    };
                    img.put_pixel(
                        (bx * TEX_SIZE + px) as u32,
                        (by * TEX_SIZE + py) as u32,
                        colour,
                    );
                }
            }
        }
    }

    Some(img)
}

fn render_status_sheet(
    sma: &SmaFile,
    anim_idx: usize,
    palette_idx: usize,
) -> Option<(RgbaImage, u32, u32)> {
    let anim = sma.anim_data.get(anim_idx)?;
    if anim.block_width == 0 || anim.frame_count == 0 {
        return None;
    }

    let frame_w = anim.block_width as u32 * TEX_SIZE as u32;
    let frame_h = anim.block_height as u32 * TEX_SIZE as u32;

    let mut frames = Vec::with_capacity(anim.frame_count as usize);
    for f in 0..anim.frame_count as usize {
        match render_status_frame(sma, anim_idx, palette_idx, f) {
            Some(frame) => frames.push(frame),
            None => frames.push(RgbaImage::new(frame_w, frame_h)),
        }
    }

    if frames.is_empty() {
        return None;
    }

    let sheet_w = frame_w * frames.len() as u32;
    let mut sheet = RgbaImage::new(sheet_w, frame_h);
    for (i, frame) in frames.iter().enumerate() {
        image::imageops::overlay(&mut sheet, frame, (i as u32 * frame_w) as i64, 0);
    }

    Some((sheet, frame_w, frame_h))
}

/// RAM address of the icon bit → SMA animation/palette lookup table in overlay 29 (US ROM).
const OV29_TABLE_ADDR: u32 = 0x02350f8c;
const TABLE_ENTRY_COUNT: usize = 34;

const BIT_FLAGS: [(u8, &str); 32] = [
    (0, "sleepless"),
    (1, "burn"),
    (2, "poison"),
    (3, "toxic"),
    (4, "confusion"),
    (5, "cowering"),
    (6, "taunt"),
    (7, "encore"),
    (8, "reflect"),
    (9, "safeguard"),
    (10, "light_screen"),
    (11, "protect"),
    (12, "endure"),
    (13, "low_hp"),
    (14, "curse"),
    (15, "embargo"),
    (16, "sure_shot"),
    (17, "whiffer"),
    (18, "set_damage"),
    (19, "focus_energy"),
    (20, "blinded"),
    (21, "cross_eyed"),
    (22, "eyedrops"),
    (23, "muzzled"),
    (24, "grudge"),
    (25, "exposed"),
    (26, "sleep"),
    (27, "lowered_stat"),
    (28, "heal_block"),
    (29, "miracle_eye"),
    (30, "red_exclamation"),
    (31, "magnet_rise"),
];

#[derive(Serialize)]
struct StatusIconEntry {
    frames: u16,
    frame_width: u32,
    frame_height: u32,
    sma_anim: u32,
    palette: u32,
    #[serde(rename = "type")]
    icon_type: String,
}

pub struct StatusIconExtractor<'a> {
    rom: &'a mut Rom,
}

impl<'a> StatusIconExtractor<'a> {
    pub fn new(rom: &'a mut Rom) -> Self {
        StatusIconExtractor { rom }
    }

    pub fn extract(
        &mut self,
        output_dir: &Path,
        reporter: &mut ProgressReporter,
    ) -> io::Result<()> {
        fs::create_dir_all(output_dir)?;

        if !self.rom.loaded_overlays.contains_key(&29) {
            self.rom.load_arm9_overlays(&[29])?;
        }

        let sma_file_id = self
            .rom
            .fnt
            .get_file_id("SYSTEM/manpu_su.sma")
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "SYSTEM/manpu_su.sma not found")
            })?;

        let sma_raw = self
            .rom
            .fat
            .get_file_data(sma_file_id as usize, &self.rom.data)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Failed to extract manpu_su.sma")
            })?;

        let sma = parse_sma(sma_raw)?;
        println!(
            "Parsed manpu_su.sma: {} animations, {} palettes",
            sma.anim_data.len(),
            sma.custom_palette.len()
        );

        let rom_table = self.read_ov29_table()?;

        let mut metadata = serde_json::Map::new();
        let total_icons = BIT_FLAGS.len() + 1;
        reporter.begin(PhaseId::StatusIcons, total_icons);

        for &(bit, flag_name) in BIT_FLAGS.iter() {
            let table_idx = bit as usize + 1;
            let (anim_idx, pal_idx) = rom_table[table_idx];

            match render_and_save(&sma, anim_idx, pal_idx, flag_name, "cycling", output_dir) {
                Ok(Some(entry)) => {
                    metadata.insert(flag_name.to_string(), serde_json::to_value(&entry).unwrap());
                }
                Ok(None) => {
                    println!("  SKIP {}: null animation", flag_name);
                }
                Err(e) => {
                    eprintln!("  Error rendering {}: {}", flag_name, e);
                }
            }

            reporter.advance();
        }

        // Persistent freeze icon: table index 33
        let (freeze_anim, freeze_pal) = rom_table[33];
        match render_and_save(
            &sma,
            freeze_anim,
            freeze_pal,
            "freeze",
            "persistent",
            output_dir,
        ) {
            Ok(Some(entry)) => {
                metadata.insert("freeze".to_string(), serde_json::to_value(&entry).unwrap());
            }
            Ok(None) => {
                println!("  SKIP freeze: null animation");
            }
            Err(e) => {
                eprintln!("  Error rendering freeze: {}", e);
            }
        }

        reporter.advance();

        let json_path = output_dir.join("status_icons.json");
        crate::json_out::write(&json_path, &metadata)?;
        println!("Saved status icon metadata to {}", json_path.display());

        Ok(())
    }

    fn read_ov29_table(&self) -> io::Result<Vec<(u32, u32)>> {
        let overlay = self
            .rom
            .loaded_overlays
            .get(&29)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Overlay 29 not loaded"))?;

        let table_offset = (OV29_TABLE_ADDR - overlay.ram_address) as usize;

        if table_offset + TABLE_ENTRY_COUNT * 8 > overlay.data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Overlay 29 table out of bounds: offset={:#x}, needed={}, len={}",
                    table_offset,
                    TABLE_ENTRY_COUNT * 8,
                    overlay.data.len()
                ),
            ));
        }

        let mut table = Vec::with_capacity(TABLE_ENTRY_COUNT);
        for i in 0..TABLE_ENTRY_COUNT {
            let off = table_offset + i * 8;
            let anim_idx = u32::from_le_bytes(overlay.data[off..off + 4].try_into().unwrap());
            let pal_idx = u32::from_le_bytes(overlay.data[off + 4..off + 8].try_into().unwrap());
            table.push((anim_idx, pal_idx));
        }

        Ok(table)
    }
}

fn render_and_save(
    sma: &SmaFile,
    anim_idx: u32,
    pal_idx: u32,
    flag_name: &str,
    icon_type: &str,
    output_dir: &Path,
) -> io::Result<Option<StatusIconEntry>> {
    let anim = match sma.anim_data.get(anim_idx as usize) {
        Some(a) if a.block_width > 0 && a.frame_count > 0 => a,
        _ => return Ok(None),
    };

    let (sheet, frame_width, frame_height) =
        match render_status_sheet(sma, anim_idx as usize, pal_idx as usize) {
            Some(result) => result,
            None => return Ok(None),
        };

    let filename = format!("{}.png", flag_name);
    sheet
        .save(output_dir.join(&filename))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    println!(
        "  -> {}.png ({}x{}px, {} frames, {})",
        flag_name, frame_width, frame_height, anim.frame_count, icon_type
    );

    Ok(Some(StatusIconEntry {
        frames: anim.frame_count,
        frame_width,
        frame_height,
        sma_anim: anim_idx,
        palette: pal_idx,
        icon_type: icon_type.to_string(),
    }))
}
