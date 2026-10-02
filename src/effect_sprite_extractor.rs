use std::{
    collections::{BTreeSet, HashMap},
    fs::{self},
    io::{self},
    path::Path,
};

use crate::{
    containers::{
        binpack::BinPack,
        compression::pkdpx::PkdpxContainer,
        sir0::{self},
        ContainerHandler,
    },
    data::animation_info::{AnimType, EffectAnimationInfo, MoveAnimationInfo},
    graphics::{
        screen_effect::{parse_screen_effect, render_screen_frame, ScreenEffectFile},
        wan::{
            model::WanFile,
            parser::{parse_wan_from_sir0_content, parse_wan_palette_only},
            renderer, AnimationStructure, ImgPiece, PaletteList,
        },
        WanType,
    },
    move_effects_index::{
        AnimationDetails, AnimationSequence, EffectDefinition, EffectLayer, MoveData,
        MoveEffectTrigger, MoveEffectsIndex, ScreenEffect, ScreenFrameInfo, SpriteEffect,
        SpriteLayer,
    },
    phases::PhaseId,
    progress::ProgressReporter,
    rom::Rom,
};

/// Handles the entire pipeline of extracting effect sprites and compiling the move/effect index
pub struct EffectAssetPipeline<'a> {
    rom: &'a Rom,
    wan_cache: HashMap<usize, WanFile>,
    effect_bin: Option<BinPack>,
    base_palette: Option<PaletteList>,
    base_wan_file292: Option<WanFile>,
}

impl<'a> EffectAssetPipeline<'a> {
    pub fn new(rom: &'a Rom) -> Self {
        EffectAssetPipeline {
            rom,
            wan_cache: HashMap::new(),
            effect_bin: None,
            base_palette: None,
            base_wan_file292: None,
        }
    }

    /// Renders sprites, saves them, and generates a final `asset_index.json`
    pub fn run(
        &mut self,
        effects_map: &HashMap<u16, EffectAnimationInfo>,
        moves_map: &HashMap<usize, MoveAnimationInfo>,
        output_dir: &Path,
        reporter: &mut ProgressReporter,
    ) -> io::Result<()> {
        println!("\n--- Starting Effect Asset Pipeline ---");

        self.load_bin_containers()?;

        let sprites_dir = output_dir.join("EFFECT");
        fs::create_dir_all(&sprites_dir)?;

        let mut index = MoveEffectsIndex::new();
        let mut effects_processed = 0;
        let mut effects_skipped = 0;
        let mut errors = 0;

        let mut sorted_effect_ids: Vec<_> = effects_map.keys().collect();
        sorted_effect_ids.sort();

        reporter.begin(PhaseId::MoveEffectSprites, sorted_effect_ids.len());

        for effect_id in sorted_effect_ids {
            let effect_info = &effects_map[effect_id];
            let anim_type = effect_info.anim_type;

            println!(
                "Processing Effect ID: {} (Type: {:?})",
                effect_id, anim_type
            );

            let effect_entry = match anim_type {
                AnimType::WanOther => {
                    match self.process_sprite_effect(*effect_id, effect_info, &sprites_dir, None) {
                        Ok(Some(entry)) => {
                            effects_processed += 1;
                            Some(entry)
                        }
                        Ok(None) => {
                            effects_skipped += 1; // Empty animations
                            None
                        }
                        Err(e) => {
                            eprintln!(" -> ERROR processing effect {}: {}", effect_id, e);
                            errors += 1;
                            None
                        }
                    }
                }
                AnimType::WanFile0 => {
                    match self.process_sprite_effect(*effect_id, effect_info, &sprites_dir, Some(0))
                    {
                        Ok(Some(entry)) => {
                            effects_processed += 1;
                            Some(entry)
                        }
                        Ok(None) => {
                            effects_skipped += 1;
                            None
                        }
                        Err(e) => {
                            eprintln!(" -> ERROR processing effect {}: {}", effect_id, e);
                            errors += 1;
                            None
                        }
                    }
                }
                AnimType::WanFile1 => {
                    match self.process_sprite_effect(*effect_id, effect_info, &sprites_dir, Some(1))
                    {
                        Ok(Some(entry)) => {
                            effects_processed += 1;
                            Some(entry)
                        }
                        Ok(None) => {
                            effects_skipped += 1;
                            None
                        }
                        Err(e) => {
                            eprintln!(" -> ERROR processing effect {}: {}", effect_id, e);
                            errors += 1;
                            None
                        }
                    }
                }
                AnimType::Screen => {
                    match self.process_screen_effect(*effect_id, effect_info, &sprites_dir) {
                        Ok(Some(entry)) => {
                            effects_processed += 1;
                            Some(entry)
                        }
                        Ok(None) => {
                            effects_skipped += 1;
                            None
                        }
                        Err(e) => {
                            eprintln!(" -> ERROR processing screen effect {}: {}", effect_id, e);
                            errors += 1;
                            None
                        }
                    }
                }
                _ => {
                    println!(" -> Skipping: Unsupported type");
                    effects_skipped += 1;
                    None
                }
            };

            if let Some(entry) = effect_entry {
                index.effects.insert(effect_id.to_string(), entry);
            }

            // Skips and errors are still work done.
            reporter.advance();
        }

        println!("Populating moves data...");
        self.populate_moves_data(&mut index, moves_map);

        // Write the complete index to disk
        self.save_index(&index, output_dir)?;

        println!("\n---------------------------------");
        println!("Effect Asset Pipeline Complete!");
        println!("  Sprites Processed: {}", effects_processed);
        println!("  Effects Skipped (by design): {}", effects_skipped);
        println!("  Errors: {}", errors);
        println!("---------------------------------");

        Ok(())
    }

