/// The scraper's phase vocabulary. This list defines the run order and is the
/// only source of truth for the strings written to `progress.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseId {
    AnimationInfo,
    TilesetProperties,
    MoveData,
    PokemonSprite,
    PortraitAtlas,
    MoveEffectSprites,
    DungeonTileset,
    DungeonExtras,
    ItemData,
    StatusIcons,
    WeatherManifest,
}

impl PhaseId {
    pub const ALL: &'static [PhaseId] = &[
        PhaseId::AnimationInfo,
        PhaseId::TilesetProperties,
        PhaseId::MoveData,
        PhaseId::PokemonSprite,
        PhaseId::PortraitAtlas,
        PhaseId::MoveEffectSprites,
        PhaseId::DungeonTileset,
        PhaseId::DungeonExtras,
        PhaseId::ItemData,
        PhaseId::StatusIcons,
        PhaseId::WeatherManifest,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            PhaseId::AnimationInfo => "animation_info",
            PhaseId::TilesetProperties => "tileset_properties",
            PhaseId::MoveData => "move_data",
            PhaseId::PokemonSprite => "pokemon_sprite",
            PhaseId::PortraitAtlas => "portrait_atlas",
            PhaseId::MoveEffectSprites => "move_effect_sprites",
            PhaseId::DungeonTileset => "dungeon_tileset",
            PhaseId::DungeonExtras => "dungeon_extras",
            PhaseId::ItemData => "item_data",
            PhaseId::StatusIcons => "status_icons",
            PhaseId::WeatherManifest => "weather_manifest",
        }
    }

    /// Unused until `--only` / `--skip` land.
    pub fn from_str(s: &str) -> Option<PhaseId> {
        PhaseId::ALL.iter().copied().find(|p| p.as_str() == s)
    }
}
