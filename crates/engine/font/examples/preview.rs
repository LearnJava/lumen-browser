//! Печатает несколько глифов из bundled Inter-Regular.ttf как ASCII-art.
//! Запуск: `cargo run --example preview -p lumen-font`.

use std::path::PathBuf;

use lumen_font::{Bitmap, Font, Rasterizer};

fn main() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("assets")
        .join("fonts")
        .join("Inter-Regular.ttf");
    let data = std::fs::read(&path).expect("read Inter-Regular.ttf");
    let font = Font::parse(&data).expect("parse Inter");
    let head = font.head().unwrap();
    let cmap = font.cmap().unwrap();
    let raster = Rasterizer::new(24.0, head.units_per_em);

    println!(
        "Inter-Regular: units_per_em={}, num_glyphs={}, pixel_size=24",
        head.units_per_em,
        font.maxp().unwrap().num_glyphs,
    );

    for ch in ['A', 'M', 'g', 'А', 'Я', 'ж', 'п', '?'] {
        println!("\n--- '{}' (U+{:04X}) ---", ch, ch as u32);
        let Some(gid) = cmap.glyph_index(ch as u32) else {
            println!("(no mapping)");
            continue;
        };
        let glyph = match font.glyph_resolved(gid) {
            Ok(Some(g)) => g,
            Ok(None) => {
                println!("(empty outline)");
                continue;
            }
            Err(err) => {
                println!("(parse error: {err})");
                continue;
            }
        };
        let Some(bitmap) = raster.rasterize(&glyph) else {
            println!("(не удалось растеризовать)");
            continue;
        };
        print_ascii(&bitmap);
    }
}

fn print_ascii(bitmap: &Bitmap) {
    const SHADES: &[char] = &[' ', '.', ':', '-', '=', '+', '*', '#', '@'];
    let max = (SHADES.len() - 1) as u32;
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let v = bitmap.pixels[(y * bitmap.width + x) as usize] as u32;
            // Округляем к ближайшему уровню затенения.
            let idx = (v * max + 127) / 255;
            print!("{}", SHADES[idx as usize]);
        }
        println!();
    }
}
