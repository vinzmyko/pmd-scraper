//! Item Data Extraction
//!
//! Joins five ROM sources into a single atlas + manifest:
//!
//! - `BALANCE/item_p.bin`: stats, sprite/palette indices
//! - `BALANCE/item_s_p.bin`: exclusive-item metadata, offset-indexed
//! - `MESSAGE/text_*.str`: names, short and long descriptions
//! - `dungeon.bin[1022]`: icon graphics and palettes

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io,
    path::Path,
};

use serde::Serialize;

use crate::{
    containers::{binpack::BinPack, sir0::Sir0},
    data::{
        item_p::{self, ItemPEntry},
        item_s_p::{self, ItemSPEntry},
        text_strings,
    },
    graphics::{grid_atlas, img_itm::ImgItm},
    rom::Rom,
    text_utils::{strip_tags, to_snake_case},
};

/// `dungeon.bin` index of `items.itm.img`.
const ITEMS_IMG_INDEX: usize = 1022;

const ATLAS_FILENAME: &str = "items_atlas.png";

#[derive(Serialize)]
struct ItemManifest {
    atlas: &'static str,
    cell: u32,
    cols: usize,
    rows: usize,
    palettes: Vec<Vec<String>>,
    entries: BTreeMap<String, ItemEntry>,
}

#[derive(Serialize)]
struct ItemEntry {
    id: usize,
    key: String,
    name: String,
    name_raw: String,
    short_desc: Option<String>,
    long_desc: Option<String>,
    sprite: u8,
    palette: u8,
    /// Atlas coordinates of this item's cell, in pixels.
    x: u32,
    y: u32,
    category: u8,
    category_name: &'static str,
    buy_price: u16,
    sell_price: u16,
    move_id: u16,
    /// Stack size on pickup. Only meaningful for thrown categories.
    quantity_min: u8,
    quantity_max: u8,
    action_name: u8,
    in_time_darkness: bool,
    ai_throw_at_enemy: bool,
    ai_throw_at_ally: bool,
    ai_use_on_self: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclusive: Option<ExclusiveEntry>,
}

#[derive(Serialize)]
struct ExclusiveEntry {
    rarity: &'static str,
    slot: Option<u8>,
    exclusive_to: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra_trait: Option<&'static str>,
    /// National Dex number when exclusive to a Pokemon, else the type id.
    parameter: u16,
}

pub struct ItemDataExtractor<'a> {
    rom: &'a Rom,
}

impl<'a> ItemDataExtractor<'a> {
    pub fn new(rom: &'a Rom) -> Self {
        ItemDataExtractor { rom }
    }

    pub fn extract_and_save(&self, binpack: &BinPack, output_dir: &Path) -> io::Result<()> {
        println!("Starting item data extraction...");

        let (names, short_desc, long_desc) = self.load_strings()?;
        println!("  Loaded {} item names", names.len());

        let items = self.load_item_p()?;
        let valid_count = items.iter().filter(|e| e.is_valid).count();
        println!("  Parsed {} entries ({} valid)", items.len(), valid_count);

        let exclusives = self.load_item_s_p()?;
        println!("  Parsed {} exclusive entries", exclusives.len());

        let img = load_img(binpack, ITEMS_IMG_INDEX)?;
        println!(
            "  Icons: {} sprites x {} palettes, {}px cells",
            img.sprite_count,
            img.palettes.len(),
            img.cell
        );

        fs::create_dir_all(output_dir)?;

        let atlas = grid_atlas::build(&img.render_all(), img.palettes.len(), img.cell, img.cell);
        let atlas_path = output_dir.join(ATLAS_FILENAME);
        atlas
            .save(&atlas_path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        println!(
            "  Wrote {} ({}x{})",
            atlas_path.display(),
            atlas.width(),
            atlas.height()
        );

        let entries = build_entries(&items, &exclusives, &names, &short_desc, &long_desc, &img);
        println!("  Built {} manifest entries", entries.len());

        let manifest = ItemManifest {
            atlas: ATLAS_FILENAME,
            cell: img.cell,
            cols: img.palettes.len(),
            rows: img.sprite_count,
            palettes: palettes_hex(&img),
            entries,
        };

        let manifest_path = output_dir.join("items.json");
        serde_json::to_writer_pretty(File::create(&manifest_path)?, &manifest)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        println!("  Wrote {}", manifest_path.display());

        println!("Item data extraction complete!");
        Ok(())
    }

    /// Returns (names, short descriptions, long descriptions), each 1:1 with
    /// item id.
    fn load_strings(&self) -> io::Result<(Vec<String>, Vec<String>, Vec<String>)> {
        let region = &self.rom.region_data;
        let (names_begin, short_begin, long_begin) = (
            region.item_names_begin as usize,
            region.item_short_desc_begin as usize,
            region.item_long_desc_begin as usize,
        );

        if names_begin == 0 || short_begin == 0 || long_begin == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Item string block offsets are not known for this region",
            ));
        }

        let strings = text_strings::parse_string_table(text_strings::load_text_file(self.rom)?)?;
        let count = text_strings::ITEM_COUNT;

        Ok((
            text_strings::block(&strings, names_begin, count)?.to_vec(),
            text_strings::block(&strings, short_begin, count)?.to_vec(),
            text_strings::block(&strings, long_begin, count)?.to_vec(),
        ))
    }

    fn load_item_p(&self) -> io::Result<Vec<ItemPEntry>> {
        let raw = self.read_rom_file("BALANCE/item_p.bin")?;
        item_p::parse(&Sir0::from_bytes(raw)?.content)
    }

    fn load_item_s_p(&self) -> io::Result<Vec<ItemSPEntry>> {
        let raw = self.read_rom_file("BALANCE/item_s_p.bin")?;
        item_s_p::parse(&Sir0::from_bytes(raw)?.content)
    }

    fn read_rom_file(&self, path: &str) -> io::Result<&[u8]> {
        let id = self.rom.fnt.get_file_id(path).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("{} not found", path))
        })?;

        self.rom
            .fat
            .get_file_data(id as usize, &self.rom.data)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Failed to read {} from ROM", path),
                )
            })
    }
}

