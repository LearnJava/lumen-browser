#![no_main]

use libfuzzer_sys::fuzz_target;
use lumen_core::url::Url;

fuzz_target!(|data: &str| {
    if let Ok(base) = Url::parse(data) {
        let _ = base.resolve(data);
    }
    let _ = Url::parse(&format!("https://example.com/{data}"));
});
