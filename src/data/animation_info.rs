use std::{fmt, io::Cursor};

use crate::binary_utils::{self};

use serde::{Deserialize, Serialize};

pub const TRAP_DATA_SIZE: usize = 2;
pub const ITEM_DATA_SIZE: usize = 4;
pub const MOVE_DATA_SIZE: usize = 24;
pub const GENERAL_DATA_SIZE: usize = 28;
pub const SPECIAL_MOVE_DATA_SIZE: usize = 6;
pub const HEADER_SIZE: usize = 20; // 5 * 4 bytes

// Sound effect constants
pub const _SFX_SILENCE: u16 = 0x3F00; // 16128 decimal - indicates no sound

// Monster animation type special values
pub const _MONSTER_ANIM_SPIN: u8 = 99; // Rotate through all 8 directions
pub const _MONSTER_ANIM_MULTI_DIR: u8 = 98; // Attack in 9 directions (increment by 2)

/// Animation point type for move animations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimPointType {
    Head = 0,
    LeftHand = 1,
    RightHand = 2,
    Centre = 3,
    None = 255,
}

impl From<u8> for AnimPointType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Head,
            1 => Self::LeftHand,
            2 => Self::RightHand,
            3 => Self::Centre,
            _ => Self::None,
        }
    }
}

impl fmt::Display for AnimPointType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Head => write!(f, "Head"),
            Self::LeftHand => write!(f, "LeftHand"),
            Self::RightHand => write!(f, "RightHand"),
            Self::Centre => write!(f, "Centre"),
            Self::None => write!(f, "None"),
        }
    }
}

/// Animation type for general animations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimType {
    Invalid = 0,
    WanFile0 = 1,
    WanFile1 = 2,
    WanOther = 3,
    Wat = 4,
    Screen = 5,
    Wba = 6,
}

impl From<u32> for AnimType {
    fn from(value: u32) -> Self {
        match value {
            1 => Self::WanFile0,
            2 => Self::WanFile1,
            3 => Self::WanOther,
            4 => Self::Wat,
            5 => Self::Screen,
            6 => Self::Wba,
            _ => Self::Invalid,
        }
    }
}