    /// Determines if an effect is directional based on ROM behavior.
    fn check_directional_effect(
        &self,
        wan_file: &WanFile,
        base_animation_index: usize,
    ) -> (bool, bool) {
        let sequence_count = wan_file.max_sequences_per_group as usize;

        // Direction is added to animation_index if sequence_count % 8 == 0
        let is_directional = sequence_count > 0 && sequence_count % 8 == 0;

        // Verify all 8 direction sequences exist
        let can_render_all = if is_directional {
            base_animation_index + 7 < sequence_count
        } else {
            false
        };

        (is_directional, can_render_all)
    }

    /// Renders, saves, and builds the definition for a 'WanOther' type effect.
    /// Handles both directional effects (8 sprite sheets) and non-directional effects (1 sheet).
    fn process_sprite_effect(
        &mut self,
        effect_id: u16,
        effect_info: &EffectAnimationInfo,
        sprites_dir: &Path,
        override_file_index: Option<usize>,
    ) -> io::Result<Option<EffectDefinition>> {
        let file_index = override_file_index.unwrap_or(effect_info.file_index as usize);
        let base_anim_index = effect_info.animation_index as usize;

        // Cache already scanned effect sprites
        self.ensure_effect_wan_cached(file_index)?;

        let wan_file = self.wan_cache.get(&file_index).unwrap();

        // Determine directionality based on ROM behavior
        let (is_directional, can_render_all_directions) =
            self.check_directional_effect(wan_file, base_anim_index);

        if is_directional {
            println!(
                " -> Directional effect detected (sequence_count={}, base_index={})",
                wan_file.max_sequences_per_group, base_anim_index
            );
        }

        let rendered_sequences = if is_directional && can_render_all_directions {
            base_anim_index..base_anim_index + 8
        } else {
            base_anim_index..base_anim_index + 1
        };
        let draw_order =
            DrawOrderOffsets::from_set(wan_file.effect_draw_order_offsets(rendered_sequences));
        if draw_order.is_mixed() {
            println!(" -> Mixed draw order offsets {:?}", draw_order.0);
        }

        if is_directional && can_render_all_directions {
            self.process_directional_effect(
                effect_id,
                effect_info,
                wan_file,
                base_anim_index,
                &draw_order,
                sprites_dir,
            )
        } else {
            if is_directional && !can_render_all_directions {
                println!(
                    " -> WARNING: Directional effect but base_index {} + 7 >= sequence_count {}. Falling back to single sheet.",
                    base_anim_index, wan_file.max_sequences_per_group
                );
            }
            self.process_non_directional_effect(
                effect_id,
                effect_info,
                wan_file,
                base_anim_index,
                &draw_order,
                sprites_dir,
            )
        }
    }

