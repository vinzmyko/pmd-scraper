//! Parser for the `MESSAGE/text_*.str` string table.
//!
//! Block offsets are region-dependent and live in `RegionData`.
//! Block lengths are region-agnostic and live here.

use std::io::{self, Cursor};

use crate::{binary_utils::read_u32_le, rom::Rom};

/// Item name / short desc / long desc blocks are each 1:1 with item id.
pub const ITEM_COUNT: usize = 1400;
/// Move name block length.
pub const MOVE_NAME_COUNT: usize = 561;

/// Locates and returns the raw bytes of the ROM's text table.
pub fn load_text_file(rom: &Rom) -> io::Result<&[u8]> {
    const PATHS: [&str; 4] = [
        "MESSAGE/text_e.str",
        "MESSAGE/text_e.bin",
        "MESSAGE/text_j.str",
        "MESSAGE/text_j.bin",
    ];

    PATHS
        .iter()
        .find_map(|&path| {
            rom.fnt
                .get_file_id(path)
                .and_then(|id| rom.fat.get_file_data(id as usize, &rom.data))
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Could not find a text_*.str/.bin table in ROM",
            )
        })
}

/// Parses the pointer-prefixed string table into a index-addressable list.
pub fn parse_string_table(data: &[u8]) -> io::Result<Vec<String>> {
    let mut cursor = Cursor::new(data);
    let mut pointers = Vec::new();

    loop {
        if cursor.position() as usize + 4 > data.len() {
            break;
        }

        let ptr = read_u32_le(&mut cursor)?;
        pointers.push(ptr);

        if ptr as usize >= data.len() {
            break;
        }
        if ptr == cursor.position() as u32 {
            break;
        }
    }

    if pointers.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "String table contains no usable pointers",
        ));
    }

    let mut strings = Vec::with_capacity(pointers.len() - 1);

    for window in pointers.windows(2) {
        let (start, end) = (window[0] as usize, window[1] as usize);

        if start >= data.len() || end > data.len() || start >= end {
            strings.push(String::new());
            continue;
        }

        let slice = &data[start..end];
        let null_pos = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
        strings.push(crate::text_utils::decode_pmd(&slice[..null_pos]));
    }

    Ok(strings)
}

/// Slices a block out of the parsed table.
pub fn block(strings: &[String], begin: usize, count: usize) -> io::Result<&[String]> {
    strings.get(begin..begin + count).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "String block {}..{} out of range (table has {} entries)",
                begin,
                begin + count,
                strings.len()
            ),
        )
    })
}