impl fmt::Display for AnimType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => write!(f, "Invalid"),
            Self::WanFile0 => write!(f, "WanFile0"),
            Self::WanFile1 => write!(f, "WanFile1"),
            Self::WanOther => write!(f, "WanOther"),
            Self::Wat => write!(f, "Wat"),
            Self::Screen => write!(f, "Screen"),
            Self::Wba => write!(f, "Wba"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrapAnimationInfo {
    pub effect_id: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemAnimationInfo {
    pub effect_id_1: u16,
    pub effect_id_2: u16,
}

/// Represents the raw data format for a move animation entry
#[derive(Debug, Clone)]
pub struct RawMoveAnimationInfo {
    // Four effect animation layers - can play up to 4 effects simultaneously
    // No layer is "primary" - game iterates all and plays any non-zero effect
    pub effect_id_1: u16, // Offset 0x0: Effect layer 1
    pub effect_id_2: u16, // Offset 0x2: Effect layer 2
    pub effect_id_3: u16, // Offset 0x4: Effect layer 3
    pub effect_id_4: u16, // Offset 0x6: Effect layer 4

    // Behavior flags (offset 0x8) - packed into single byte
    pub projectile_wave_pattern: u8, // Bits 0-2: Bits 0-2: 0=straight, 1=vertical sine, 2=spiral
    pub dual_target: bool,           // Bit 3: plays effect on both attacker and target
    pub skip_fade_in: bool,          // Bit 4: If true, skip screen fade-in effect
    pub face_direction_with_delay: bool, // Bit 5: resets attacker facing direction + adds pre-animation pause
    pub add_delay: bool,                 // Bit 6: If true, add delay after animation
    pub flag_bit7: bool,                 // Bit 7: Unused

    // Offset 0x9-0xB: Padding (3 bytes) - unused, for 4-byte alignment

    // Animation parameters (offset 0xC onwards)
    pub projectile_speed: u32, // 0=fast(4f), 1=slow(12f), 2=medium(8f), other=fast(4f)
    pub monster_anim_type: u8, // 0-12 (standard), 98 (multi-dir), 99 (spin rotation)
    pub attachment_point_idx: i8, // -1 to 3: position offset lookup index (SIGNED)
    pub sound_effect_id: u16,  // Sound effect ID (0x3F00 = silence)

    // Per-Pokemon animation overrides
    pub special_animation_count: u16,
    pub special_animation_start_index: u16,
}

/// Represents the final move animation entry with embedded special animations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveAnimationInfo {
    // Effect layers - all can be used simultaneously, no "primary" layer
    pub effect_id_1: u16,
    pub effect_id_2: u16,
    pub effect_id_3: u16,
    pub effect_id_4: u16,

    // Flags (offset 0x8)
    pub projectile_wave_pattern: u8, // Bits 0-2: Bits 0-2: 0=straight, 1=vertical sine, 2=spiral
    pub dual_target: bool,           // Bit 3: plays effect on both attacker and target
    pub skip_fade_in: bool,          // Bit 4: Skip screen fade-in effect
    pub face_direction_with_delay: bool, // Bit 5: resets attacker facing direction + adds pre-animation pause
    pub add_delay: bool,                 // Bit 6: Add post-animation delay
    pub flag_bit7: bool,                 // Bit 7: Unknown/unused

    // Animation parameters
    pub projectile_speed: u32, // 0=fast(4f), 1=slow(12f), 2=medium(8f), other=fast(4f)
    pub pokemon_anim_id: u8,   // 0-12=standard, 98=multi-directional, 99=spin
    pub attachment_point_idx: i8, // -1 to 3: position offset lookup index
    pub sound_effect_id: u16,  // 0x3F00 (16128) = silence

    pub special_animations: Vec<SpecialMoveAnimationInfo>,
}

impl MoveAnimationInfo {
    // Create a MoveAnimationInfo from a RawMoveAnimationInfo and list of special animations
    pub fn from_raw(raw: &RawMoveAnimationInfo, specials: Vec<SpecialMoveAnimationInfo>) -> Self {
        Self {
            effect_id_1: raw.effect_id_1,
            effect_id_2: raw.effect_id_2,
            effect_id_3: raw.effect_id_3,
            effect_id_4: raw.effect_id_4,
            projectile_wave_pattern: raw.projectile_wave_pattern,
            dual_target: raw.dual_target,
            skip_fade_in: raw.skip_fade_in,
            face_direction_with_delay: raw.face_direction_with_delay,
            add_delay: raw.add_delay,
            flag_bit7: raw.flag_bit7,
            projectile_speed: raw.projectile_speed,
            pokemon_anim_id: raw.monster_anim_type,
            attachment_point_idx: raw.attachment_point_idx,
            sound_effect_id: raw.sound_effect_id,
            special_animations: specials,
        }
    }

    /// Returns the number of frames for projectile travel based on ROM speed mapping.
    ///
    /// ROM behavior:
    /// - Speed 0: Instant (no projectile animation)
    /// - Speed 1 → maps to 2 → 24/2 = 12 frames (slow)
    /// - Speed 2 → maps to 3 → 24/3 = 8 frames (medium)  
    /// - Other  → maps to 6 → 24/6 = 4 frames (fast)
    ///
    /// At ~60 FPS: 12 frames ≈ 0.2s, 8 frames ≈ 0.13s, 4 frames ≈ 0.067s
    #[allow(dead_code)]
    pub fn projectile_frame_count(&self) -> u8 {
        match self.projectile_speed {
            1 => 12,
            2 => 8,
            _ => 4, // includes 0
        }
    }

    /// Returns projectile travel duration in seconds
    #[allow(dead_code)]
    pub fn projectile_duration_secs(&self) -> f32 {
        self.projectile_frame_count() as f32 / 60.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectAnimationInfo {
    pub anim_type: AnimType,
    pub file_index: u32,
    pub palette_index: u32,
    pub animation_index: u32,
    pub sfx_id: i32,
    pub timing_offset: u32,
    pub screen_effect_param: u8,
    pub attachment_point: i8,
    pub is_non_blocking: bool,
    pub loop_flag: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialMoveAnimationInfo {
    pub pokemon_id: u16,
    pub user_animation_index: u8,
    pub point: AnimPointType,
    pub sfx_id: u16,
}

#[derive(Debug, Clone)]
pub struct AnimData {
    pub trap_table: Vec<TrapAnimationInfo>,
    pub item_table: Vec<ItemAnimationInfo>,
    pub raw_move_table: Vec<RawMoveAnimationInfo>,
    pub effect_table: Vec<EffectAnimationInfo>,
    pub special_move_table: Vec<SpecialMoveAnimationInfo>,
}

impl AnimData {
    // Transform raw move data into final format with embedded special animations
    pub fn transform_move_data(&self) -> std::collections::HashMap<usize, MoveAnimationInfo> {
        let mut move_map = std::collections::HashMap::new();

        for (idx, raw_move) in self.raw_move_table.iter().enumerate() {
            let mut special_animations = Vec::new();

            // If there are special animations, include them
            if raw_move.special_animation_count > 0 {
                let start_idx = raw_move.special_animation_start_index as usize;
                let end_idx = start_idx + raw_move.special_animation_count as usize;

                if end_idx <= self.special_move_table.len() {
                    special_animations = self.special_move_table[start_idx..end_idx].to_vec();
                }
            }

            let move_info = MoveAnimationInfo::from_raw(raw_move, special_animations);
            move_map.insert(idx, move_info);
        }

        move_map
    }
}

/// Region-specific data for animation tables
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionData {
    pub start_table: u32,
    pub check_addr: u32,
    pub move_animation_table_overlay: u8,
    pub move_animation_table_offset: u32,
    pub effect_animation_table_overlay: u8,
    pub effect_animation_table_offset: u32,
    pub effect_animation_entry_size: u32,
    pub tileset_properties_addr: u32,

    /// Index of the first entry of each `text_*.str` block. Zero means
    /// "not yet determined for this region". Callers must reject it.
    pub move_names_begin: u32,
    pub item_names_begin: u32,
    pub item_short_desc_begin: u32,
    pub item_long_desc_begin: u32,
}

pub const NA_REGION_DATA: RegionData = RegionData {
    start_table: 0xAFD0,
    check_addr: 0x3420,
    move_animation_table_overlay: 29,
    move_animation_table_offset: 0x3E064,
    effect_animation_table_overlay: 29,
    effect_animation_table_offset: 0x4152C,
    effect_animation_entry_size: 16,
    tileset_properties_addr: 0x022C631C,
    move_names_begin: 8173,
    item_names_begin: 6773,
    item_short_desc_begin: 12104,
    item_long_desc_begin: 10704,
};

pub const EU_REGION_DATA: RegionData = RegionData {
    start_table: 0xAFE8,
    check_addr: 0x3420,
    move_animation_table_overlay: 29,
    move_animation_table_offset: 0x3E184,
    effect_animation_table_overlay: 29,
    effect_animation_table_offset: 0x41654,
    effect_animation_entry_size: 16,
    tileset_properties_addr: 0, // TODO: unknown for EU
    move_names_begin: 8175,
    item_names_begin: 0,      // TODO: unknown for EU
    item_short_desc_begin: 0, // TODO: unknown for EU
    item_long_desc_begin: 0,  // TODO: unknown for EU
};

pub const JP_REGION_DATA: RegionData = RegionData {
    start_table: 0xAF18,
    check_addr: 0x3424,
    move_animation_table_overlay: 29,
    move_animation_table_offset: 0x3EE94,
    effect_animation_table_overlay: 29,
    effect_animation_table_offset: 0x41354,
    effect_animation_entry_size: 16,
    tileset_properties_addr: 0, // TODO: unknown for JP
    move_names_begin: 4874,
    item_names_begin: 0,      // TODO: unknown for JP
    item_short_desc_begin: 0, // TODO: unknown for JP
    item_long_desc_begin: 0,  // TODO: unknown for JP
};

pub fn get_region_data(game_code: &str) -> Option<RegionData> {
    if game_code.ends_with('E') {
        Some(NA_REGION_DATA) // YFYE, YFTE, C2SE
    } else if game_code.ends_with('P') {
        Some(EU_REGION_DATA) // YFYP, YFTP, C2SP
    } else if game_code.ends_with('J') {
        Some(JP_REGION_DATA) // YFYJ, YFTJ, C2SJ
    } else {
        None
    }
}

/// Parse animation data from binary blob
pub fn parse_animation_data(data: &[u8]) -> Result<AnimData, String> {
    if data.len() < HEADER_SIZE {
        return Err(format!("Data too short: {} bytes", data.len()));
    }

    let mut cursor = Cursor::new(data);

    binary_utils::seek_to(&mut cursor, 0).map_err(|e| e.to_string())?;
    let trap_table_ptr = binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;
    let item_table_ptr = binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;
    let move_table_ptr = binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;
    let general_table_ptr = binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;
    let special_move_table_ptr =
        binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;

    let mut trap_table = Vec::new();
    for offset in (trap_table_ptr as usize..item_table_ptr as usize).step_by(TRAP_DATA_SIZE) {
        binary_utils::seek_to(&mut cursor, offset as u64).map_err(|e| e.to_string())?;

        if offset + TRAP_DATA_SIZE > data.len() {
            break;
        }

        let effect_id = binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;
        trap_table.push(TrapAnimationInfo { effect_id });
    }

    let mut item_table = Vec::new();
    for offset in (item_table_ptr as usize..move_table_ptr as usize).step_by(ITEM_DATA_SIZE) {
        binary_utils::seek_to(&mut cursor, offset as u64).map_err(|e| e.to_string())?;

        if offset + ITEM_DATA_SIZE > data.len() {
            break;
        }

        let anim1 = binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;
        let anim2 = binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;

        item_table.push(ItemAnimationInfo {
            effect_id_1: anim1,
            effect_id_2: anim2,
        });
    }

    let mut raw_move_table = Vec::new();
    for offset in (move_table_ptr as usize..general_table_ptr as usize).step_by(MOVE_DATA_SIZE) {
        binary_utils::seek_to(&mut cursor, offset as u64).map_err(|e| e.to_string())?;

        if offset + MOVE_DATA_SIZE > data.len() {
            break;
        }

        // Read effect IDs (4 layers)
        let effect_id_1 = binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;
        let effect_id_2 = binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;
        let effect_id_3 = binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;
        let effect_id_4 = binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;

        // Read and parse flags byte
        let flags = binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;
        let projectile_wave_pattern = (flags & 0x7) as u8;
        let dual_target = (flags & 0x8) != 0;
        let skip_fade_in = (flags & 0x10) != 0;
        let face_direction_with_delay = (flags & 0x20) != 0;
        let add_delay = (flags & 0x40) != 0;
        let flag_bit7 = (flags & 0x80) != 0;

        // Read animation parameters
        let projectile_speed = binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;
        let monster_anim_type = binary_utils::read_u8(&mut cursor).map_err(|e| e.to_string())?;
        let position_offset_idx = binary_utils::read_i8(&mut cursor).map_err(|e| e.to_string())?;
        let sound_effect_id = binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;
        let special_animation_count =
            binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;
        let special_animation_start_index =
            binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;

        raw_move_table.push(RawMoveAnimationInfo {
            effect_id_1,
            effect_id_2,
            effect_id_3,
            effect_id_4,
            projectile_wave_pattern,
            dual_target,
            skip_fade_in,
            face_direction_with_delay,
            add_delay,
            flag_bit7,
            projectile_speed,
            monster_anim_type,
            attachment_point_idx: position_offset_idx,
            sound_effect_id,
            special_animation_count,
            special_animation_start_index,
        });
    }

    let mut effect_table = Vec::new();
    for offset in
        (general_table_ptr as usize..special_move_table_ptr as usize).step_by(GENERAL_DATA_SIZE)
    {
        binary_utils::seek_to(&mut cursor, offset as u64).map_err(|e| e.to_string())?;

        if offset + GENERAL_DATA_SIZE > data.len() {
            break;
        }

        let anim_type_value = binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;
        let anim_type = AnimType::from(anim_type_value);

        let anim_file = binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;
        let palette_index = binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;
        let animation_index = binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;
        let sfx = binary_utils::read_i32_le(&mut cursor).map_err(|e| e.to_string())?;
        let timing_offset = binary_utils::read_u32_le(&mut cursor).map_err(|e| e.to_string())?;
        let screen_effect_param = binary_utils::read_u8(&mut cursor).map_err(|e| e.to_string())?;

        let point_value = binary_utils::read_i8(&mut cursor).map_err(|e| e.to_string())?;

        let is_non_blocking = binary_utils::read_u8(&mut cursor).map_err(|e| e.to_string())? != 0;
        let loop_flag = binary_utils::read_u8(&mut cursor).map_err(|e| e.to_string())? != 0;

        effect_table.push(EffectAnimationInfo {
            anim_type,
            file_index: anim_file,
            palette_index,
            animation_index,
            sfx_id: sfx,
            timing_offset,
            screen_effect_param,
            attachment_point: point_value,
            is_non_blocking,
            loop_flag,
        });
    }

    let mut special_move_table = Vec::new();
    let data_len = data.len();
    for offset in (special_move_table_ptr as usize..data_len).step_by(SPECIAL_MOVE_DATA_SIZE) {
        binary_utils::seek_to(&mut cursor, offset as u64).map_err(|e| e.to_string())?;

        if offset + SPECIAL_MOVE_DATA_SIZE > data_len {
            break;
        }

        let pkmn_id = binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;
        let animation = binary_utils::read_u8(&mut cursor).map_err(|e| e.to_string())?;

        let point_value = binary_utils::read_u8(&mut cursor).map_err(|e| e.to_string())?;
        let point = AnimPointType::from(point_value);

        let sfx = binary_utils::read_u16_le(&mut cursor).map_err(|e| e.to_string())?;

        special_move_table.push(SpecialMoveAnimationInfo {
            pokemon_id: pkmn_id,
            user_animation_index: animation,
            point,
            sfx_id: sfx,
        });
    }

    Ok(AnimData {
        trap_table,
        item_table,
        raw_move_table,
        effect_table,
        special_move_table,
    })
}