    /// Calculates the unified canvas box that encompasses all 8 directional animations.
    fn calculate_unified_canvas_box(
        &self,
        wan_file: &WanFile,
        base_anim_index: usize,
    ) -> Option<(i16, i16, i16, i16)> {
        let mut unified_box: Option<(i16, i16, i16, i16)> = None;

        // Collect bounds from all 8 directions
        for direction in 0u8..8 {
            let anim_index = base_anim_index + direction as usize;

            if let Ok(Some(canvas_box)) =
                renderer::get_effect_animation_canvas_box(wan_file, anim_index)
            {
                unified_box = Some(match unified_box {
                    None => canvas_box,
                    Some(current) => {
                        (
                            current.0.min(canvas_box.0), // min x
                            current.1.min(canvas_box.1), // min y
                            current.2.max(canvas_box.2), // max x
                            current.3.max(canvas_box.3), // max y
                        )
                    }
                });
            }
        }

        // If we got a unified box, ensure dimensions are multiples of 8
        unified_box.map(|b| {
            let width = b.2 - b.0;
            let height = b.3 - b.1;

            // Round up to multiples of 8
            let new_width = ((width + 7) / 8) * 8;
            let new_height = ((height + 7) / 8) * 8;

            // Center the expanded box
            let width_diff = new_width - width;
            let height_diff = new_height - height;

            (
                b.0 - width_diff / 2,
                b.1 - height_diff / 2,
                b.0 - width_diff / 2 + new_width,
                b.1 - height_diff / 2 + new_height,
            )
        })
    }

    /// Processes a directional effect: 8 flattened sheets, plus 8 per draw order layer
    /// for mixed effects. Every sheet shares one canvas box across all directions and
    /// layers so they line up when stacked.
    fn process_directional_effect(
        &self,
        effect_id: u16,
        effect_info: &EffectAnimationInfo,
        wan_file: &WanFile,
        base_anim_index: usize,
        draw_order: &DrawOrderOffsets,
        sprites_dir: &Path,
    ) -> io::Result<Option<EffectDefinition>> {
        let Some(canvas_box) = self.calculate_unified_canvas_box(wan_file, base_anim_index) else {
            println!(" -> WARNING: Could not calculate unified canvas. Skipping effect.");
            return Ok(None);
        };
        let frame_width = (canvas_box.2 - canvas_box.0) as u32;
        let frame_height = (canvas_box.3 - canvas_box.1) as u32;
        println!(
            " -> Unified canvas: {}x{} (from box {:?})",
            frame_width, frame_height, canvas_box
        );

        for offset in draw_order.sheet_variants() {
            let any_rendered = self.save_directional_sheets(
                effect_id,
                offset,
                wan_file,
                base_anim_index,
                canvas_box,
                sprites_dir,
            )?;
            if offset.is_none() && !any_rendered {
                println!(" -> WARNING: No directions rendered successfully. Skipping effect.");
                return Ok(None);
            }
        }

        // The 8 direction sequences share frame counts and timing, so direction 0 stands in.
        let effect_definition = self.build_sprite_effect_definition_directional(
            effect_info,
            effect_id,
            base_anim_index,
            frame_width,
            frame_height,
            effect_sequence(wan_file, base_anim_index),
            true,
            8,
            draw_order,
        );

        println!(
            " -> SUCCESS: directional sprite sheets saved (unified {}x{})",
            frame_width, frame_height
        );
        Ok(Some(effect_definition))
    }

    /// Renders and saves the 8 direction sheets for one layer (`Some(offset)`) or for the
    /// flattened effect (`None`). Returns whether any direction rendered.
    fn save_directional_sheets(
        &self,
        effect_id: u16,
        offset: Option<i8>,
        wan_file: &WanFile,
        base_anim_index: usize,
        canvas_box: (i16, i16, i16, i16),
        sprites_dir: &Path,
    ) -> io::Result<bool> {
        let stem = sheet_stem(effect_id, offset);
        let mut any_rendered = false;

        for direction in 0..8 {
            match renderer::render_effect_animation_sheet_with_canvas(
                wan_file,
                base_anim_index + direction,
                Some(canvas_box),
                offset,
            ) {
                Ok(Some((sprite_sheet, _, _))) => {
                    let filename = format!("{}_dir{}.png", stem, direction);
                    self.save_effect_sprite_png(&sprite_sheet, &sprites_dir.join(&filename))?;
                    println!(" -> Direction {}: saved {}", direction, filename);
                    any_rendered = true;
                }
                Ok(None) => println!(" -> Direction {}: empty/no visible pixels", direction),
                Err(e) => eprintln!(" -> Direction {}: render error: {:?}", direction, e),
            }
        }

        Ok(any_rendered)
    }

