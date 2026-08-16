//! # ImgItm - paletted icon container
//!
//! Used by `dungeon.bin` for `items.itm.img` (index 1022) and `traps.trp.img` (index 1033). SIR0-wrapped.
//! The header sits at the SIR0 data pointer and is four little-endian u32s:
//!
//! | Offset | Meaning                                  |
//! |--------|------------------------------------------|
//! | 0x00   | pointer to sprite data                   |
//! | 0x04   | sprite count                             |
//! | 0x08   | pointer to palette data                  |
//! | 0x0C   | total colour count (divide by 16)        |
//!
//! Each sprite is a square `chunk_dim × chunk_dim` grid of 8x8 4bpp tiles, laid out row-major.
//! Colour index 0 is transparent.

use std::io;

use image::{Rgba, RgbaImage};

pub const TILE_DIM: u32 = 8;
pub const TILE_BYTES: usize = 32; // 8x8 at 4bpp
pub const PAL_LEN: usize = 16;
pub const PAL_ENTRY_BYTES: usize = 4; // RGBX

/// Sanity bound on the derived chunk dimension. Items are 2, traps are 3.
const MAX_CHUNK_DIM: usize = 4;

pub struct ImgItm {
    /// All tiles, flattened. Sprite `n` owns
    /// `tiles[n * tiles_per_sprite() .. (n + 1) * tiles_per_sprite()]`.
    pub tiles: Vec<[u8; TILE_BYTES]>,
    pub palettes: Vec<[Rgba<u8>; PAL_LEN]>,
    pub sprite_count: usize,
    /// Tiles per side of one sprite.
    pub chunk_dim: usize,
    /// Rendered cell size in pixels: `TILE_DIM * chunk_dim`.
    pub cell: u32,
}

impl ImgItm {
    pub fn tiles_per_sprite(&self) -> usize {
        self.chunk_dim * self.chunk_dim
    }

    /// Parses the unwrapped SIR0 content.
    pub fn from_sir0(content: &[u8], data_pointer: u32) -> io::Result<Self> {
        let hdr = data_pointer as usize;
        if hdr + 16 > content.len() {
            return Err(invalid(format!(
                "ImgItm header at 0x{:X} exceeds content length {}",
                hdr,
                content.len()
            )));
        }

        let read_u32 = |off: usize| -> u32 {
            u32::from_le_bytes([
                content[off],
                content[off + 1],
                content[off + 2],
                content[off + 3],
            ])
        };

        let spr_ptr = read_u32(hdr) as usize;
        let sprite_count = read_u32(hdr + 0x04) as usize;
        let pal_ptr = read_u32(hdr + 0x08) as usize;
        let colour_count = read_u32(hdr + 0x0C) as usize;

        if sprite_count == 0 {
            return Err(invalid("ImgItm declares zero sprites"));
        }
        if pal_ptr <= spr_ptr {
            return Err(invalid(format!(
                "ImgItm palette pointer 0x{:X} does not follow sprite pointer 0x{:X}",
                pal_ptr, spr_ptr
            )));
        }

        // The writer emits sprites, then palettes, then the header, so the gap
        // between the two pointers is exactly the sprite block.
        let sprite_block = pal_ptr - spr_ptr;
        let sprite_bytes = sprite_block / sprite_count;

        if sprite_block % sprite_count != 0 || sprite_bytes % TILE_BYTES != 0 {
            return Err(invalid(format!(
                "ImgItm sprite block {} does not divide evenly into {} sprites of whole tiles",
                sprite_block, sprite_count
            )));
        }

        let tiles_per_sprite = sprite_bytes / TILE_BYTES;
        let chunk_dim = isqrt_exact(tiles_per_sprite).ok_or_else(|| {
            invalid(format!(
                "ImgItm sprite of {} tiles is not square",
                tiles_per_sprite
            ))
        })?;

        if chunk_dim == 0 || chunk_dim > MAX_CHUNK_DIM {
            return Err(invalid(format!(
                "ImgItm chunk dimension {} outside supported range 1..={}",
                chunk_dim, MAX_CHUNK_DIM
            )));
        }

        let tiles = read_tiles(content, spr_ptr, sprite_count * tiles_per_sprite)?;
        let palettes = read_palettes(content, pal_ptr, colour_count / PAL_LEN)?;

        Ok(ImgItm {
            tiles,
            palettes,
            sprite_count,
            chunk_dim,
            cell: TILE_DIM * chunk_dim as u32,
        })
    }