/// Loads and parses an ImgItm container out of `dungeon.bin`.
pub fn load_img(binpack: &BinPack, index: usize) -> io::Result<ImgItm> {
    let raw = binpack.get(index).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("dungeon.bin index {} not found", index),
        )
    })?;

    let sir0 = Sir0::from_bytes(raw)?;
    ImgItm::from_sir0(&sir0.content, sir0.data_pointer)
}

/// Formats every palette as `#rrggbb` strings so the client can recolour a
/// sprite shape without new art.
pub fn palettes_hex(img: &ImgItm) -> Vec<Vec<String>> {
    img.palettes
        .iter()
        .map(|pal| {
            pal.iter()
                .map(|c| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]))
                .collect()
        })
        .collect()
}

fn build_entries(
    items: &[ItemPEntry],
    exclusives: &[ItemSPEntry],
    names: &[String],
    short_desc: &[String],
    long_desc: &[String],
    img: &ImgItm,
) -> BTreeMap<String, ItemEntry> {
    let mut entries = BTreeMap::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();

    for (id, item) in items.iter().enumerate() {
        // The game's EnsureValidItem substitutes Plain Seed for anything with
        // this bit clear, so invalid entries never render.
        if !item.is_valid {
            continue;
        }

        let name_raw = names.get(id).cloned().unwrap_or_default();
        let name = strip_tags(&name_raw);

        // Distinct items can share a display name (Oran/Oren-style); suffix
        // the id so keys stay unique and stable across runs.
        let base_key = to_snake_case(&name);
        let key = if seen.contains_key(&base_key) {
            format!("{}_{}", base_key, id)
        } else {
            base_key.clone()
        };
        seen.insert(base_key, id);

        let exclusive = item_s_p::index_for_item(id)
            .and_then(|idx| exclusives.get(idx))
            .filter(|e| !e.is_empty())
            .map(|e| ExclusiveEntry {
                rarity: e.info.rarity,
                slot: e.info.slot,
                exclusive_to: e.info.exclusive_to.as_key(),
                extra_trait: e.info.extra_trait,
                parameter: e.parameter,
            });

        entries.insert(
            key.clone(),
            ItemEntry {
                id,
                key,
                name,
                name_raw,
                short_desc: short_desc.get(id).map(|s| strip_tags(s)),
                long_desc: long_desc.get(id).map(|s| strip_tags(s)),
                sprite: item.sprite,
                palette: item.palette,
                x: item.palette as u32 * img.cell,
                y: item.sprite as u32 * img.cell,
                category: item.category as u8,
                category_name: item.category.as_key(),
                buy_price: item.buy_price,
                sell_price: item.sell_price,
                move_id: item.move_id,
                quantity_min: item.range_min,
                quantity_max: item.range_max,
                action_name: item.action_name,
                in_time_darkness: item.is_in_td,
                ai_throw_at_enemy: item.ai_throw_at_enemy,
                ai_throw_at_ally: item.ai_throw_at_ally,
                ai_use_on_self: item.ai_use_on_self,
                exclusive,
            },
        );
    }

    entries
}