    /// Processes a non-directional effect: one flattened sheet, plus one per draw order
    /// layer for mixed effects, all on the same canvas box.
    fn process_non_directional_effect(
        &self,
        effect_id: u16,
        effect_info: &EffectAnimationInfo,
        wan_file: &WanFile,
        anim_index: usize,
        draw_order: &DrawOrderOffsets,
        sprites_dir: &Path,
    ) -> io::Result<Option<EffectDefinition>> {
        let Some(canvas_box) = renderer::get_effect_animation_canvas_box(wan_file, anim_index)?
        else {
            println!(" -> WARNING: Animation is empty or has no visible pixels. Skipping.");
            return Ok(None);
        };
        let frame_width = (canvas_box.2 - canvas_box.0) as u32;
        let frame_height = (canvas_box.3 - canvas_box.1) as u32;

        for offset in draw_order.sheet_variants() {
            let Some((sprite_sheet, _, _)) = renderer::render_effect_animation_sheet_with_canvas(
                wan_file,
                anim_index,
                Some(canvas_box),
                offset,
            )?
            else {
                println!(" -> WARNING: Animation has no frames. Skipping.");
                return Ok(None);
            };
            let sheet_path = sprites_dir.join(format!("{}.png", sheet_stem(effect_id, offset)));
            self.save_effect_sprite_png(&sprite_sheet, &sheet_path)?;
            println!(
                " -> SUCCESS: Sprite sheet saved to {}",
                sheet_path.display()
            );
        }

        Ok(Some(self.build_sprite_effect_definition_directional(
            effect_info,
            effect_id,
            anim_index,
            frame_width,
            frame_height,
            effect_sequence(wan_file, anim_index),
            false,
            1,
            draw_order,
        )))
    }

    /// Builds the `SpriteEffect` data structure from a rendered animation.
    #[allow(clippy::too_many_arguments)]
    fn build_sprite_effect_definition_directional(
        &self,
        effect_info: &EffectAnimationInfo,
        effect_id: u16,
        base_animation_index: usize,
        frame_width: u32,
        frame_height: u32,
        animation_sequence: Option<&crate::graphics::wan::model::Animation>,
        is_directional: bool,
        direction_count: u8,
        draw_order: &DrawOrderOffsets,
    ) -> EffectDefinition {
        let mut animations = HashMap::new();

        if let Some(animation_sequence) = animation_sequence {
            let frame_details: Vec<[f32; 3]> = animation_sequence
                .frames
                .iter()
                .map(|frame| {
                    let duration_sec = ticks_to_seconds(frame.duration);
                    // zero out the offsets in the JSON since they're now baked into animation sheet
                    [duration_sec, 0.0, 0.0]
                })
                .collect();

            // Check if all frames have the same duration and zero offset
            let is_simple = if frame_details.len() > 1 {
                let first_duration = frame_details[0][0];
                frame_details.iter().all(|frame| {
                    (frame[0] - first_duration).abs() < f32::EPSILON
                        && frame[1] == 0.0
                        && frame[2] == 0.0
                })
            } else {
                // A single frame animation is simple if its offset is zero
                matches!(frame_details.first(), Some(f) if f[1] == 0.0 && f[2] == 0.0)
            };

            let animation_details = if is_simple {
                AnimationDetails::Simple {
                    frame_count: frame_details.len(),
                    duration: frame_details.first().map(|f| f[0]).unwrap_or(0.1),
                }
            } else {
                AnimationDetails::Complex {
                    frames: frame_details,
                }
            };

            animations.insert(
                "play".to_string(),
                AnimationSequence {
                    looping: effect_info.loop_flag,
                    details: animation_details,
                },
            );
        }

        let layers = draw_order
            .layer_offsets()
            .iter()
            .map(|&offset| SpriteLayer {
                draw_order_offset: offset,
                sprite_sheet: sheet_index_path(effect_id, Some(offset), is_directional),
            })
            .collect();

        EffectDefinition::Sprite(SpriteEffect {
            sprite_sheet: sheet_index_path(effect_id, None, is_directional),
            frame_width: frame_width.max(1),
            frame_height: frame_height.max(1),
            animations,
            is_directional,
            direction_count,
            base_animation_index: base_animation_index as u32,
            is_non_blocking: effect_info.is_non_blocking,
            draw_order_offset: draw_order.uniform(),
            draw_order_offsets: draw_order.0.clone(),
            layers,
        })
    }