    /// Renders one sprite under one palette. Out-of-range indices yield a fully transparent cell.
    pub fn render(&self, sprite: usize, palette: usize) -> RgbaImage {
        let mut img = RgbaImage::new(self.cell, self.cell);

        let Some(pal) = self.palettes.get(palette) else {
            return img;
        };
        if sprite >= self.sprite_count {
            return img;
        }

        let base = sprite * self.tiles_per_sprite();

        for local in 0..self.tiles_per_sprite() {
            let tile = &self.tiles[base + local];
            let ox = (local % self.chunk_dim) as u32 * TILE_DIM;
            let oy = (local / self.chunk_dim) as u32 * TILE_DIM;

            for (byte_idx, &byte) in tile.iter().enumerate() {
                let px = ((byte_idx % 4) * 2) as u32;
                let py = (byte_idx / 4) as u32;

                let lo = (byte & 0x0F) as usize;
                let hi = ((byte >> 4) & 0x0F) as usize;

                // Colour index 0 is transparent.
                if lo != 0 {
                    img.put_pixel(ox + px, oy + py, pal[lo]);
                }
                if hi != 0 {
                    img.put_pixel(ox + px + 1, oy + py, pal[hi]);
                }
            }
        }

        img
    }

    /// Every `(sprite, palette)` pair rendered.
    pub fn render_all(&self) -> Vec<RgbaImage> {
        let mut out = Vec::with_capacity(self.sprite_count * self.palettes.len());
        for sprite in 0..self.sprite_count {
            for palette in 0..self.palettes.len() {
                out.push(self.render(sprite, palette));
            }
        }
        out
    }
}

fn read_tiles(content: &[u8], ptr: usize, count: usize) -> io::Result<Vec<[u8; TILE_BYTES]>> {
    let end = ptr + count * TILE_BYTES;
    if end > content.len() {
        return Err(invalid(format!(
            "ImgItm tile data 0x{:X}..0x{:X} exceeds content length {}",
            ptr,
            end,
            content.len()
        )));
    }

    Ok(content[ptr..end]
        .chunks_exact(TILE_BYTES)
        .map(|c| {
            let mut tile = [0u8; TILE_BYTES];
            tile.copy_from_slice(c);
            tile
        })
        .collect())
}

/// Palettes are RGBX, four bytes per colour. Index 0 is transparent.
fn read_palettes(content: &[u8], ptr: usize, count: usize) -> io::Result<Vec<[Rgba<u8>; PAL_LEN]>> {
    if count == 0 {
        return Err(invalid("ImgItm declares zero palettes"));
    }

    let end = ptr + count * PAL_LEN * PAL_ENTRY_BYTES;
    if end > content.len() {
        return Err(invalid(format!(
            "ImgItm palette data 0x{:X}..0x{:X} exceeds content length {}",
            ptr,
            end,
            content.len()
        )));
    }

    Ok(content[ptr..end]
        .chunks_exact(PAL_LEN * PAL_ENTRY_BYTES)
        .map(|pal_bytes| {
            let mut pal = [Rgba([0, 0, 0, 0]); PAL_LEN];
            for (i, colour) in pal_bytes.chunks_exact(PAL_ENTRY_BYTES).enumerate() {
                pal[i] = Rgba([colour[0], colour[1], colour[2], 255]);
            }
            pal
        })
        .collect())
}

/// Integer square root, but only when `n` is a perfect square.
fn isqrt_exact(n: usize) -> Option<usize> {
    let root = (n as f64).sqrt().round() as usize;
    (root * root == n).then_some(root)
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}
