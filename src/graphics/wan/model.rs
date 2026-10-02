//! Data structures for WAN sprite files
//!
//! This module defines the core data structures used to represent
//! WAN sprite data

use std::collections::{BTreeSet, HashMap};

use super::{flags, WanType, DIM_TABLE, TEX_SIZE};

pub type RgbaTuple = (u8, u8, u8, u8);
pub type Palette = Vec<RgbaTuple>;
pub type PaletteList = Vec<Palette>;
pub type TileLookup = HashMap<usize, usize>;

#[derive(Debug, Clone)]
pub enum AnimationStructure {
    Character(Vec<Vec<Animation>>), // [animation_type][direction]
    Effect(Vec<Vec<Animation>>),    // [group][sequence] - but ROM only uses group 0
}

// TODO: Maybe create a effect and character inside this and CharaWan and EffectWan for better
// separation instead of one structure.
#[derive(Debug, Clone)]
pub struct WanFile {
    pub img_data: Vec<ImgPiece>,
    pub frame_data: Vec<MetaFrame>,
    pub animations: AnimationStructure,
    pub body_part_offset_data: Vec<FrameOffset>,
    pub custom_palette: PaletteList,
    pub effect_specific_palette: Option<PaletteList>,
    pub tile_lookup_8bpp: Option<TileLookup>,
    pub wan_type: WanType,
    pub palette_offset: u16,
    pub max_sequences_per_group: u16,
    pub offset_table_size: usize,
}

impl WanFile {
    /// Distinct `draw_order_offset`s used by the given sequences of an effect WAN,
    /// sorted ascending. Empty for character WANs or missing sequences.
    pub fn effect_draw_order_offsets(
        &self,
        sequences: impl IntoIterator<Item = usize>,
    ) -> BTreeSet<i8> {
        let AnimationStructure::Effect(groups) = &self.animations else {
            return BTreeSet::new();
        };
        let Some(group) = groups.first() else {
            return BTreeSet::new();
        };
        sequences
            .into_iter()
            .filter_map(|seq| group.get(seq))
            .flat_map(|anim| &anim.frames)
            .filter_map(|frame| self.frame_data.get(frame.frame_index as usize))
            .flat_map(|meta| &meta.pieces)
            .map(MetaFramePiece::draw_order_offset)
            .collect()
    }
}

/// A collection of image data strips
#[derive(Debug, Clone)]
pub struct ImgPiece {
    pub img_px: Vec<u8>,
}

/// A collection of meta frame pieces that form a complete sprite frame
#[derive(Debug, Clone)]
pub struct MetaFrame {
    /// Individual pieces of the frame
    pub pieces: Vec<MetaFramePiece>,
}

#[derive(Debug, Clone)]
pub struct MetaFramePiece {
    pub tile_num: u16,
    pub palette_index: u8,
    pub h_flip: bool,
    pub v_flip: bool,
    pub x_offset: i16,
    pub y_offset: i16,
    pub resolution_idx: usize,
    pub is_256_colour: bool,

    /// Raw 5 x u16 piece words as stored in the WAN.
    pub raw: [u16; 5],
}

#[derive(Debug, Clone, Copy)]
pub struct MetaFramePieceArgs {
    pub tile_num: u16,
    pub palette_index: u8,
    pub h_flip: bool,
    pub v_flip: bool,
    pub x_offset: i16,
    pub y_offset: i16,
    pub resolution_idx: usize,
    pub is_256_colour: bool,

    /// Raw 5 x u16 piece words as stored in the WAN.
    pub raw: [u16; 5],
}

impl MetaFramePiece {
    pub fn new(args: MetaFramePieceArgs) -> Self {
        Self {
            tile_num: args.tile_num,
            palette_index: args.palette_index,
            h_flip: args.h_flip,
            v_flip: args.v_flip,
            x_offset: args.x_offset,
            y_offset: args.y_offset,
            resolution_idx: args.resolution_idx,
            is_256_colour: args.is_256_colour,
            raw: args.raw,
        }
    }

    /// Signed offset added to the sprite's draw order to pick this piece's bucket
    /// (ROM `FUN_0201b6d4`). High byte of `piece[1]`.
    pub fn draw_order_offset(&self) -> i8 {
        (self.raw[1] >> 8) as u8 as i8
    }

    pub fn get_dimensions(&self) -> (usize, usize) {
        DIM_TABLE
            .get(self.resolution_idx)
            .copied()
            .unwrap_or((1, 1))
    }

    pub fn get_bounds(&self) -> (i16, i16, i16, i16) {
        let start_x = self.x_offset;
        let start_y = self.y_offset;
        let (width_blocks, height_blocks) = self.get_dimensions();

        (
            start_x,
            start_y,
            start_x + (width_blocks * TEX_SIZE) as i16,
            start_y + (height_blocks * TEX_SIZE) as i16,
        )
    }
}

/// Body part offset data for a frame
#[derive(Debug, Clone)]
pub struct FrameOffset {
    /// Head position (x, y)
    pub head: (i16, i16),
    /// Left hand position (x, y)
    pub lhand: (i16, i16),
    /// Right hand position (x, y)
    pub rhand: (i16, i16),
    // Centre position (x, y)
    pub centre: (i16, i16),
}

impl FrameOffset {
    pub fn new(head: (i16, i16), lhand: (i16, i16), rhand: (i16, i16), centre: (i16, i16)) -> Self {
        Self {
            head,
            lhand,
            rhand,
            centre,
        }
    }
}

/// A frame in an animation sequence
#[derive(Debug, Clone)]
pub struct SequenceFrame {
    pub frame_index: u16,
    /// in 1/60ths of a second
    pub duration: u16,
    /// Special flags (bit 0 = return, bit 1 = hit)
    pub flag: u8,
    pub is_rush_point: bool,
    /// Sprite offset from centre (x, y)
    pub offset: (i16, i16),
    /// Shadow offset from centre (x, y)
    pub shadow: (i16, i16),
}

impl SequenceFrame {
    pub fn new(
        frame_index: u16,
        duration: u16,
        flag: u8,
        offset: (i16, i16),
        shadow: (i16, i16),
    ) -> Self {
        Self {
            frame_index,
            duration,
            flag,
            is_rush_point: false,
            offset,
            shadow,
        }
    }

    pub fn is_rush_point(&self) -> bool {
        self.is_rush_point
    }

    pub fn is_effect_point(&self) -> bool {
        (self.flag & flags::FRAME_HIT_MASK) != 0
    }

    pub fn is_return_point(&self) -> bool {
        (self.flag & flags::FRAME_RETURN_MASK) != 0
    }
}

/// An animation sequence
#[derive(Debug, Clone)]
pub struct Animation {
    /// Frames in this animation
    pub frames: Vec<SequenceFrame>,
}

impl Animation {
    pub fn new(frames: Vec<SequenceFrame>) -> Self {
        Self { frames }
    }

    pub fn empty() -> Self {
        Self { frames: Vec::new() }
    }
}
