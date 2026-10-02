use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    data::{
        animation_info::{
            AnimData, EffectAnimationInfo as GeneralAnim, ItemAnimationInfo as ItemAnim,
            MoveAnimationInfo as MoveAnim, TrapAnimationInfo as TrapAnim,
        },
        monster_md::MonsterData,
    },
    rom::Rom,
};

pub struct AnimationInfoExtractor<'a> {
    rom: &'a mut Rom,
}

impl<'a> AnimationInfoExtractor<'a> {
    pub fn new(rom: &'a mut Rom) -> Self {
        AnimationInfoExtractor { rom }
    }

    pub fn parse_and_transform_animation_data(&mut self) -> AnimData {
        println!("Starting extraction of all animation data");

        let _ = self
            .rom
            .load_arm9_overlays(&[10])
            .map_err(|e| format!("Failed to load overlay 10: {}", e));

        let anim_data = self.rom.extract_animation_data();
        println!("Extracted all animation data tables");
        anim_data.expect("Failed to extract animation data tables")
    }

    pub fn save_animation_info_json(
        &mut self,
        anim_data: &AnimData,
        output_dir: &Path,
    ) -> Result<PathBuf, String> {
        let json_dir = output_dir.join("animation_data");
        fs::create_dir_all(&json_dir)
            .map_err(|e| format!("Failed to create JSON output directory: {}", e))?;

        self.save_trap_animations_json(&json_dir, &anim_data.trap_table)?;
        self.save_item_animations_json(&json_dir, &anim_data.item_table)?;
        let monsters = MonsterData::from_rom(self.rom)
            .map_err(|e| format!("Failed to load monster.md: {}", e))?;
        self.save_move_animation_info_json(&json_dir, anim_data, &monsters)?;
        self.save_effect_animation_table_json(&json_dir, &anim_data.effect_table)?;

        self.save_animation_summary(&json_dir, anim_data)?;

        println!("All animation data saved to {}", json_dir.display());
        Ok(json_dir)
    }

    /// Saves trap animation data to JSON
    fn save_trap_animations_json(&self, dir: &Path, trap_table: &[TrapAnim]) -> Result<(), String> {
        let file_path = dir.join("trap_animations.json");
        crate::json_out::write(&file_path, trap_table)
            .map_err(|e| format!("Failed to write trap_animations.json: {}", e))?;

        println!(
            "Trap animations saved to {} ({} entries)",
            file_path.display(),
            trap_table.len()
        );
        Ok(())
    }

    fn save_item_animations_json(&self, dir: &Path, item_table: &[ItemAnim]) -> Result<(), String> {
        let file_path = dir.join("item_animations.json");
        crate::json_out::write(&file_path, item_table)
            .map_err(|e| format!("Failed to write item_animations.json: {}", e))?;

        println!(
            "Item animations saved to {} ({} entries)",
            file_path.display(),
            item_table.len()
        );
        Ok(())
    }

    /// Saves move animation data to JSON as an object mapping move IDs to animation data
    fn save_move_animation_info_json(
        &self,
        dir: &Path,
        anim_data: &AnimData,
        monsters: &MonsterData,
    ) -> Result<(), String> {
        let file_path = dir.join("move_animation_info.json");

        // Transform the raw move data to the final format with embedded special animations
        let move_map = anim_data.transform_move_data(monsters);

        let move_map_str: HashMap<String, &MoveAnim> = move_map
            .iter()
            .map(|(idx, anim)| (idx.to_string(), anim))
            .collect();

        crate::json_out::write(&file_path, &move_map_str)
            .map_err(|e| format!("Failed to write move_animation_info.json: {}", e))?;

        println!(
            "Move animation info table saved to {} ({} entries)",
            file_path.display(),
            move_map.len()
        );
        Ok(())
    }

    fn save_effect_animation_table_json(
        &self,
        dir: &Path,
        general_table: &[GeneralAnim],
    ) -> Result<(), String> {
        let file_path = dir.join("effect_animation_info.json");

        // Create a JSON object mapping effect_id to general animation data
        let effect_map: HashMap<String, &GeneralAnim> = general_table
            .iter()
            .enumerate()
            .map(|(idx, anim)| (idx.to_string(), anim))
            .collect();

        crate::json_out::write(&file_path, &effect_map)
            .map_err(|e| format!("Failed to write effect_animation_info.json: {}", e))?;

        println!(
            "Effect animation info table saved to {} ({} entries)",
            file_path.display(),
            general_table.len()
        );
        Ok(())
    }

    fn save_animation_summary(&self, dir: &Path, anim_data: &AnimData) -> Result<(), String> {
        let file_path = dir.join("animation_summary.json");

        let summary = serde_json::json!({
            "trap_table_count": anim_data.trap_table.len(),
            "item_table_count": anim_data.item_table.len(),
            "move_table_count": anim_data.raw_move_table.len(),
            "general_table_count": anim_data.effect_table.len(),
            "special_move_table_count": anim_data.special_move_table.len(),
            "game_id": self.rom.id_code,
            "region": match self.rom.id_code.chars().last() {
                Some('E') => "NA",
                Some('P') => "EU",
                Some('J') => "JP",
                _ => "Unknown"
            }
        });

        crate::json_out::write(&file_path, &summary)
            .map_err(|e| format!("Failed to write animation_summary.json: {}", e))?;

        println!("Animation summary saved to {}", file_path.display());
        Ok(())
    }
}
