//! # Exclusive Item Data (`BALANCE/item_s_p.bin`)
//!
//! SIR0-wrapped, 4 bytes per entry, 956 entries in the US ROM. Exclusive items are item ids
//! (444-1351), this table only adds rarity and metadata.

use std::io;

pub const ITEM_S_P_ENTRY_SIZE: usize = 4;

/// Item id of Prism Ruff, which occupies table index 0.
pub const FIRST_EXCLUSIVE_ITEM_ID: usize = 444;

/// What an exclusive item is restricted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExclusiveTo {
    /// The entry carries no exclusive-item metadata.
    NotApplicable,
    Monster,
    Type,
}

impl ExclusiveTo {
    pub fn as_key(&self) -> &'static str {
        match self {
            Self::NotApplicable => "not_applicable",
            Self::Monster => "monster",
            Self::Type => "type",
        }
    }
}

/// Decoded properties of an exclusive item type id.
#[derive(Debug, Clone, Copy)]
pub struct ExclusiveTypeInfo {
    /// Star rating as it appears in game: "-", "*", "**" or "***".
    pub rarity: &'static str,
    /// Held-item slot restriction, where one exists.
    pub slot: Option<u8>,
    pub exclusive_to: ExclusiveTo,
    /// Extra behaviour noted for this type, if any.
    pub extra_trait: Option<&'static str>,
}

/// Decodes the `type` field. Unknown ids fall back to `NotApplicable`.
pub fn type_info(raw: u16) -> ExclusiveTypeInfo {
    let (rarity, slot, exclusive_to, extra_trait): (
        &'static str,
        Option<u8>,
        ExclusiveTo,
        Option<&'static str>,
    ) = match raw {
        0x01 => ("*", Some(1), ExclusiveTo::Type, None),
        0x02 => ("*", Some(2), ExclusiveTo::Type, None),
        0x03 => ("**", None, ExclusiveTo::Type, None),
        0x04 => ("***", None, ExclusiveTo::Type, None),
        0x05 => ("*", Some(1), ExclusiveTo::Monster, None),
        0x06 => ("*", Some(2), ExclusiveTo::Monster, None),
        0x07 => ("**", None, ExclusiveTo::Monster, None),
        0x08 => ("***", None, ExclusiveTo::Monster, None),
        0x09 => (
            "***",
            None,
            ExclusiveTo::Monster,
            Some("The Pokemon may hatch holding the item."),
        ),
        0x0A => (
            "***",
            None,
            ExclusiveTo::Monster,
            Some("Unknown; only the Eeveelutions and the Tyrogue line have this type."),
        ),
        _ => ("-", None, ExclusiveTo::NotApplicable, None),
    };

    ExclusiveTypeInfo {
        rarity,
        slot,
        exclusive_to,
        extra_trait,
    }
}

#[derive(Debug, Clone)]
pub struct ItemSPEntry {
    pub raw_type: u16,
    pub info: ExclusiveTypeInfo,
    /// National Dex number when exclusive to a Pokemon, or the type id when
    /// exclusive to a type.
    pub parameter: u16,
}

impl ItemSPEntry {
    fn parse(data: &[u8]) -> Self {
        let raw_type = u16::from_le_bytes([data[0], data[1]]);
        ItemSPEntry {
            raw_type,
            info: type_info(raw_type),
            parameter: u16::from_le_bytes([data[2], data[3]]),
        }
    }

    /// True when this entry carries no exclusive-item metadata.
    pub fn is_empty(&self) -> bool {
        self.info.exclusive_to == ExclusiveTo::NotApplicable
    }
}

/// Parses the unwrapped SIR0 content into the offset-indexed exclusive table.
/// Use [`index_for_item`] to address it by item id.
pub fn parse(content: &[u8]) -> io::Result<Vec<ItemSPEntry>> {
    if content.len() < ITEM_S_P_ENTRY_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("item_s_p content too short: {} bytes", content.len()),
        ));
    }

    Ok(content
        .chunks_exact(ITEM_S_P_ENTRY_SIZE)
        .map(ItemSPEntry::parse)
        .collect())
}

/// Maps an item id to its index in the exclusive table, or `None` if the item
/// sits below the exclusive range.
pub fn index_for_item(item_id: usize) -> Option<usize> {
    item_id.checked_sub(FIRST_EXCLUSIVE_ITEM_ID)
}
