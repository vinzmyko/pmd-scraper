use std::{
    fs::{self},
    io::{self},
    path::Path,
};

use crate::{
    graphics::portrait::{AtlasType, KaoFile, create_portrait_atlas}, phases::PhaseId, progress::ProgressReporter, rom::Rom
};

pub struct PortraitExtractor<'a> {
    rom: &'a Rom,
}

impl<'a> PortraitExtractor<'a> {
    pub fn new(rom: &'a Rom) -> Self {
        PortraitExtractor { rom }
    }

    /// Extract portrait atlases from the ROM
    pub fn extract_portrait_atlases(
        &self,
        output_dir: &Path,
        reporter: &mut ProgressReporter,
    ) -> io::Result<()> {
        // Create directories
        fs::create_dir_all(output_dir)?;

        // Get the KAO file data
        let kao_data = self.extract_kao_file()?;

        // Parse the KAO file
        let kao_file = match KaoFile::from_bytes(kao_data) {
            Ok(file) => file,
            Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
        };

        // Generate both atlas types
        reporter.begin(PhaseId::PortraitAtlas, 2);
        self.generate_atlas(&kao_file, AtlasType::Pokedex, output_dir)?;
        reporter.advance();
        self.generate_atlas(&kao_file, AtlasType::Expressions, output_dir)?;
        reporter.advance();

        Ok(())
    }

    // Helper methods
    fn extract_kao_file(&self) -> io::Result<Vec<u8>> {
        let kao_file_id = self
            .rom
            .fnt
            .get_file_id("FONT/kaomado.kao")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "kao.kao not found"))?;

        // Extract KAO file data and convert to Vec<u8> using to_vec()
        self.rom
            .fat
            .get_file_data(kao_file_id as usize, &self.rom.data)
            .map(|data| data.to_vec()) // Convert &[u8] to Vec<u8>
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Failed to extract kao.kao"))
    }

    fn generate_atlas(
        &self,
        kao_file: &KaoFile,
        atlas_type: AtlasType,
        output_dir: &Path,
    ) -> io::Result<()> {
        let type_name = match atlas_type {
            AtlasType::Pokedex => "pokedex",
            AtlasType::Expressions => "expressions",
        };

        let atlas_path = output_dir.join(format!("{}_atlas.png", type_name));

        println!("Generating {} atlas...", type_name);
        match create_portrait_atlas(kao_file, &atlas_type, &atlas_path) {
            Ok(_) => {
                println!(
                    "Successfully created {} atlas at: {}",
                    type_name,
                    atlas_path.display()
                );
                Ok(())
            }
            Err(e) => Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to create {} atlas: {}", type_name, e),
            )),
        }
    }
}
