mod animation_info_extractor;
mod arm9;
mod binary_utils;
mod dungeon_bin_extractor;
mod effect_sprite_extractor;
mod filesystem;
mod item_data_extractor;
mod move_data_extractor;
mod move_effects_index;
mod pokemon_portrait_extractor;
mod pokemon_sprite_extractor;
mod progress;
mod rom;
mod status_icon_extractor;
mod text_utils;
mod weather_manifest;

mod containers;
mod data;
mod dungeon;
mod formats;
mod graphics;

use std::{collections::HashMap, fs, path::PathBuf};

use clap::Parser;

use crate::status_icon_extractor::StatusIconExtractor;

use {
    animation_info_extractor::AnimationInfoExtractor, dungeon_bin_extractor::DungeonBinExtractor,
    effect_sprite_extractor::EffectAssetPipeline, move_data_extractor::MoveDataExtractor,
    pokemon_portrait_extractor::PortraitExtractor,
    pokemon_sprite_extractor::PokemonSpriteExtractor, progress::write_progress, rom::Rom,
};

#[derive(Parser, Debug)]
#[command(name = "pmd_scraper")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(value_name = "ROM_PATH")]
    rom_path: PathBuf,
    #[arg(short, long, value_name = "OUTPUT_DIR", default_value = "./output")]
    output_dir: PathBuf,
    #[arg(long)]
    progress: PathBuf,
    #[arg(long)]
    num_pokemon: Option<u32>,
}

fn main() {
    let cli = Cli::parse();

    if !cli.rom_path.exists() {
        eprintln!("Error: ROM path does not exist: {:?}", cli.rom_path);
        std::process::exit(1);
    }

    if !cli.output_dir.exists() {
        std::fs::create_dir_all(&cli.output_dir).expect("Failed to create output directory");
    }

    let output_dir_sprites = cli.output_dir.join("MONSTER");
    let output_dir_portraits = cli.output_dir.join("PORTRAIT");
    let output_dir_jsons = cli.output_dir.join("DATA");
    let output_dir_pipeline = cli.output_dir;

    for dir in [
        &output_dir_sprites,
        &output_dir_portraits,
        &output_dir_jsons,
        &output_dir_pipeline,
    ] {
        if !dir.exists() {
            fs::create_dir_all(dir).expect("Failed to create output directory");
        }
    }

    match Rom::new(cli.rom_path) {
        Ok(mut rom) => {
            println!("Successfully parsed ROM, no corruption detected");

            let mut animation_info_extractor = AnimationInfoExtractor::new(&mut rom);
            println!("Extracting all animation data...");

            let anim_data_info = animation_info_extractor.parse_and_transform_animation_data();
            let _ = animation_info_extractor
                .save_animation_info_json(&anim_data_info, &output_dir_jsons);

            // Tileset properties (overlay 10): weather_effect / is_water / map_color
            let tileset_properties = match rom.extract_tileset_properties() {
                Ok(props) => {
                    let path = output_dir_jsons.join("tileset_properties.json");
                    if let Err(e) = data::tileset_properties::save_json(&props, &path) {
                        eprintln!("Failed to write tileset_properties.json: {}", e);
                    } else {
                        println!("Wrote {} tileset properties to DATA/", props.len());
                    }
                    Some(props)
                }
                Err(e) => {
                    eprintln!("Failed to extract tileset properties: {}", e);
                    None
                }
            };

            let move_data_extractor = MoveDataExtractor::new(&rom);
            let _ = move_data_extractor.extract_and_save(&output_dir_jsons);

            let effects_map: HashMap<u16, _> = anim_data_info
                .effect_table
                .clone()
                .into_iter()
                .enumerate()
                .map(|(idx, info)| (idx as u16, info))
                .collect();

            let moves_map = anim_data_info.transform_move_data();

            // Includes all pokemon, female versions, different forms
            let mut total_pokemon: usize = 572;
            const EFFECT_SPRITE_NUM: usize = 664;

            if let Some(num) = cli.num_pokemon {
                total_pokemon = num as usize;
            }

            write_progress(&cli.progress, 0, total_pokemon, "pokemon_sprite", "running");
            let sprite_extractor = PokemonSpriteExtractor::new(&rom);
            let _ = sprite_extractor.extract_monster_data(
                cli.num_pokemon,
                &output_dir_sprites,
                &cli.progress,
            );

            write_progress(&cli.progress, 0, 2, "portrait_atlas", "running");
            let portrait_extractor = PortraitExtractor::new(&rom);
            let _ =
                portrait_extractor.extract_portrait_atlases(&output_dir_portraits, &cli.progress);

            write_progress(
                &cli.progress,
                0,
                EFFECT_SPRITE_NUM,
                "move_effect_sprites",
                "running",
            );
            let mut effect_pipeline = EffectAssetPipeline::new(&rom);
            let _ = effect_pipeline.run(
                &effects_map,
                &moves_map,
                &output_dir_pipeline,
                &cli.progress,
                EFFECT_SPRITE_NUM,
            );

            let output_dir_dungeons = output_dir_pipeline.join("DUNGEON").join("tilesets");
            write_progress(&cli.progress, 0, 170, "dungeon_tileset", "running");
            let dungeon_extractor = DungeonBinExtractor::new(&rom);
            let _ = dungeon_extractor.extract_dungeon_tilesets(
                None,
                &output_dir_dungeons,
                &cli.progress,
                tileset_properties.as_deref(),
            );

            if let Err(e) = weather_manifest::build_and_save(&output_dir_pipeline) {
                eprintln!("Failed to write weather manifest: {}", e);
            }

            let output_dir_status_icons = output_dir_pipeline.join("STATUS_ICONS");
            write_progress(&cli.progress, 0, 33, "status_icons", "running");
            let mut status_icon_extractor = StatusIconExtractor::new(&mut rom);
            if let Err(e) = status_icon_extractor.extract(&output_dir_status_icons, &cli.progress) {
                eprintln!("Failed to extract status icons: {}", e);
            }

            write_progress(&cli.progress, 0, 0, "", "complete");
        }
        Err(e) => {
            eprintln!("Failed to read ROM file, possibly corrupted: {}", e);
        }
    }
}
