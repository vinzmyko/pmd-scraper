//! Move data extraction from `BALANCE/waza_p.bin`, joined against move names from the `text_*.str` string table.
//!
//! Block offsets are region-dependent and live in `RegionData`. The block length is region-invariant
//! and lives in `data::text_strings`.

use std::{
    collections::BTreeMap,
    io::{self, Cursor},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    binary_utils::{read_u16_le, read_u32_le, read_u8},
    containers::sir0::Sir0,
    data::text_strings,
    rom::Rom,
    text_utils::to_snake_case,
};

fn move_type_str(value: u8) -> String {
    match value {
        0 => "None",
        1 => "Normal",
        2 => "Fire",
        3 => "Water",
        4 => "Grass",
        5 => "Electric",
        6 => "Ice",
        7 => "Fighting",
        8 => "Poison",
        9 => "Ground",
        10 => "Flying",
        11 => "Psychic",
        12 => "Bug",
        13 => "Rock",
        14 => "Ghost",
        15 => "Dragon",
        16 => "Dark",
        17 => "Steel",
        18 => "Neutral",
        _ => "Unknown",
    }
    .to_string()
}

fn move_category_str(value: u8) -> String {
    match value {
        0 => "Physical",
        1 => "Special",
        2 => "Status",
        3 => "None",
        _ => "Unknown",
    }
    .to_string()
}

fn move_target_str(value: u8) -> String {
    match value {
        0 => "Enemies",
        1 => "Party (including user)",
        2 => "All (including user)",
        3 => "User",
        4 => "Enemies (after charging)",
        5 => "All (except user)",
        6 => "Teammates (excluding user)",
        15 => "Special (unique targeting, e.g. Spikes, Curse)",
        _ => "Unknown",
    }
    .to_string()
}

fn move_range_str(value: u8) -> String {
    match value {
        0 => "Front (1 tile)",
        1 => "Front_and_Sides (cuts corners)",
        2 => "Nearby (8 surrounding tiles)",
        3 => "Room",
        4 => "Front_2 (2 tiles, cuts corners, AI unaware)",
        5 => "Front_10 (10 tiles)",
        6 => "Floor",
        7 => "User (self-target or front after charging)",
        8 => "Front (cuts corners)",
        9 => "Front_2 (2 tiles, cuts corners, AI aware)",
        15 => "Special (unique range, move-specific behavior)",
        _ => "Unknown",
    }
    .to_string()
}

fn ai_condition_str(value: u8) -> String {
    match value {
        0 => "Always eligible",
        1 => "Random chance (uses ai_random_use_chance %)",
        2 => "Target HP <= 25%",
        3 => "Target has negative status",
        4 => "Target is asleep/napping/nightmare",
        5 => "Target HP <= 25% or has negative status",
        6 => "Target is Ghost type and not exposed",
        _ => "Unknown",
    }
    .to_string()
}

/// Maps the AI range nibble to GetMoveRangeDistance's return value.
///
/// Only three AI range values produce a nonzero distance, everything else returns 0.
/// Used by FUN_023230fc as param_3 to drive projectile loop count and amplitude.
///
/// Range 4 ("Front_2, AI unaware") returns 0, NOT 2. Only range 9
/// ("Front_2, AI aware") drives 2-tile projectile travel.
fn move_range_distance(ai_range_val: u8) -> i32 {
    match ai_range_val {
        5 => 10, // Front_10 (10 tiles)
        8 => 1,  // Front (cuts corners)
        9 => 2,  // Front_2 (2 tiles, AI aware)
        _ => 0,  // Includes range 4 (Front_2 AI unaware) — confirmed via Ghidra
    }
}

/// Unpack a 16-bit target_range field into (target, range, ai_condition)
fn unpack_target_range(packed: u16) -> (u8, u8, u8) {
    let target = (packed & 0x0F) as u8;
    let range = ((packed >> 4) & 0x0F) as u8;
    let ai_condition = ((packed >> 8) & 0x0F) as u8;
    (target, range, ai_condition)
}