    /// Populates the `moves` section of the index.
    fn populate_moves_data(
        &self,
        index: &mut MoveEffectsIndex,
        moves_map: &HashMap<usize, MoveAnimationInfo>,
    ) {
        let mut sorted_move_ids: Vec<_> = moves_map.keys().collect();
        sorted_move_ids.sort();

        for move_id in sorted_move_ids {
            let move_info = &moves_map[move_id];
            let mut move_effects = Vec::new();

            // Effect layers with their purposes from ROM findings
            let effect_layers = [
                (move_info.effect_id_1, EffectLayer::Charge),
                (move_info.effect_id_2, EffectLayer::Secondary),
                (move_info.effect_id_3, EffectLayer::Primary),
                (move_info.effect_id_4, EffectLayer::Projectile),
            ];

            for (effect_id, layer) in effect_layers {
                if effect_id > 0 && index.effects.contains_key(&effect_id.to_string()) {
                    move_effects.push(MoveEffectTrigger {
                        id: effect_id.to_string(),
                        layer,
                        trigger: "OnExecute".to_string(),
                    });
                }
            }

            if !move_effects.is_empty() {
                index.moves.insert(
                    move_id.to_string(),
                    MoveData {
                        effects: move_effects,
                    },
                );
            }
        }
    }

    /// Caches a WAN file if it's not already loaded.
    fn ensure_effect_wan_cached(&mut self, effect_index: usize) -> io::Result<()> {
        if self.wan_cache.contains_key(&effect_index) {
            return Ok(());
        }

        let effect_bin = self
            .effect_bin
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "effect.bin not loaded"))?;
        if effect_index >= effect_bin.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Effect index {} out of range for effect.bin", effect_index),
            ));
        }

        let sprite_data = &effect_bin[effect_index];
        let mut wan_file = self.parse_wan_from_data(sprite_data, WanType::Effect, false)?;

        if let Some(base_palette) = &self.base_palette {
            if wan_file.palette_offset > 0 {
                let mut merged_palette = base_palette.clone();
                let effect_own_palette = wan_file.custom_palette.clone();
                let offset = wan_file.palette_offset as usize;

                for (i, effect_row) in effect_own_palette.iter().enumerate() {
                    let target_idx = offset + i;
                    while merged_palette.len() <= target_idx {
                        merged_palette.push(vec![(0, 0, 0, 0); effect_row.len()]);
                    }
                    merged_palette[target_idx] = effect_row.clone();
                }

                wan_file.custom_palette = merged_palette;
                wan_file.palette_offset = 0;
            }
        }

        self.wan_cache.insert(effect_index, wan_file);
        Ok(())
    }

    fn save_index(&self, index: &MoveEffectsIndex, output_dir: &Path) -> io::Result<()> {
        let output_path = output_dir.join("asset_index.json");
        println!("Writing final index to {}...", output_path.display());
        crate::json_out::write(&output_path, index)
    }

    fn save_effect_sprite_png(&self, image: &image::RgbaImage, path: &Path) -> io::Result<()> {
        let temp_path = path.with_extension("temp.png");
        image
            .save(&temp_path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // No difference between compression setting 6 and 2 size wise just so much faster
        let mut options = oxipng::Options::from_preset(2);
        options.bit_depth_reduction = true;
        options.interlace = None;

        match oxipng::optimize(
            &oxipng::InFile::Path(temp_path.clone()),
            &oxipng::OutFile::Path(Some(path.to_path_buf())),
            &options,
        ) {
            Ok(_) => {
                let _ = fs::remove_file(temp_path);
                Ok(())
            }
            Err(e) => {
                fs::rename(temp_path, path)?;
                eprintln!(
                    "Warning: oxipng optimisation failed for {}: {}. File saved unoptimised.",
                    path.display(),
                    e
                );
                Ok(())
            }
        }
    }

    fn load_bin_containers(&mut self) -> io::Result<()> {
        if self.effect_bin.is_some() {
            return Ok(());
        }

        let effect_bin_id = self
            .rom
            .fnt
            .get_file_id("EFFECT/effect.bin")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "effect.bin not found"))?;
        let effect_bin_data = self
            .rom
            .fat
            .get_file_data(effect_bin_id as usize, &self.rom.data)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Failed to extract effect.bin")
            })?;

        println!("Parsing effect.bin...");
        let effect_bin = BinPack::from_bytes(effect_bin_data)?;
        println!(
            " -> Success. Found {} files in the archive.",
            effect_bin.len()
        );

        let base_palette_index = 292;
        if effect_bin.len() > base_palette_index {
            println!(
                "Loading Base Palette from effect.bin[{}]...",
                base_palette_index
            );
            let base_palette_data = &effect_bin[base_palette_index];

            // Parse palette-only for the base_palette field (existing behavior)
            match self.parse_wan_from_data(base_palette_data, WanType::Effect, true) {
                Ok(base_wan) => {
                    self.base_palette = Some(base_wan.custom_palette);
                    println!(" -> Base Palette loaded successfully.");
                }
                Err(e) => {
                    eprintln!(
                        "Fatal Error: Could not load the base palette: {}. Cannot continue.",
                        e
                    );
                    return Err(e);
                }
            }

            // Parse fully for WanFile0/1 image data merging
            match self.parse_wan_from_data(base_palette_data, WanType::Effect, false) {
                Ok(full_wan) => {
                    self.base_wan_file292 = Some(full_wan);
                    println!(" -> Full file 292 WAN parsed for image data.");
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Could not fully parse file 292: {}. WanFile0/1 effects may fail.",
                        e
                    );
                }
            }
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "effect.bin is too small to contain the base palette.",
            ));
        }

        // Pre-cache shared WAN files 0 and 1 by merging file 292 images with file 0/1 animations
        if let Some(ref base_wan) = self.base_wan_file292 {
            for shared_idx in [0usize] {
                if shared_idx < effect_bin.len() {
                    let sprite_data = &effect_bin[shared_idx];
                    match self.parse_wan_from_data(sprite_data, WanType::Effect, false) {
                        Ok(anim_wan) => {
                            // Merge: file 292 provides palette only,
                            // file 0/1 provides images, frames, and animations
                            let merged_wan = WanFile {
                                img_data: {
                                    // Build VRAM with 128-byte block alignment.
                                    // Empty chunks occupy one block. Data chunks pad to next boundary.
                                    let block_size = 128usize;
                                    let mut padded_vram: Vec<u8> = Vec::new();

                                    for piece in &base_wan.img_data {
                                        if piece.img_px.is_empty() {
                                            continue;
                                        } else {
                                            padded_vram.extend_from_slice(&piece.img_px);
                                            let remainder = padded_vram.len() % block_size;
                                            if remainder != 0 {
                                                padded_vram.resize(
                                                    padded_vram.len() + (block_size - remainder),
                                                    0,
                                                );
                                            }
                                        }
                                    }

                                    // Tail-slices at 128-byte granularity so multi-tile pieces read forward
                                    let num_tiles = padded_vram.len() / block_size;
                                    (0..num_tiles)
                                        .map(|i| ImgPiece {
                                            img_px: padded_vram[i * block_size..].to_vec(),
                                        })
                                        .collect()
                                },
                                frame_data: anim_wan.frame_data,
                                animations: anim_wan.animations,
                                body_part_offset_data: anim_wan.body_part_offset_data,
                                custom_palette: self.base_palette.clone().unwrap_or_default(),
                                effect_specific_palette: None,
                                wan_type: WanType::Effect,
                                palette_offset: 0,
                                tile_lookup_8bpp: {
                                    // Identity lookup: tile_num N → img_data[N]
                                    // Build padded len same way to get correct count
                                    let block_size = 128usize;
                                    let mut padded_len = 0usize;
                                    for piece in &base_wan.img_data {
                                        if piece.img_px.is_empty() {
                                            continue;
                                        } else {
                                            padded_len += piece.img_px.len();
                                            let remainder = padded_len % block_size;
                                            if remainder != 0 {
                                                padded_len += block_size - remainder;
                                            }
                                        }
                                    }
                                    let max_tile = padded_len / block_size;
                                    Some((0..max_tile).map(|i| (i, i)).collect())
                                },
                                max_sequences_per_group: anim_wan.max_sequences_per_group,
                                offset_table_size: 0,
                            };
                            println!(
                                " -> Shared WAN file {} merged successfully (frames: {}, sequences: {}).",
                                shared_idx,
                                merged_wan.frame_data.len(),
                                merged_wan.max_sequences_per_group
                            );

                            self.wan_cache.insert(shared_idx, merged_wan);

                            if let Some(cached_wan) = self.wan_cache.get_mut(&shared_idx) {
                                for frame in &mut cached_wan.frame_data {
                                    for piece in &mut frame.pieces {
                                        piece.is_256_colour = true;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "Warning: Failed to parse shared WAN file {}: {}",
                                shared_idx, e
                            );
                        }
                    }
                }
            }
        }

        self.effect_bin = Some(effect_bin);
        Ok(())
    }

    fn parse_wan_from_data(
        &self,
        data: &[u8],
        wan_type: WanType,
        palette_only: bool,
    ) -> io::Result<WanFile> {
        let decompressed_data = if data.starts_with(b"PKDPX") {
            match PkdpxContainer::deserialise(data) {
                Ok(pkdpx) => pkdpx.decompress().map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("PKDPX decompress error: {}", e),
                    )
                })?,
                Err(e) => return Err(e),
            }
        } else {
            data.to_vec()
        };

        if !decompressed_data.starts_with(b"SIR0") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data is not in SIR0 format",
            ));
        }

        let sir0_data = sir0::Sir0::from_bytes(&decompressed_data)?;
        if sir0_data.data_pointer as usize >= sir0_data.content.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SIR0 data pointer out of bounds",
            ));
        }

        let parse_result = if palette_only {
            parse_wan_palette_only(&sir0_data.content, sir0_data.data_pointer)
        } else {
            parse_wan_from_sir0_content(&sir0_data.content, sir0_data.data_pointer, wan_type)
        };

        parse_result.map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("WAN parse error: {:?}", e),
            )
        })
    }

    /// Renders a type-5 screen effect to a horizontal sheet and builds its index entry.
    ///
    /// The actual file lives at `effect_animation_info.file_index + 0x10C` (268).
    /// Returns `Ok(None)` when the effect has no frames.
    fn process_screen_effect(
        &self,
        effect_id: u16,
        effect_info: &EffectAnimationInfo,
        sprites_dir: &Path,
    ) -> io::Result<Option<EffectDefinition>> {
        let file_index = effect_info.file_index as usize + 0x10C;

        let effect_bin = self
            .effect_bin
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "effect.bin not loaded"))?;
        if file_index >= effect_bin.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Screen effect file_index {} out of range", file_index),
            ));
        }

        let screen = parse_screen_effect_from_data(&effect_bin[file_index])?;
        if screen.frames.is_empty() {
            println!(
                " -> WARNING: screen effect {} has no frames, skipping",
                effect_id
            );
            return Ok(None);
        }

        // Every frame is exactly 256x160; stitch them into one horizontal strip.
        let count = screen.frames.len() as u32;
        let mut sheet = image::RgbaImage::new(256 * count, 160);
        for (i, frame) in screen.frames.iter().enumerate() {
            let fimg = render_screen_frame(&screen, frame);
            image::imageops::overlay(&mut sheet, &fimg, (i as u32 * 256) as i64, 0);
        }

        let sheet_path = sprites_dir.join(format!("{}.png", effect_id));
        self.save_effect_sprite_png(&sheet, &sheet_path)?;
        println!(
            " -> SUCCESS: Screen sheet saved to {}",
            sheet_path.display()
        );

        let frames_meta = screen
            .frames
            .iter()
            .map(|f| ScreenFrameInfo {
                duration: ticks_to_seconds(f.duration),
                alpha: f.alpha,
                row_height: f.row_height,
            })
            .collect();

        Ok(Some(EffectDefinition::Screen(ScreenEffect {
            effect_name: format!("ScreenEffect_{}", effect_id),
            sprite_sheet: format!("res://effect_sprites/{}.png", effect_id),
            frame_width: 256,
            frame_height: 160,
            frame_count: screen.frames.len(),
            looping: effect_info.loop_flag,
            is_non_blocking: effect_info.is_non_blocking,
            frames: frames_meta,
        })))
    }
}

