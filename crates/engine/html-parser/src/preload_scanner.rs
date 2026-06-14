//! Preload-сканер для HTML chunks (HTML Living Standard §13.2.6.4.7
//! «Speculative HTML parsing»).
//!
//! Бежит поверх [`crate::tokenizer::Tokenizer`] и эмитит ссылки на
//! subresource-ы, которые shell может **начать загружать ещё ДО** того,
//! как main parser построит DOM. Это даёт parallelism: пока tree-builder
//! доделывает обработку HTML, network-слой уже тянет CSS, картинки,
//! шрифты, скрипты.
//!
//! Что распознаём:
//!
//! * `<link rel="stylesheet" href="...">` — внешний CSS;
//! * `<link rel="preload" href="..." as="...">` — author-явный hint;
//! * `<link rel="preconnect|dns-prefetch" href="...">` — connection-уровневые
//!   подсказки, эмитим как `Preconnect { url }` (тип = только origin, но
//!   мы не парсим URL — это работа shell);
//! * `<script src="...">` — внешний JS (хотя в Phase 0 мы JS не исполняем);
//! * `<img src="...">` и `<img srcset="..." sizes="...">` — растровая графика;
//! * `<source srcset="...">` внутри `<picture>` — растровая графика с
//!   media-conditions (сам media-фильтр не применяем здесь — выбор
//!   правильного кандидата делает picker по факту resize-а; preload
//!   качает все потенциальные candidate-URL для самой нижней ветки,
//!   но в Phase 0 я эмитю только `srcset`-строку, чтобы caller сам
//!   решил, что с ней делать).
//!
//! Что **не** делаем:
//!
//! * Не строим DOM (это работа `tree_builder`).
//! * Не резолвим относительные URL — caller (shell) должен сам прогнать
//!   через `Url::resolve(base)`.
//! * Не парсим внутренности `srcset`/`sizes` — отдаём сырыми строками,
//!   чтобы caller мог либо прокачать через [`crate::srcset::parse_srcset`],
//!   либо просто фетчить кандидатов pessimistically.
//! * Не валидируем URL — пустую строку и whitespace-only `href` мы
//!   silently пропускаем (lenient, как и tokenizer).
//! * Не фильтруем по `media` / `type` на `<source>` — preload должен
//!   быть speculative, поэтому фетчим все candidate URL; финальный
//!   pick зависит от viewport-а в момент layout-а.
//!
//! Случаи, когда у одного тега несколько hints, сейчас не возникают —
//! `<link rel>` может быть multi-token (`rel="preload stylesheet"`), и
//! тогда мы эмитим оба hint-а в порядке `rel`-токенов: тип каждого
//! токена обрабатывается независимо. Дубликаты по URL — на совести
//! caller-а (shell может дедуплицировать через свой fetch-кэш).

use crate::push_tokenizer::PushTokenizer;
use crate::tokenizer::{Token, Tokenizer};