/// Represents a single move entry from waza_p.bin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveData {
    pub move_id: u16,
    pub name: String,
    pub base_power: u16,
    pub move_type: String,
    pub category: String,
    pub target: String,
    pub range: String,

    /// AI-use targeting (waza_p.bin offset 0x06). Also the effective release
    /// range for two-turn moves and the source GetMoveRangeDistance() reads for
    /// projectile distance regardless of caster. For two-turn moves the player
    /// field (0x04) is only the charge-turn placeholder.
    pub ai_target: String,
    pub ai_range: String,
    pub ai_use_condition: String,
    pub ai_random_use_chance: u8,
    pub ai_weight: u8,
    pub ai_can_use_against_frozen: bool,

    pub move_range_distance: i32,

    pub pp: u8,
    pub accuracy1: u8,
    pub accuracy2: u8,
    pub strikes: u8,
    pub crit_chance: u8,
    pub max_ginseng_boost: u8,
    // Interaction flags
    pub reflected_by_magic_coat: bool,
    pub can_be_snatched: bool,
    pub fails_while_muzzled: bool,
    pub usable_while_taunted: bool,
    pub message_string_idx: u16,
}

/// Main extractor struct
pub struct MoveDataExtractor<'a> {
    rom: &'a Rom,
}

impl<'a> MoveDataExtractor<'a> {
    pub fn new(rom: &'a Rom) -> Self {
        MoveDataExtractor { rom }
    }

    /// Extract move data and save to JSON files
    pub fn extract_and_save(&self, output_dir: &Path) -> io::Result<()> {
        println!("Starting move data extraction...");

        println!("  Loading text_e.str for move names...");
        let move_names = self.load_move_names()?;
        println!("  Loaded {} move names", move_names.len());

        let waza_data = self.load_waza_p_bin()?;

        let sir0_data = Sir0::from_bytes(&waza_data)?;

        let moves = self.parse_move_data(&sir0_data, &move_names)?;

        println!("  Extracted {} moves", moves.len());

        self.save_move_lookup(&moves, output_dir)?;
        self.save_move_data(&moves, output_dir)?;

        println!("Move data extraction complete!");
        Ok(())
    }

