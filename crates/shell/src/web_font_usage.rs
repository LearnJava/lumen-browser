//! Ленивая загрузка поднаборов `@font-face` по `unicode-range` (CSS Fonts 4 §4.5, BUG-1634).
//!
//! Google Fonts отдаёт на каждое начертание десятки блоков `@font-face` с разными
//! `unicode-range` (latin, cyrillic, vietnamese…). Браузер скачивает поднабор, только
//! если на странице есть символ из его диапазона. Здесь — две чистые половины этого
//! решения: какие кодпоинты показывает документ и пересекаются ли они с диапазоном.

use crate::subresources::PendingWebFont;
use lumen_dom::{Document, NodeData, NodeId};

/// Теги, чей текст не рисуется: содержимое `<script>`/`<style>` (на YouTube — мегабайты
/// не-латиницы) и `<head>` (`<title>` на странице не виден) не должно «заказывать» шрифты.
fn is_invisible_text_container(local: &str) -> bool {
    matches!(local, "head" | "script" | "style" | "noscript" | "template" | "title")
}

/// Атрибуты, значения которых рисуются как текст (`<input value>`, `placeholder`).
const TEXT_ATTRS: [&str; 2] = ["value", "placeholder"];

/// Отсортированный набор кодпоинтов видимого текста документа (текстовые узлы,
/// включая теневые деревья, и значения `value`/`placeholder`). Обход итеративный:
/// глубина DOM не ограничена.
pub(crate) fn document_codepoints(doc: &Document) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut stack: Vec<NodeId> = vec![doc.root()];
    while let Some(id) = stack.pop() {
        let node = doc.get(id);
        match &node.data {
            NodeData::Text(s) => out.extend(s.chars().map(u32::from)),
            NodeData::Element { name, .. } => {
                if is_invisible_text_container(&name.local) {
                    continue;
                }
                for attr in TEXT_ATTRS {
                    if let Some(v) = node.get_attr(attr) {
                        out.extend(v.chars().map(u32::from));
                    }
                }
                if let Some(shadow) = doc.shadow_root_of(id) {
                    stack.push(shadow);
                }
            }
            _ => {}
        }
        stack.extend(node.children.iter().copied());
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Нужен ли блоку `@font-face` с таким `unicode-range` файл: хотя бы один кодпоинт
/// `codepoints` (отсортированный) попадает в диапазон. Без дескриптора (или с
/// непарсящимся) блок покрывает всё и грузится сразу — как до BUG-1634.
pub(crate) fn face_needed(unicode_range: Option<&str>, codepoints: &[u32]) -> bool {
    let Some(raw) = unicode_range else { return true };
    let ranges = lumen_font::parse_unicode_ranges(raw);
    if ranges.is_empty() {
        return true;
    }
    ranges.iter().any(|r| {
        let i = codepoints.partition_point(|&c| c < r.start);
        codepoints.get(i).is_some_and(|&c| c <= r.end)
    })
}

/// BUG-1650: один сетевой запрос на URL шрифта. Google Fonts отдаёт вариативный
/// Roboto четырьмя блоками `@font-face` (wght 300/400/500/700) на один и тот же
/// файл; каждый блок остаётся отдельным дескриптором (свой вес), но байты качаются
/// и декодируются один раз. Потоки загрузки приходят за результатом через
/// [`OnceLock`]: первый грузит, остальные ждут его же итог (в том числе неудачу —
/// повторять упавший запрос на каждый вес незачем).
#[derive(Clone, Default)]
pub(crate) struct FontUrlFetches(
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<FontFetchCell>>>>,
);

type FontFetchCell = std::sync::OnceLock<Option<std::sync::Arc<Vec<u8>>>>;

impl FontUrlFetches {
    /// Байты шрифта по `url`: `fetch` вызывается не более одного раза на URL
    /// (до [`Self::clear`]); параллельные вызовы с тем же URL ждут его результата.
    pub(crate) fn get_or_fetch(
        &self,
        url: &str,
        fetch: impl FnOnce() -> Option<Vec<u8>>,
    ) -> Option<std::sync::Arc<Vec<u8>>> {
        let cell = {
            let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
            std::sync::Arc::clone(map.entry(url.to_owned()).or_default())
        };
        cell.get_or_init(|| fetch().map(std::sync::Arc::new)).clone()
    }

    /// Навигация / смена вкладки: прежние результаты принадлежали другой странице.
    pub(crate) fn clear(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

/// Делит блоки на «нужны странице сейчас» и «отложены» (порядок внутри групп сохраняется).
pub(crate) fn partition_by_usage(
    all: Vec<PendingWebFont>,
    codepoints: &[u32],
) -> (Vec<PendingWebFont>, Vec<PendingWebFont>) {
    all.into_iter().partition(|pf| face_needed(pf.unicode_range_str.as_deref(), codepoints))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(url: &str, range: Option<&str>) -> PendingWebFont {
        PendingWebFont {
            family: "Roboto".into(),
            weight: 400,
            style: lumen_core::FontStyle::Normal,
            unicode_range_str: range.map(str::to_owned),
            ascent_override_str: None,
            descent_override_str: None,
            size_adjust_str: None,
            line_gap_override_str: None,
            variation_settings_str: None,
            url: url.into(),
        }
    }

    fn urls(v: &[PendingWebFont]) -> Vec<&str> {
        v.iter().map(|p| p.url.as_str()).collect()
    }

    #[test]
    fn google_fonts_page_with_latin_text_loads_only_latin_subset() {
        let faces = vec![
            pending("cyr.woff2", Some(CYRILLIC)),
            pending("lat.woff2", Some(LATIN)),
            pending("vie.woff2", Some(VIETNAMESE)),
            pending("all.woff2", None),
        ];
        let (needed, deferred) = partition_by_usage(faces, &cps("Hello"));
        assert_eq!(urls(&needed), ["lat.woff2", "all.woff2"]);
        assert_eq!(urls(&deferred), ["cyr.woff2", "vie.woff2"]);
        // Текст изменился: кириллица появилась — отложенный поднабор уходит в загрузку.
        let (needed, deferred) = partition_by_usage(deferred, &cps("Hello Привет"));
        assert_eq!(urls(&needed), ["cyr.woff2"]);
        assert_eq!(urls(&deferred), ["vie.woff2"]);
    }

    const LATIN: &str = "U+0000-00FF, U+0131, U+0152-0153";
    const CYRILLIC: &str = "U+0301, U+0400-045F, U+0490-0491";
    const VIETNAMESE: &str = "U+0102-0103, U+1EA0-1EF9, U+20AB";

    fn cps(s: &str) -> Vec<u32> {
        let mut v: Vec<u32> = s.chars().map(u32::from).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    #[test]
    fn latin_text_needs_only_latin_subset() {
        let c = cps("Hello, world");
        assert!(face_needed(Some(LATIN), &c));
        assert!(!face_needed(Some(CYRILLIC), &c));
        assert!(!face_needed(Some(VIETNAMESE), &c));
    }

    #[test]
    fn cyrillic_character_pulls_cyrillic_subset() {
        let c = cps("Hello Привет");
        assert!(face_needed(Some(LATIN), &c));
        assert!(face_needed(Some(CYRILLIC), &c));
    }

    #[test]
    fn single_codepoint_range_matches() {
        assert!(!face_needed(Some(VIETNAMESE), &cps("price 5 \u{20AA}")));
        assert!(face_needed(Some(VIETNAMESE), &cps("cost \u{20AB}")));
    }

    #[test]
    fn missing_or_broken_descriptor_means_always_needed() {
        assert!(face_needed(None, &[]));
        assert!(face_needed(Some("garbage"), &cps("abc")));
    }

    #[test]
    fn empty_document_needs_no_ranged_subset() {
        assert!(!face_needed(Some(LATIN), &[]));
    }

    #[test]
    fn document_codepoints_skip_script_style_and_head() {
        let doc = lumen_html_parser::parse(
            "<html><head><title>Заголовок</title><style>.a{content:'щ'}</style></head>\
             <body><p>ab</p><script>var s='ю';</script><input placeholder='ж'></body></html>",
        );
        let c = document_codepoints(&doc);
        assert!(c.contains(&u32::from('a')));
        assert!(c.contains(&u32::from('ж')));
        assert!(!c.contains(&u32::from('щ')));
        assert!(!c.contains(&u32::from('ю')));
        assert!(!c.contains(&u32::from('З')));
    }

    #[test]
    fn concurrent_blocks_of_one_url_fetch_once() {
        let fetches = FontUrlFetches::default();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    let got = fetches.get_or_fetch("https://f.test/roboto.woff2", || {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(30));
                        Some(vec![1, 2, 3])
                    });
                    assert_eq!(got.as_deref().map(Vec::as_slice), Some(&[1u8, 2, 3][..]));
                });
            }
        });
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Другой URL — отдельный запрос; после `clear` тот же URL качается заново.
        assert!(fetches.get_or_fetch("https://f.test/other.woff2", || None).is_none());
        fetches.clear();
        let again = fetches.get_or_fetch("https://f.test/roboto.woff2", || Some(vec![9]));
        assert_eq!(again.as_deref().map(Vec::as_slice), Some(&[9u8][..]));
    }

    /// BUG-1650: четыре `@font-face` (веса 300/400/500/700) на один `src` —
    /// ровно один HTTP-запрос.
    #[test]
    fn four_weights_of_one_file_make_one_http_request() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hits_srv = std::sync::Arc::clone(&hits);
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut c) = conn else { break };
                hits_srv.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = c.read(&mut buf);
                // Не sfnt: файл отбрасывается после загрузки, но запрос уже учтён.
                let _ = c.write_all(
                    b"HTTP/1.1 200 OK
Content-Type: font/woff2
Content-Length: 4
Connection: close

xxxx",
                );
            }
        });
        let css: String = [300, 400, 500, 700]
            .iter()
            .map(|w| format!("@font-face {{ font-family: 'R'; font-weight: {w}; src: url(r.woff2); }}"))
            .collect();
        let sheet = lumen_css_parser::parse(&css);
        struct NullSink;
        impl lumen_core::ext::EventSink for NullSink {
            fn emit(&self, _event: &lumen_core::event::Event) {}
        }
        let base = crate::ResourceBase::Url(format!("http://127.0.0.1:{port}/index.html"));
        let sink: std::sync::Arc<dyn lumen_core::ext::EventSink> = std::sync::Arc::new(NullSink);
        let (_registry, pending) = crate::subresources::load_font_faces(&sheet.font_faces, &base, &sink, None);
        assert_eq!(pending.len(), 4);
        let _ = crate::frames::fetch_web_fonts_blocking(
            pending, &base, &sink, None, None, None,
            lumen_network::ReferrerPolicy::default_policy(), None,
        );
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
