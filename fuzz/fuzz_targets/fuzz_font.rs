#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let sfnt = lumen_font::maybe_decode_font(data).ok().flatten();
    let bytes: &[u8] = sfnt.as_deref().unwrap_or(data);
    let _ = lumen_font::Font::parse(bytes);
});
