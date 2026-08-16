//! # Item Data (`BALANCE/item_p.bin`)
//!
//! SIR0-wrapped, 16 bytes per entry, 1400 entries in the US ROM.

use std::io;

pub const ITEM_P_ENTRY_SIZE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemCategory {
    ThrownLine = 0,
    ThrownArc = 1,
    BerriesSeedsVitamins = 2,
    FoodGummies = 3,
    HeldItems = 4,
    TmsHms = 5,
    Poke = 6,
    Unk7 = 7,
    Other = 8,
    Orbs = 9,
    LinkBox = 10,
    UsedTm = 11,
    TreasureBoxes1 = 12,
    TreasureBoxes2 = 13,
    TreasureBoxes3 = 14,
    ExclusiveItems = 15,
    Dummy = 16,
    Unknown = 255,
}

impl From<u8> for ItemCategory {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::ThrownLine,
            1 => Self::ThrownArc,
            2 => Self::BerriesSeedsVitamins,
            3 => Self::FoodGummies,
            4 => Self::HeldItems,
            5 => Self::TmsHms,
            6 => Self::Poke,
            7 => Self::Unk7,
            8 => Self::Other,
            9 => Self::Orbs,
            10 => Self::LinkBox,
            11 => Self::UsedTm,
            12 => Self::TreasureBoxes1,
            13 => Self::TreasureBoxes2,
            14 => Self::TreasureBoxes3,
            15 => Self::ExclusiveItems,
            16 => Self::Dummy,
            _ => Self::Unknown,
        }
    }
}

impl ItemCategory {
    /// Snake-case name for JSON output.
    pub fn as_key(&self) -> &'static str {
        match self {
            Self::ThrownLine => "thrown_line",
            Self::ThrownArc => "thrown_arc",
            Self::BerriesSeedsVitamins => "berries_seeds_vitamins",
            Self::FoodGummies => "food_gummies",
            Self::HeldItems => "held_items",
            Self::TmsHms => "tms_hms",
            Self::Poke => "poke",
            Self::Unk7 => "unk_7",
            Self::Other => "other",
            Self::Orbs => "orbs",
            Self::LinkBox => "link_box",
            Self::UsedTm => "used_tm",
            Self::TreasureBoxes1 => "treasure_boxes_1",
            Self::TreasureBoxes2 => "treasure_boxes_2",
            Self::TreasureBoxes3 => "treasure_boxes_3",
            Self::ExclusiveItems => "exclusive_items",
            Self::Dummy => "dummy",
            Self::Unknown => "unknown",
        }
    }

    /// Stack quantity is only meaningful for thrown items (`IsThrownItem`).
    /// Every other category reads zero.
    pub fn is_thrown(&self) -> bool {
        matches!(self, Self::ThrownLine | Self::ThrownArc)
    }
}

#[derive(Debug, Clone)]
pub struct ItemPEntry {
    pub buy_price: u16,
    pub sell_price: u16,
    pub category: ItemCategory,
    /// Index into `items.itm.img` sprites.
    pub sprite: u8,
    /// Index into `items.itm.img` palettes.
    pub palette: u8,
    /// Set for TMs/HMs/orbs; role unclear for other categories.
    pub move_id: u16,
    /// Minimum stack size on pickup. Thrown categories only.
    pub range_min: u8,
    /// Maximum stack size on pickup. Thrown categories only.
    pub range_max: u8,
    /// Menu verb shown in dungeons (Use / Eat / Ingest / Equip...).
    pub action_name: u8,
    pub is_valid: bool,
    pub is_in_td: bool,
    pub ai_throw_at_enemy: bool,
    pub ai_throw_at_ally: bool,
    pub ai_use_on_self: bool,
}

impl ItemPEntry {
    fn parse(data: &[u8]) -> Self {
        let flags = data[0x0E];
        ItemPEntry {
            buy_price: u16::from_le_bytes([data[0x00], data[0x01]]),
            sell_price: u16::from_le_bytes([data[0x02], data[0x03]]),
            category: ItemCategory::from(data[0x04]),
            sprite: data[0x05],
            // 0x06-0x07 hold the item id, redundant with the array index.
            move_id: u16::from_le_bytes([data[0x08], data[0x09]]),
            range_min: data[0x0A],
            range_max: data[0x0B],
            palette: data[0x0C],
            action_name: data[0x0D],
            is_valid: flags & 0x01 != 0,
            is_in_td: flags & 0x02 != 0,
            ai_throw_at_enemy: flags & 0x20 != 0,
            ai_throw_at_ally: flags & 0x40 != 0,
            ai_use_on_self: flags & 0x80 != 0,
        }
    }
}

/// Parses the unwrapped SIR0 content into one entry per item id.
pub fn parse(content: &[u8]) -> io::Result<Vec<ItemPEntry>> {
    if content.len() < ITEM_P_ENTRY_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("item_p content too short: {} bytes", content.len()),
        ));
    }

    Ok(content
        .chunks_exact(ITEM_P_ENTRY_SIZE)
        .map(ItemPEntry::parse)
        .collect())
}
