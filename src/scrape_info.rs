//! Version stamp and completion marker for a scraper run. Written as the final step of a successful run. 
//! Its presence is what tells the client the output directory is current and complete.

use std::{
    fs, io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

/// Bump ONLY when scraper output format or content changes.
/// Each bump costs a new rescrape.
pub const SCRAPER_VERSION: u32 = 1;

#[derive(Serialize)]
pub struct ScrapeInfo {
    pub scraper_version: u32,
    pub num_pokemon: u32,
    pub rom_id: String,
    pub scraped_at_unix: u64,
    /// Reserved for per-phase invalidation. Stays empty for now.
    pub phases: serde_json::Value,
}

impl ScrapeInfo {
    pub fn new(num_pokemon: u32, rom_id: &str) -> Self {
        ScrapeInfo {
            scraper_version: SCRAPER_VERSION,
            num_pokemon,
            rom_id: rom_id.to_string(),
            scraped_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            phases: serde_json::json!({}),
        }
    }
}

/// tmp + rename so a reader never sees a partial file and mistakes it for a
/// complete run. Never call this on an error path.
pub fn write(output_dir: &Path, info: &ScrapeInfo) -> io::Result<()> {
    let path = output_dir.join("scrape_info.json");
    let tmp = path.with_extension("json.tmp");

    let json = serde_json::to_string_pretty(info)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    fs::write(&tmp, json)?;
    fs::rename(&tmp, &path)
}