/// Distinct per-piece draw order offsets of one effect, sorted ascending. Never empty.
struct DrawOrderOffsets(Vec<i8>);

impl DrawOrderOffsets {
    fn from_set(set: BTreeSet<i8>) -> Self {
        if set.is_empty() {
            Self(vec![0])
        } else {
            Self(set.into_iter().collect())
        }
    }

    /// Fragments use more than one offset, so the effect wraps around other sprites.
    fn is_mixed(&self) -> bool {
        self.0.len() > 1
    }

    /// The whole effect's offset when uniform. 0 when mixed (layers carry the real values).
    fn uniform(&self) -> i8 {
        if self.is_mixed() {
            0
        } else {
            self.0[0]
        }
    }

    /// Offsets that get their own sheet. Empty for uniform effects.
    fn layer_offsets(&self) -> &[i8] {
        if self.is_mixed() {
            &self.0
        } else {
            &[]
        }
    }

    /// The flattened sheet (`None`) first, then one entry per layer.
    fn sheet_variants(&self) -> impl Iterator<Item = Option<i8>> + '_ {
        std::iter::once(None).chain(self.layer_offsets().iter().copied().map(Some))
    }
}

/// File stem shared by every sheet of an effect: `{id}`, or `{id}_offset_{offset}` for a
/// draw order layer. Directional sheets append `_dir{d}`, and all sheets end in `.png`.
fn sheet_stem(effect_id: u16, offset: Option<i8>) -> String {
    match offset {
        Some(offset) => format!("{}_offset_{}", effect_id, offset),
        None => effect_id.to_string(),
    }
}

