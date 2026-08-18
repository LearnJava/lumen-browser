#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    let _ = lumen_css_parser::parse(data);
    let _ = lumen_css_parser::parse_inline_style(data);
    let _ = lumen_css_parser::parse_selector_list(data);
});