/// Один speculative-fetch hint, извлечённый preload-сканером.
///
/// URL — сырая строка из атрибута, без trim-а (HTML5 `Reflect.IDL` для
/// `href`/`src` сохраняет whitespace, но мы сами trim-аем в самом
/// сканере; caller получает уже trimmed-form). Резолв относительно
/// base URL — работа shell-а.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreloadHint {
    /// `<link rel="stylesheet" href="...">`. Caller должен дополнительно
    /// проверить, что у author-а нет `disabled`-атрибута (для Phase 0
    /// мы это не делаем — disabled-stylesheets редкие).
    Stylesheet { url: String },
    /// `<script src="...">`. Без `type="module"` и атрибутов defer/async —
    /// caller-у достаточно URL.
    Script { url: String },
    /// `<img src="...">` или fallback-fetch одиночного `<img>`. `srcset`
    /// и `sizes` отделены для удобства caller-а; при отсутствии срабатывает
    /// только `src`.
    Image {
        url: Option<String>,
        srcset: Option<String>,
        sizes: Option<String>,
    },
    /// `<source srcset="...">` внутри `<picture>` / `<video>` / `<audio>`.
    /// `media`-атрибут специально не учитываем — речь о preload, fetch
    /// должен быть speculative для всех media-веток. Caller волен
    /// фильтровать post-factum.
    SourceSet { srcset: String, sizes: Option<String> },
    /// `<link rel="preload" href="..." as="...">`. `as_kind` — нормализован
    /// в lower-case (HTML5 §4.6.7 «The link element» — destination keyword).
    /// Caller сам решает, какой fetch-приоритет / Accept-header выставить.
    Preload {
        url: String,
        as_kind: Option<String>,
    },
    /// `<link rel="preconnect">` / `rel="dns-prefetch"`. URL — это origin
    /// (по spec — `href`, обычно `https://cdn.example/`). Caller-у этого
    /// достаточно, чтобы открыть TCP/TLS-сокет / резолвить DNS заранее.
    Preconnect {
        url: String,
        /// `true` если `rel="dns-prefetch"` (легче — только DNS), `false`
        /// для `rel="preconnect"` (полный TCP+TLS handshake).
        dns_only: bool,
    },
}

/// Пробежать по HTML и вернуть все subresource-hint-ы, найденные в
/// start-тегах.
///
/// Не строит DOM, не исполняет скрипты. Использует общий
/// [`Tokenizer`] — поведение RAWTEXT/RCDATA одинаковое с main parser-ом.
/// Это значит, что внутри `<script>`/`<style>` мы не извлекаем hint-ов
/// (там литеральный текст до `</tag` — `<img src=...>` в комментарии
/// `<style>...</style>` не парсится как тег). Это правильно: spec-овский
/// scanner делает то же самое.
///
/// End-теги, текст, комментарии, doctype — игнорируются.
///
/// Для streaming-обработки используйте [`PreloadScanner`].
pub fn scan_preload_hints(input: &str) -> Vec<PreloadHint> {
    let tokens: Vec<Token> = Tokenizer::new(input).collect();
    collect_hints_from_tokens(&tokens)
}

fn collect_link_hints(attrs: &[(String, String)], out: &mut Vec<PreloadHint>) {
    let rel = find_attr(attrs, "rel").map(|v| v.to_ascii_lowercase());
    let href = find_attr(attrs, "href").map(str::trim).filter(|s| !s.is_empty());
    let as_kind = find_attr(attrs, "as").map(|v| v.trim().to_ascii_lowercase());

    let Some(rel) = rel else {
        return;
    };
    let Some(href) = href else {
        return;
    };

    // `rel` может содержать список keywords (`rel="preload stylesheet"`).
    // Каждый эмитится отдельным hint-ом.
    for token in rel.split_ascii_whitespace() {
        match token {
            "stylesheet" => out.push(PreloadHint::Stylesheet {
                url: href.to_string(),
            }),
            "preload" => out.push(PreloadHint::Preload {
                url: href.to_string(),
                as_kind: as_kind
                    .as_ref()
                    .filter(|s| !s.is_empty())
                    .cloned(),
            }),
            "preconnect" => out.push(PreloadHint::Preconnect {
                url: href.to_string(),
                dns_only: false,
            }),
            "dns-prefetch" => out.push(PreloadHint::Preconnect {
                url: href.to_string(),
                dns_only: true,
            }),
            _ => {}
        }
    }
}

fn collect_script_hint(attrs: &[(String, String)], out: &mut Vec<PreloadHint>) {
    if let Some(src) = find_attr(attrs, "src").map(str::trim)
        && !src.is_empty()
    {
        out.push(PreloadHint::Script {
            url: src.to_string(),
        });
    }
}

