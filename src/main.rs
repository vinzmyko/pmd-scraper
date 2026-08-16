mod animation_info_extractor;
mod arm9;
mod binary_utils;
mod dungeon_bin_extractor;
mod effect_sprite_extractor;
mod filesystem;
mod item_data_extractor;
mod json_out;
mod move_data_extractor;
mod move_effects_index;
mod phases;
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

use crate::{
    dungeon_bin_extractor::{extract_dungeon_extras, extract_dungeon_tilesets},
    phases::PhaseId,
    progress::ProgressReporter,
    status_icon_extractor::StatusIconExtractor,
};

use {
    animation_info_extractor::AnimationInfoExtractor, dungeon_bin_extractor::open_dungeon_bin,
    effect_sprite_extractor::EffectAssetPipeline, item_data_extractor::ItemDataExtractor,
    move_data_extractor::MoveDataExtractor, pokemon_portrait_extractor::PortraitExtractor,
    pokemon_sprite_extractor::PokemonSpriteExtractor, rom::Rom,
};

#[derive(Parser, Debug)]
#[command(name = "pmd_scraper")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(value_name = "ROM_PATH", required_unless_present = "list_phases")]
    rom_path: Option<PathBuf>,
    #[arg(short, long, value_name = "OUTPUT_DIR", default_value = "./output")]
    output_dir: PathBuf,
    #[arg(long)]
    progress: Option<PathBuf>,
    #[arg(long)]
    num_pokemon: Option<u32>,
    #[arg(long)]
    list_phases: bool,
    #[arg(long)]
    pretty: bool,
    /// Emit item names and descriptions in items.json.
    #[arg(long)]
    with_text: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.list_phases {
        for phase in PhaseId::ALL {
            println!("{}", phase.as_str());
        }
        return;
    }

    json_out::set_pretty(cli.pretty);

    // Guaranteed present by `required_unless_present`.
    let rom_path = cli.rom_path.clone().unwrap();

    if !rom_path.exists() {
        eprintln!("Error: ROM path does not exist: {:?}", rom_path);
        std::process::exit(1);
    }

    let output_dir_sprites = cli.output_dir.join("MONSTER");
    let output_dir_portraits = cli.output_dir.join("PORTRAIT");
    let output_dir_jsons = cli.output_dir.join("DATA");
    let output_dir_pipeline = cli.output_dir.clone();

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

    let mut reporter = ProgressReporter::new(cli.progress.clone());

    let mut rom = match Rom::new(rom_path) {
        Ok(rom) => rom,
        Err(e) => {
            eprintln!("Failed to read ROM file, possibly corrupted: {}", e);
            reporter.fail(&format!("Failed to read ROM file: {}", e));
            std::process::exit(1);
        }
    };
    println!("Successfully parsed ROM, no corruption detected");

    // Cross-phase state. Each is produced by one phase and read by later ones.
    let mut anim_data_info = None;
    let mut tileset_properties = None;
    let mut dungeon_bin = None;

    for &phase in PhaseId::ALL {
        match phase {
            PhaseId::AnimationInfo => {
                reporter.begin(phase, 1);
                let mut extractor = AnimationInfoExtractor::new(&mut rom);
                let data = extractor.parse_and_transform_animation_data();
                let _ = extractor.save_animation_info_json(&data, &output_dir_jsons);
                anim_data_info = Some(data);
                reporter.advance();
            }

            PhaseId::TilesetProperties => {
                reporter.begin(phase, 1);
                match rom.extract_tileset_properties() {
                    Ok(props) => {
                        let path = output_dir_jsons.join("tileset_properties.json");
                        if let Err(e) = data::tileset_properties::save_json(&props, &path) {
                            eprintln!("Failed to write tileset_properties.json: {}", e);
                        } else {
                            println!("Wrote {} tileset properties to DATA/", props.len());
                        }
                        tileset_properties = Some(props);
                    }
                    Err(e) => eprintln!("Failed to extract tileset properties: {}", e),
                }
                reporter.advance();
            }

            PhaseId::MoveData => {
                reporter.begin(phase, 1);
                let _ = MoveDataExtractor::new(&rom).extract_and_save(&output_dir_jsons);
                reporter.advance();
            }

            PhaseId::PokemonSprite => {
                let extractor = PokemonSpriteExtractor::new(&rom);
                let _ = extractor.extract_monster_data(
                    cli.num_pokemon,
                    &output_dir_sprites,
                    &mut reporter,
                );
            }

            PhaseId::PortraitAtlas => {
                let extractor = PortraitExtractor::new(&rom);
                let _ = extractor.extract_portrait_atlases(&output_dir_portraits, &mut reporter);
            }

            PhaseId::MoveEffectSprites => match &anim_data_info {
                Some(anim) => {
                    let effects_map: HashMap<u16, _> = anim
                        .effect_table
                        .clone()
                        .into_iter()
                        .enumerate()
                        .map(|(idx, info)| (idx as u16, info))
                        .collect();
                    let moves_map = anim.transform_move_data();

                    let mut pipeline = EffectAssetPipeline::new(&rom);
                    let _ = pipeline.run(
                        &effects_map,
                        &moves_map,
                        &output_dir_pipeline,
                        &mut reporter,
                    );
                }
                None => eprintln!("Skipping {}: animation data unavailable", phase.as_str()),
            },

            PhaseId::DungeonTileset => {
                ensure_dungeon_bin(&rom, &mut dungeon_bin);
                if let Some(binpack) = &dungeon_bin {
                    let output_dir_dungeons = output_dir_pipeline.join("DUNGEON").join("tilesets");
                    let _ = extract_dungeon_tilesets(
                        binpack,
                        None,
                        &output_dir_dungeons,
                        &mut reporter,
                        tileset_properties.as_deref(),
                    );
                }
            }

            PhaseId::DungeonExtras => {
                ensure_dungeon_bin(&rom, &mut dungeon_bin);
                if let Some(binpack) = &dungeon_bin {
                    let dungeon_dir = output_dir_pipeline.join("DUNGEON");
                    let _ = extract_dungeon_extras(binpack, &dungeon_dir, &mut reporter);
                }
            }

            PhaseId::ItemData => {
                ensure_dungeon_bin(&rom, &mut dungeon_bin);
                if let Some(binpack) = &dungeon_bin {
                    reporter.begin(phase, 1);
                    let output_dir_items = output_dir_pipeline.join("ITEMS");
                    let extractor = ItemDataExtractor::new(&rom);
                    if let Err(e) =
                        extractor.extract_and_save(binpack, &output_dir_items, cli.with_text)
                    {
                        eprintln!("Failed to extract item data: {}", e);
                    }
                    reporter.advance();
                }
            }

            PhaseId::StatusIcons => {
                let output_dir_status_icons = output_dir_pipeline.join("STATUS_ICONS");
                let mut extractor = StatusIconExtractor::new(&mut rom);
                if let Err(e) = extractor.extract(&output_dir_status_icons, &mut reporter) {
                    eprintln!("Failed to extract status icons: {}", e);
                }
            }

            PhaseId::WeatherManifest => {
                reporter.begin(phase, 1);
                if let Err(e) = weather_manifest::build_and_save(&output_dir_pipeline) {
                    eprintln!("Failed to write weather manifest: {}", e);
                }
                reporter.advance();
            }
        }
    }

    reporter.complete();
}

/// Opens `dungeon.bin` on first use and keeps it for the phases that follow.
fn ensure_dungeon_bin(rom: &Rom, slot: &mut Option<containers::binpack::BinPack>) {
    if slot.is_some() {
        return;
    }
    match open_dungeon_bin(rom) {
        Ok(pack) => *slot = Some(pack),
        Err(e) => eprintln!("Failed to open dungeon.bin: {}", e),
    }
}
