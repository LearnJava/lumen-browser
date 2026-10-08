//! BUG-1273: однократный headless-снимок дожидается `@font-face url()`.

use super::*;
use super::page_resources::null_sink;

#[test]
fn screenshot_waits_for_url_web_font() {
    let dir = std::env::temp_dir().join(format!("lumen-bug1273-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ahem = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/wpt/fonts/Ahem.ttf");
    std::fs::copy(&ahem, dir.join("Ahem.ttf")).unwrap();
    let page = dir.join("t.html");
    std::fs::write(
        &page,
        "<style>@font-face{font-family:AH;src:url(Ahem.ttf)}body{margin:0}\
         div{font:50px/1 AH;color:#000}</style><div>XXXX</div>",
    )
    .unwrap();
    let (png, w, _h) = render_source_to_png(&PageSource::File(page), null_sink(), None).unwrap();
    let img = lumen_image::decode_png(&png).unwrap();
    let dark = (0..w.min(300))
        .flat_map(|x| (0..60).map(move |y| (x, y)))
        .filter(|&(x, y)| img.data[((y * img.width + x) * 4) as usize] < 64)
        .count();
    let _ = std::fs::remove_dir_all(&dir);
    // Ahem: `X` — сплошной квадрат em×em, четыре буквы = 200×50 px.
    assert_eq!(dark, 200 * 50, "Ahem не применён к снимку");
}
