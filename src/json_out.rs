//! Single JSON writer for every manifest the scraper emits.
//! Formatting is chosen once from the CLI; call sites don't know or care.

use std::{
    fs::{self, File},
    io::{self, BufWriter},
    path::Path,
    sync::OnceLock,
};

use serde::Serialize;

static PRETTY: OnceLock<bool> = OnceLock::new();

/// Called once from `main`, before any phase runs.
pub fn set_pretty(pretty: bool) {
    let _ = PRETTY.set(pretty);
}

fn is_pretty() -> bool {
    *PRETTY.get().unwrap_or(&false)
}

fn to_io<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}

/// Serialise `value` to `path`, creating parent directories.
pub fn write<T: Serialize + ?Sized>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let writer = BufWriter::new(File::create(path)?);
    if is_pretty() {
        serde_json::to_writer_pretty(writer, value).map_err(to_io)
    } else {
        serde_json::to_writer(writer, value).map_err(to_io)
    }
}
