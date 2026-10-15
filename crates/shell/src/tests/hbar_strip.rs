//! UX-TAB-CLOSE-FREEZE: раскладка горизонтальной полосы вкладок на настоящем
//! chrome-документе — равная ширина, × внутри вкладки, замороженная ширина.

use super::*;

/// Раскладывает `#hbarTabs` с `n` вкладками и возвращает прямоугольники
/// `.hbar-tab` и их `.tab-close` (в порядке вкладок).
fn hbar_rects(n: usize, frozen: Option<f32>, viewport_w: f32) -> Vec<(Rect, Option<Rect>)> {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let mut model = cc12_bench_model("");
    model.tabs = (0..n)
        .map(|i| lumen_chrome::ChromeTabModel {
            id: i,
            title: format!("Вкладка с длинным заголовком номер {i}"),
            active: i == 0,
            sleeping: false,
            loading: false,
            is_child: false,
            tree: None,
            container_color: None,
            group: None,
        })
        .collect();
    model.hbar_tab_width = frozen;
    lumen_chrome::bind_model(&mut doc, &model);
    let layout = lumen_layout::layout_measured_hyp(
        &doc,
        &sheet,
        Size::new(viewport_w, 800.0),
        &measurer,
        &KnuthLiangHyphenation::new(),
        false,
    );
    let strip = doc.find_by_id(lumen_chrome::ids::HBAR_TABS).expect("asset has #hbarTabs");
    doc.get(strip)
        .children
        .iter()
        .copied()
        .filter(|&c| doc.get(c).get_attr("class").is_some_and(|cl| cl.split_whitespace().any(|t| t == "hbar-tab")))
        .map(|row| {
            let close = doc
                .get(row)
                .children
                .iter()
                .copied()
                .find(|&c| doc.get(c).get_attr("class") == Some("tab-close"))
                .expect("hbar tab has a close button");
            let rect = |nid| lumen_layout::find_box_by_node(&layout, nid).map(|b| b.rect);
            (rect(row).expect("tab has a box"), rect(close))
        })
        .collect()
}

#[test]
fn hbar_tabs_share_the_strip_equally_and_stay_within_limits() {
    let rects = hbar_rects(5, None, 1280.0);
    assert_eq!(rects.len(), 5);
    let w0 = rects[0].0.width;
    for (tab, _) in &rects {
        assert!((tab.width - w0).abs() < 0.5, "equal widths expected, got {} vs {w0}", tab.width);
        assert!((72.0..=200.0).contains(&tab.width), "tab width {} outside 72..=200", tab.width);
    }
}

#[test]
fn hbar_active_tab_close_button_sits_inside_the_tab() {
    let rects = hbar_rects(5, None, 1280.0);
    let (tab, close) = rects[0];
    let close = close.expect("active tab shows its ×");
    assert!(close.width >= 16.0 && close.height >= 16.0, "× hit zone {}x{} < 16x16", close.width, close.height);
    assert!(close.x >= tab.x && close.right() <= tab.right() + 0.5, "× {close:?} leaves the tab {tab:?}");
}

#[test]
fn hbar_frozen_width_pins_every_tab() {
    let rects = hbar_rects(4, Some(150.0), 1280.0);
    assert_eq!(rects.len(), 4);
    for (tab, _) in &rects {
        assert!((tab.width - 150.0).abs() < 0.5, "frozen width 150 expected, got {}", tab.width);
    }
}