/// Index path for a sheet. A directional path is the base the client appends `_dir{d}.png` to.
fn sheet_index_path(effect_id: u16, offset: Option<i8>, is_directional: bool) -> String {
    let stem = sheet_stem(effect_id, offset);
    if is_directional {
        format!("res://effect_sprites/{}", stem)
    } else {
        format!("res://effect_sprites/{}.png", stem)
    }
}

/// Sequence `index` of an effect WAN's group 0, the only group the ROM reads.
fn effect_sequence(
    wan_file: &WanFile,
    index: usize,
) -> Option<&crate::graphics::wan::model::Animation> {
    match &wan_file.animations {
        AnimationStructure::Effect(groups) => groups.first().and_then(|g| g.get(index)),
        AnimationStructure::Character(_) => None,
    }
}

/// Convert raw animation ticks (1/60s) to seconds.
fn ticks_to_seconds(ticks: u16) -> f32 {
    (ticks as f32 / 59.8261 * 10000.0).round() / 10000.0
}

/// Decompress (PKDPX or raw), unwrap SIR0, and parse a screen effect.
fn parse_screen_effect_from_data(data: &[u8]) -> io::Result<ScreenEffectFile> {
    let decompressed = if data.starts_with(b"PKDPX") {
        PkdpxContainer::deserialise(data)?
            .decompress()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
    } else {
        data.to_vec()
    };
    if !decompressed.starts_with(b"SIR0") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Screen effect data is not SIR0",
        ));
    }

    let sir0 = sir0::Sir0::from_bytes(&decompressed)?;
    if sir0.data_pointer as usize >= sir0.content.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SIR0 data pointer out of bounds",
        ));
    }

    Ok(parse_screen_effect(&sir0.content, sir0.data_pointer)?)
}
