//! Fixed-grid atlas assembly.
//!
//! Places equally-sized cells into a `cols`-wide grid in row-major order.

use image::RgbaImage;

/// Blits `cells` into a `cols`-wide grid. Every cell must share the same
/// dimensions. Mismatched cells are skipped with a warning.
pub fn build(cells: &[RgbaImage], cols: usize, cell_w: u32, cell_h: u32) -> RgbaImage {
    let cols = cols.max(1);
    let rows = cells.len().div_ceil(cols).max(1);

    let mut atlas = RgbaImage::new(cols as u32 * cell_w, rows as u32 * cell_h);

    for (i, cell) in cells.iter().enumerate() {
        if cell.width() != cell_w || cell.height() != cell_h {
            eprintln!(
                "  Warning: grid cell {} is {}x{}, expected {}x{}; skipping",
                i,
                cell.width(),
                cell.height(),
                cell_w,
                cell_h
            );
            continue;
        }

        let ox = (i % cols) as i64 * cell_w as i64;
        let oy = (i / cols) as i64 * cell_h as i64;
        image::imageops::overlay(&mut atlas, cell, ox, oy);
    }

    atlas
}