    /// Load move names from the ROM's string table.
    fn load_move_names(&self) -> io::Result<Vec<String>> {
        let begin = self.rom.region_data.move_names_begin as usize;
        if begin == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Move name string block offset is not known for this region",
            ));
        }

        let text_data = text_strings::load_text_file(self.rom)?;
        println!("  Found text file: {} bytes", text_data.len());

        let strings = text_strings::parse_string_table(text_data)?;
        println!("  Parsed {} total strings from text file", strings.len());

        Ok(text_strings::block(&strings, begin, text_strings::MOVE_NAME_COUNT)?.to_vec())
    }

    /// Load waza_p.bin from ROM
    fn load_waza_p_bin(&self) -> io::Result<Vec<u8>> {
        let file_id = self
            .rom
            .fnt
            .get_file_id("BALANCE/waza_p.bin")
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "waza_p.bin not found in ROM")
            })?;

        self.rom
            .fat
            .get_file_data(file_id as usize, &self.rom.data)
            .map(|data| data.to_vec())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Failed to extract waza_p.bin data",
                )
            })
    }

    /// Parse move data from SIR0 content
    fn parse_move_data(&self, sir0: &Sir0, move_names: &[String]) -> io::Result<Vec<MoveData>> {
        let mut cursor = Cursor::new(&sir0.content as &[u8]);

        cursor.set_position(sir0.data_pointer as u64);

        let ptr_moves_data = read_u32_le(&mut cursor)?;
        let _ptr_moveset_table = read_u32_le(&mut cursor)?;

        println!("  Moves data pointer: 0x{:X}", ptr_moves_data);

        cursor.set_position(ptr_moves_data as u64);

        let mut moves = Vec::new();
        let mut move_index = 0;

        loop {
            let current_pos = cursor.position() as usize;

            if current_pos + 26 > sir0.content.len() {
                break;
            }

            if sir0.content[current_pos] == 0xAA {
                println!("  Found padding marker at offset 0x{:X}", current_pos);
                break;
            }

            let name = move_names
                .get(move_index)
                .cloned()
                .unwrap_or_else(|| format!("Unknown_{:04}", move_index));

            let move_data = self.parse_move_entry(&mut cursor, name)?;
            moves.push(move_data);

            move_index += 1;
        }

        Ok(moves)
    }

    /// Parse a single 26-byte move entry
    fn parse_move_entry(&self, cursor: &mut Cursor<&[u8]>, name: String) -> io::Result<MoveData> {
        let base_power = read_u16_le(cursor)?; // 0x00
        let raw_type = read_u8(cursor)?; // 0x02
        let raw_category = read_u8(cursor)?; // 0x03
        let raw_target_range = read_u16_le(cursor)?; // 0x04
        let raw_ai_target_range = read_u16_le(cursor)?; // 0x06
        let pp = read_u8(cursor)?; // 0x08
        let ai_weight = read_u8(cursor)?; // 0x09
        let accuracy1 = read_u8(cursor)?; // 0x0A
        let accuracy2 = read_u8(cursor)?; // 0x0B
        let ai_random_use_chance = read_u8(cursor)?; // 0x0C
        let strikes = read_u8(cursor)?; // 0x0D
        let max_ginseng_boost = read_u8(cursor)?; // 0x0E
        let crit_chance = read_u8(cursor)?; // 0x0F
        let reflected_by_magic_coat = read_u8(cursor)? != 0; // 0x10
        let can_be_snatched = read_u8(cursor)? != 0; // 0x11
        let fails_while_muzzled = read_u8(cursor)? != 0; // 0x12
        let ai_can_use_against_frozen = read_u8(cursor)? != 0; // 0x13
        let usable_while_taunted = read_u8(cursor)? != 0; // 0x14
        let _range_string_idx = read_u8(cursor)?; // 0x15
        let move_id = read_u16_le(cursor)?; // 0x16
        let message_string_idx = read_u16_le(cursor)?; // 0x18

        let (target_val, range_val, _ai_cond_val) = unpack_target_range(raw_target_range);
        let (ai_target_val, ai_range_val, ai_cond_val) = unpack_target_range(raw_ai_target_range);

        Ok(MoveData {
            move_id,
            name,
            base_power,
            move_type: move_type_str(raw_type),
            category: move_category_str(raw_category),
            target: move_target_str(target_val),
            range: move_range_str(range_val),
            ai_target: move_target_str(ai_target_val),
            ai_range: move_range_str(ai_range_val),
            ai_use_condition: ai_condition_str(ai_cond_val),
            ai_random_use_chance,
            ai_weight,
            ai_can_use_against_frozen,
            move_range_distance: move_range_distance(ai_range_val),
            pp,
            accuracy1,
            accuracy2,
            strikes,
            crit_chance,
            max_ginseng_boost,
            reflected_by_magic_coat,
            can_be_snatched,
            fails_while_muzzled,
            usable_while_taunted,
            message_string_idx,
        })
    }

    fn save_move_lookup(&self, moves: &[MoveData], output_dir: &Path) -> io::Result<()> {
        let lookup: BTreeMap<String, u16> = moves
            .iter()
            .enumerate()
            .map(|(idx, m)| (to_snake_case(&m.name), idx as u16))
            .collect();

        let output_path = output_dir.join("move_lookup.json");
        crate::json_out::write(&output_path, &lookup)?;
        println!("  Saved move lookup to {}", output_path.display());
        Ok(())
    }

    /// Save full move data JSON
    fn save_move_data(&self, moves: &[MoveData], output_dir: &Path) -> io::Result<()> {
        let output_path = output_dir.join("move_data.json");
        let move_map: BTreeMap<u16, &MoveData> = moves.iter().map(|m| (m.move_id, m)).collect();

        crate::json_out::write(&output_path, &move_map)?;
        println!("  Saved move data to {}", output_path.display());
        Ok(())
    }
}