fn collect_img_hint(attrs: &[(String, String)], out: &mut Vec<PreloadHint>) {
    let src = find_attr(attrs, "src")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let srcset = find_attr(attrs, "srcset")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let sizes = find_attr(attrs, "sizes")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if src.is_none() && srcset.is_none() {
        return;
    }
    out.push(PreloadHint::Image { url: src, srcset, sizes });
}

fn collect_source_hint(attrs: &[(String, String)], out: &mut Vec<PreloadHint>) {
    let Some(srcset) = find_attr(attrs, "srcset")
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    let sizes = find_attr(attrs, "sizes")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    out.push(PreloadHint::SourceSet {
        srcset: srcset.to_string(),
        sizes,
    });
}

/// Инкрементальный preload-сканер (HTML LS §13.2.6.4.7).
///
/// В отличие от [`scan_preload_hints`], которая принимает полную HTML-строку,
/// `PreloadScanner` обрабатывает HTML **chunk-ами** — по мере прихода байт
/// из сети. Hint-ы эмитятся немедленно при обнаружении, ещё до того как
/// tree-builder получил и разобрал полный документ.
///
/// Это даёт реальный выигрыш: stylesheet/шрифты после первых 8 КБ HTML
/// (за пределами окна `STREAM_CHUNK_BYTES`) не теряются — каждый `HtmlChunk`
/// сканируется отдельно, а shell аккумулирует хинты по всем chunk-ам.
///
/// # Использование
///
/// ```rust
/// use lumen_html_parser::preload_scanner::PreloadScanner;
///
/// let mut scanner = PreloadScanner::new();
/// for chunk in html_chunks {
///     for hint in scanner.feed_bytes(chunk) {
///         // ранний fetch
///     }
/// }
/// for hint in scanner.end() {
///     // финальные hint-ы из незавершённого хвоста
/// }
/// ```
pub struct PreloadScanner {
    push: PushTokenizer,
}

impl PreloadScanner {
    /// Создаёт новый инкрементальный сканер.
    pub fn new() -> Self {
        Self { push: PushTokenizer::new() }
    }

    /// Скармливает очередной chunk сырых байт и возвращает все hint-ы,
    /// найденные в полностью разобранных токенах этого chunk-а.
    ///
    /// Незавершённые теги на границе chunk-а автоматически буферизуются
    /// и обрабатываются при следующем `feed_bytes` или `end`.
    pub fn feed_bytes(&mut self, chunk: &[u8]) -> Vec<PreloadHint> {
        let tokens = self.push.feed_bytes(chunk);
        collect_hints_from_tokens(&tokens)
    }

    /// Завершает ввод и возвращает hint-ы из буферизованного хвоста.
    ///
    /// После вызова `end` объект нельзя использовать для дальнейшего `feed_bytes`.
    pub fn end(&mut self) -> Vec<PreloadHint> {
        let tokens = self.push.end();
        collect_hints_from_tokens(&tokens)
    }
}

impl Default for PreloadScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Извлечь hint-ы из готовых токенов (общая логика для `scan_preload_hints`
/// и `PreloadScanner`).
fn collect_hints_from_tokens(tokens: &[Token]) -> Vec<PreloadHint> {
    let mut out = Vec::new();
    for tok in tokens {
        let Token::StartTag { name, attrs, .. } = tok else {
            continue;
        };
        match name.as_str() {
            "link" => collect_link_hints(attrs, &mut out),
            "script" => collect_script_hint(attrs, &mut out),
            "img" => collect_img_hint(attrs, &mut out),
            "source" => collect_source_hint(attrs, &mut out),
            _ => {}
        }
    }
    out
}

/// ASCII case-insensitive lookup атрибута. Имена в нашем tokenizer-е
/// уже lower-case (HTML5 §13.2.5.32), но lookup всё равно делаем
/// case-insensitive для robustness к будущим изменениям токенайзера.
fn find_attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_no_hints() {
        assert!(scan_preload_hints("").is_empty());
        assert!(scan_preload_hints("  \n  ").is_empty());
        assert!(scan_preload_hints("<p>hello</p>").is_empty());
    }

    #[test]
    fn link_stylesheet() {
        let hints = scan_preload_hints(r#"<link rel="stylesheet" href="theme.css">"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Stylesheet {
                url: "theme.css".into()
            }]
        );
    }

    #[test]
    fn link_stylesheet_case_insensitive() {
        // HTML — case-insensitive в tag/attr names; tokenizer уже
        // нормализует в lower-case.
        let hints = scan_preload_hints(r#"<LINK REL="Stylesheet" HREF="theme.css">"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Stylesheet {
                url: "theme.css".into()
            }]
        );
    }

    #[test]
    fn link_preload_with_as() {
        let hints =
            scan_preload_hints(r#"<link rel="preload" href="font.woff2" as="font">"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Preload {
                url: "font.woff2".into(),
                as_kind: Some("font".into()),
            }]
        );
    }

    #[test]
    fn link_preload_without_as() {
        // `as` отсутствует — caller увидит None и сам решит default.
        let hints = scan_preload_hints(r#"<link rel="preload" href="x.bin">"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Preload {
                url: "x.bin".into(),
                as_kind: None,
            }]
        );
    }

    #[test]
    fn link_preconnect_full() {
        let hints =
            scan_preload_hints(r#"<link rel="preconnect" href="https://cdn.example/">"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Preconnect {
                url: "https://cdn.example/".into(),
                dns_only: false,
            }]
        );
    }

    #[test]
    fn link_dns_prefetch() {
        let hints =
            scan_preload_hints(r#"<link rel="dns-prefetch" href="//cdn.example/">"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Preconnect {
                url: "//cdn.example/".into(),
                dns_only: true,
            }]
        );
    }

    #[test]
    fn link_rel_with_multiple_tokens_emits_both() {
        // `rel="preload stylesheet"` — крайний случай: эмитим оба hint-а.
        // Order: как в `rel`-атрибуте.
        let hints = scan_preload_hints(
            r#"<link rel="preload stylesheet" href="hero.css" as="style">"#,
        );
        assert_eq!(
            hints,
            vec![
                PreloadHint::Preload {
                    url: "hero.css".into(),
                    as_kind: Some("style".into()),
                },
                PreloadHint::Stylesheet {
                    url: "hero.css".into(),
                },
            ]
        );
    }

    #[test]
    fn link_unknown_rel_skipped() {
        // `rel="icon"`/`canonical`/`manifest` — не subresource preload.
        let hints = scan_preload_hints(
            r#"<link rel="icon" href="/favicon.ico">
               <link rel="canonical" href="https://example/">"#,
        );
        assert!(hints.is_empty());
    }

    #[test]
    fn link_without_href_skipped() {
        let hints = scan_preload_hints(r#"<link rel="stylesheet">"#);
        assert!(hints.is_empty());
    }

    #[test]
    fn link_without_rel_skipped() {
        let hints = scan_preload_hints(r#"<link href="theme.css">"#);
        assert!(hints.is_empty());
    }

    #[test]
    fn link_empty_href_skipped() {
        let hints = scan_preload_hints(r#"<link rel="stylesheet" href="">"#);
        assert!(hints.is_empty());
        let hints = scan_preload_hints(r#"<link rel="stylesheet" href="   ">"#);
        assert!(hints.is_empty());
    }

    #[test]
    fn script_with_src() {
        let hints = scan_preload_hints(r#"<script src="app.js"></script>"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Script {
                url: "app.js".into()
            }]
        );
    }

    #[test]
    fn inline_script_without_src_skipped() {
        // Inline <script> без src — не subresource. И его тело (RAWTEXT)
        // не должно дать ложных match-ов на `<img>` внутри.
        let hints = scan_preload_hints(
            r#"<script>var s = '<img src="fake.png">'; console.log(s);</script>"#,
        );
        assert!(hints.is_empty());
    }

    #[test]
    fn style_body_does_not_match() {
        // `<style>` тело — RAWTEXT, в нём `<img>` буквальный текст и
        // токенизатор его не парсит как старт-тег.
        let hints = scan_preload_hints(
            r#"<style>body { background: url(real.png); }</style>
               <img src="actual.jpg">"#,
        );
        assert_eq!(
            hints,
            vec![PreloadHint::Image {
                url: Some("actual.jpg".into()),
                srcset: None,
                sizes: None,
            }]
        );
    }

    #[test]
    fn img_with_src() {
        let hints = scan_preload_hints(r#"<img src="cat.png">"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Image {
                url: Some("cat.png".into()),
                srcset: None,
                sizes: None,
            }]
        );
    }

    #[test]
    fn img_with_srcset_and_sizes() {
        let hints = scan_preload_hints(
            r#"<img srcset="s.png 480w, m.png 1024w" sizes="100vw" src="fb.png">"#,
        );
        assert_eq!(
            hints,
            vec![PreloadHint::Image {
                url: Some("fb.png".into()),
                srcset: Some("s.png 480w, m.png 1024w".into()),
                sizes: Some("100vw".into()),
            }]
        );
    }

    #[test]
    fn img_srcset_only_no_src() {
        // `<img srcset="...">` без `src` — валидный кейс, picker сам
        // выберет кандидата.
        let hints = scan_preload_hints(r#"<img srcset="hi.png 2x">"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Image {
                url: None,
                srcset: Some("hi.png 2x".into()),
                sizes: None,
            }]
        );
    }

    #[test]
    fn img_without_src_or_srcset_skipped() {
        let hints = scan_preload_hints(r#"<img alt="no source">"#);
        assert!(hints.is_empty());
    }

    #[test]
    fn source_inside_picture() {
        let hints = scan_preload_hints(
            r#"<picture>
                 <source srcset="hi.webp 1x, hi2.webp 2x" type="image/webp">
                 <img src="hi.jpg">
               </picture>"#,
        );
        assert_eq!(
            hints,
            vec![
                PreloadHint::SourceSet {
                    srcset: "hi.webp 1x, hi2.webp 2x".into(),
                    sizes: None,
                },
                PreloadHint::Image {
                    url: Some("hi.jpg".into()),
                    srcset: None,
                    sizes: None,
                },
            ]
        );
    }

    #[test]
    fn source_without_srcset_skipped() {
        // `<source>` без srcset — невалиден; пропускаем.
        let hints = scan_preload_hints(r#"<source src="x.png">"#);
        assert!(hints.is_empty());
    }

    #[test]
    fn source_media_ignored_for_preload() {
        // Preload-сканер не фильтрует по media — фетчим все candidate URLs
        // pessimistically. Caller / picker post-factum выберет нужный
        // вариант по viewport-у.
        let hints = scan_preload_hints(
            r#"<source media="(min-width: 5000px)" srcset="never.webp">
               <source srcset="always.webp">"#,
        );
        assert_eq!(
            hints,
            vec![
                PreloadHint::SourceSet {
                    srcset: "never.webp".into(),
                    sizes: None,
                },
                PreloadHint::SourceSet {
                    srcset: "always.webp".into(),
                    sizes: None,
                },
            ]
        );
    }

    #[test]
    fn full_page_order_preserved() {
        // Hints должны идти в source-order — это важно для shell-а:
        // первые fetch-и стартуют раньше.
        let hints = scan_preload_hints(
            r#"<!DOCTYPE html>
               <html>
               <head>
                 <link rel="preconnect" href="https://cdn.example/">
                 <link rel="stylesheet" href="reset.css">
                 <link rel="stylesheet" href="theme.css">
                 <script src="lib.js"></script>
               </head>
               <body>
                 <img src="hero.png" width="1200" height="600">
                 <picture>
                   <source srcset="thumb.webp" type="image/webp">
                   <img src="thumb.jpg">
                 </picture>
               </body>
               </html>"#,
        );
        assert_eq!(hints.len(), 7);
        assert!(matches!(hints[0], PreloadHint::Preconnect { .. }));
        assert!(matches!(hints[1], PreloadHint::Stylesheet { .. }));
        assert!(matches!(hints[2], PreloadHint::Stylesheet { .. }));
        assert!(matches!(hints[3], PreloadHint::Script { .. }));
        assert!(matches!(hints[4], PreloadHint::Image { .. }));
        assert!(matches!(hints[5], PreloadHint::SourceSet { .. }));
        assert!(matches!(hints[6], PreloadHint::Image { .. }));
    }

    #[test]
    fn cyrillic_attribute_values_preserved() {
        // Принцип №7: русский — first-class. URL с кириллицей в пути
        // должен дойти до caller-а без mangling-а.
        let hints =
            scan_preload_hints(r#"<link rel="stylesheet" href="/тема.css">"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Stylesheet {
                url: "/тема.css".into()
            }]
        );
    }

    #[test]
    fn href_attribute_trimmed() {
        // HTML5 не trim-ит href, но мы trim-аем — лидирующий/trailing
        // whitespace в URL почти всегда означает author-typo.
        let hints =
            scan_preload_hints(r#"<link rel="stylesheet" href="  theme.css  ">"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Stylesheet {
                url: "theme.css".into()
            }]
        );
    }

    #[test]
    fn self_closing_void_tags_processed() {
        // `<link>` и `<img>` — void elements, парсятся даже без `/>`.
        let hints = scan_preload_hints(r#"<link rel="stylesheet" href="x.css" /><img src="y.png" />"#);
        assert_eq!(hints.len(), 2);
    }

    #[test]
    fn link_preload_empty_as_attr_yields_none() {
        // `as=""` — empty after trim. None, а не Some("").
        let hints =
            scan_preload_hints(r#"<link rel="preload" href="x.bin" as="">"#);
        assert_eq!(
            hints,
            vec![PreloadHint::Preload {
                url: "x.bin".into(),
                as_kind: None,
            }]
        );
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;

    /// Вспомогательная функция: скармливает HTML по одному байту через
    /// `PreloadScanner::feed_bytes`. Результат — все hint-ы в source-order.
    fn scan_byte_by_byte(input: &str) -> Vec<PreloadHint> {
        let mut scanner = PreloadScanner::new();
        let mut out = Vec::new();
        for byte in input.as_bytes() {
            out.extend(scanner.feed_bytes(std::slice::from_ref(byte)));
        }
        out.extend(scanner.end());
        out
    }

    /// Скармливает чанками фиксированного размера.
    fn scan_chunked(input: &str, chunk_size: usize) -> Vec<PreloadHint> {
        let mut scanner = PreloadScanner::new();
        let mut out = Vec::new();
        let bytes = input.as_bytes();
        let mut pos = 0;
        while pos < bytes.len() {
            let end = (pos + chunk_size).min(bytes.len());
            out.extend(scanner.feed_bytes(&bytes[pos..end]));
            pos = end;
        }
        out.extend(scanner.end());
        out
    }

    #[test]
    fn streaming_matches_batch_simple() {
        let html = r#"<link rel="stylesheet" href="theme.css"><img src="hero.png">"#;
        let batch = scan_preload_hints(html);
        let streaming = scan_byte_by_byte(html);
        assert_eq!(batch, streaming);
    }

    #[test]
    fn streaming_matches_batch_full_page() {
        let html = r#"<!DOCTYPE html>
<html><head>
  <link rel="preconnect" href="https://cdn.example/">
  <link rel="stylesheet" href="reset.css">
  <link rel="preload" href="font.woff2" as="font">
  <script src="lib.js"></script>
</head><body>
  <img src="hero.png">
  <picture>
    <source srcset="hi.webp" type="image/webp">
    <img src="hi.jpg">
  </picture>
</body></html>"#;
        let batch = scan_preload_hints(html);
        let streaming_1 = scan_byte_by_byte(html);
        let streaming_8 = scan_chunked(html, 8);
        let streaming_64 = scan_chunked(html, 64);
        assert_eq!(batch, streaming_1, "byte-by-byte != batch");
        assert_eq!(batch, streaming_8, "chunk-8 != batch");
        assert_eq!(batch, streaming_64, "chunk-64 != batch");
    }

    #[test]
    fn stylesheet_tag_split_across_chunks() {
        // Атрибут href разрезан на границе chunk-а — hint всё равно выдаётся корректно.
        let html = r#"<link rel="stylesheet" href="styles.css">"#;
        // Разрезаем посредине `href="sty|les.css"`.
        let split_at = html.find("sty").unwrap() + 3;
        let mut scanner = PreloadScanner::new();
        let mut hints: Vec<PreloadHint> = scanner.feed_bytes(&html.as_bytes()[..split_at]);
        hints.extend(scanner.feed_bytes(&html.as_bytes()[split_at..]));
        hints.extend(scanner.end());
        assert_eq!(
            hints,
            vec![PreloadHint::Stylesheet { url: "styles.css".into() }]
        );
    }

    #[test]
    fn hints_beyond_first_chunk_window() {
        // Stylesheet встречается ПОСЛЕ первых 8192 байт — это именно то,
        // что PH1-8 исправляет: batch-scan первого chunk-а его не видел.
        let padding = "<!-- ".to_string() + &"x".repeat(8200) + " -->";
        let html = format!(r#"{padding}<link rel="stylesheet" href="late.css">"#);
        let batch = scan_preload_hints(&html);
        let streaming = scan_chunked(&html, 8192);
        assert_eq!(batch, streaming);
        assert_eq!(
            streaming,
            vec![PreloadHint::Stylesheet { url: "late.css".into() }]
        );
    }

    #[test]
    fn empty_stream_yields_no_hints() {
        let mut scanner = PreloadScanner::new();
        assert!(scanner.feed_bytes(b"").is_empty());
        assert!(scanner.end().is_empty());
    }

    #[test]
    fn cyrillic_url_preserved_in_streaming() {
        let html = r#"<link rel="stylesheet" href="/тема.css">"#;
        let hints = scan_byte_by_byte(html);
        assert_eq!(
            hints,
            vec![PreloadHint::Stylesheet { url: "/тема.css".into() }]
        );
    }

    #[test]
    fn rawtext_in_streaming_does_not_leak_inner_tags() {
        // `<img>` внутри `<script>` не должен давать hint — токенайзер
        // трактует тело <script> как RAWTEXT.
        let html = r#"<script>var s = '<img src="fake.png">';</script><img src="real.png">"#;
        let hints = scan_chunked(html, 8);
        assert_eq!(
            hints,
            vec![PreloadHint::Image {
                url: Some("real.png".into()),
                srcset: None,
                sizes: None,
            }]
        );
    }

    #[test]
    fn source_order_preserved_across_chunks() {
        // Порядок hint-ов должен совпадать с source-order независимо от chunk-границ.
        let html = r#"<link rel="preconnect" href="https://cdn.example/">
<link rel="stylesheet" href="a.css">
<link rel="stylesheet" href="b.css">
<script src="main.js"></script>"#;
        let batch = scan_preload_hints(html);
        // Проверяем разные chunk-размеры.
        for chunk_size in [1usize, 4, 16, 64, 256] {
            let streaming = scan_chunked(html, chunk_size);
            assert_eq!(batch, streaming, "source-order mismatch at chunk_size={chunk_size}");
        }
    }
}
