//! The document's external CSS: which `<link rel=stylesheet>` applies at
//! all, fetching the ones that do, and flattening their `@import` chains.
//!
//! The media gate and the two [`lumen_css_parser::MediaContext`] builders live
//! here rather than next to the cascade because they answer a question about
//! the `<link>` element, not about a rule: `collect_link_hrefs` drops a sheet
//! whose `media` does not match before anything is fetched, and the print
//! pipeline swaps in `print_media_context` to make the same gate answer
//! differently (BUG-268 / BUG-270).
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3d); behaviour and
//! signatures are unchanged.

use crate::*;

/// BUG-268: media-гейт для `<link rel=stylesheet media=...>` (HTML LS §4.2.4).
///
/// Отсутствующий/пустой атрибут = «all» → лист применяется. Иначе строка
/// парсится штатным media-query-парсером lumen-css-parser и матчится против
/// переданного контекста — второй матчер не пишем. `ctx` передаётся
/// параметром (а не хардкодится «screen»), чтобы print-пайплайн мог
/// использовать тот же гейт с `media_type: "print"`, когда каскад научится
/// print-контексту (см. BUGS.md BUG-270).
pub(crate) fn link_media_matches(media: &str, ctx: &lumen_css_parser::MediaContext) -> bool {
    let media = media.trim();
    if media.is_empty() {
        return true;
    }
    lumen_css_parser::parse_media_query(media).matches(ctx)
}

/// Экранный `MediaContext` для media-гейта `<link>`: те же media_type /
/// размеры / prefers-color-scheme, что каскад строит внутри layout
/// (`media_context_from_viewport`, layout/src/style.rs) — гейт на `<link>`
/// и фильтр `@media`-блоков должны решать одинаково.
pub(crate) fn screen_media_context(viewport: Size, dark_mode: bool) -> lumen_css_parser::MediaContext {
    lumen_css_parser::MediaContext {
        media_type: "screen".into(),
        width: viewport.width,
        height: viewport.height,
        prefers_dark: dark_mode,
        ..Default::default()
    }
}

/// Print `MediaContext` для media-гейта `<link>` при генерации PDF (BUG-270):
/// `media_type: "print"`, чтобы `<link rel=stylesheet media=print>` попадали в
/// каскад, а `media=screen` — нет. Каскадный фильтр `@media` внутри layout
/// решает так же через `set_print_media` → `media_context_from_viewport`.
pub(crate) fn print_media_context(viewport: Size, dark_mode: bool) -> lumen_css_parser::MediaContext {
    lumen_css_parser::MediaContext {
        media_type: "print".into(),
        width: viewport.width,
        height: viewport.height,
        prefers_dark: dark_mode,
        ..Default::default()
    }
}

/// Загрузить все `<link rel=stylesheet>` документа и склеить их текст.
///
/// Второй элемент результата — исход по каждому элементу (`узел`, `получен
/// ли лист`) в порядке объявления, для BUG-804: `load`/`error` принадлежат
/// элементу `<link>`, а знает исход только этот проход. Раньше провал просто
/// логировался, и страница не могла отличить загруженный лист от 404.
pub(crate) fn load_linked_stylesheets(doc: &Document, base: &ResourceBase, sink: &Arc<dyn EventSink>, cookie_jar: Option<Arc<lumen_storage::CookieJar>>, media_ctx: &lumen_css_parser::MediaContext) -> (String, Vec<(NodeId, bool)>) {
    let mut hrefs = Vec::new();
    collect_link_hrefs(doc, doc.root(), &mut hrefs, media_ctx);

    // Загружаем все таблицы параллельно (сеть — главный тормоз), затем
    // конкатенируем строго в порядке объявления, чтобы каскад не нарушился.
    // Каждый лист резолвит собственные `@import` относительно СВОЕГО URL
    // (`sheet_base`), чтобы вложенные импорты (`<link href="/css/a.css">` →
    // `@import "b.css"` = `/css/b.css`) разрешались корректно.
    let parts = parallel_map(&hrefs, |_, (_, href)| {
        let (text, sheet_base) = fetch_stylesheet_text(href, base, sink, cookie_jar.clone())?;
        Some(inline_css_imports(
            &text,
            &sheet_base,
            sink,
            cookie_jar.clone(),
            media_ctx,
            &mut std::collections::HashSet::new(),
            0,
        ))
    });

    let mut css = String::new();
    let mut outcomes = Vec::with_capacity(parts.len());
    for ((node, _), part) in hrefs.iter().zip(parts) {
        outcomes.push((*node, part.is_some()));
        if let Some(part) = part {
            css.push_str(&part);
            css.push('\n');
        }
    }
    (css, outcomes)
}

/// Загружает текст одной таблицы стилей, разрешённой относительно `base`.
///
/// Обрабатывает локальные пути (`file://`/относительные — читаются с диска)
/// и `http(s)` (через prefetch-кэш, как `<link rel=stylesheet>`). Возвращает
/// текст листа **и** его разрешённый [`ResourceBase`], чтобы вложенные
/// `@import` резолвились относительно собственного URL листа, а не документа.
/// При любой ошибке resolve/чтения/сети — `None` (залогировано), поэтому один
/// битый `@import`/`<link>` не валит весь рендер.
fn fetch_stylesheet_text(
    href: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Option<(String, ResourceBase)> {
    match base.resolve(href) {
        ResolvedResource::File(path) => match std::fs::read_to_string(&path) {
            Ok(content) => {
                eprintln!("Загружен CSS: {}", path.display());
                Some((content, ResourceBase::File(path)))
            }
            Err(e) => {
                eprintln!("Пропуск CSS {}: {e}", path.display());
                None
            }
        },
        ResolvedResource::Url(url) => {
            use lumen_core::url::Url;
            use lumen_network::RequestDestination;

            let sub_url = match Url::parse(&url) {
                Ok(u) => u,
                Err(e) => { eprintln!("Пропуск CSS {url}: {e}"); return None; }
            };

            // Cross-origin stylesheets are allowed by the web platform:
            // `<link rel=stylesheet>` is fetched in no-cors mode and the
            // resulting styles apply normally (Fetch §request, HTML §link).
            // CORS only gates script-level CSSOM reads (cssRules), not the
            // visual application — so we fetch cross-origin CSS like any
            // browser. Real sites host CSS on CDN subdomains (icdn.*,
            // static.*); blocking them left pages unstyled.

            // BUG-171: read through the prefetch cache — the streaming thread
            // warms linked stylesheets with this same client, so the cascade
            // concatenation here reuses identical bytes without a second fetch.
            // PERF-1: one span per stylesheet fetch.
            let mut fetch_span = lumen_core::trace::span(format!("css {url}"), "net");
            let bytes = crate::prefetch::PREFETCH_CACHE.fetch_current(&url, || {
                let client = base.http_client_for_subresource(sink.clone(), cookie_jar.clone());
                client
                    .fetch_subresource(&sub_url, RequestDestination::Style)
                    .map_err(|e| e.to_string())
            });
            match bytes {
                Ok(bytes) => {
                    fetch_span.set_bytes(bytes.len());
                    Some((
                        String::from_utf8_lossy(&bytes[..]).into_owned(),
                        ResourceBase::Url(url),
                    ))
                }
                Err(e) => { eprintln!("Пропуск CSS {url}: {e}"); None }
            }
        }
    }
}

/// Максимальная глубина вложенности `@import` (защита от рекурсии/циклов).
const MAX_CSS_IMPORT_DEPTH: u32 = 16;

/// Рекурсивно резолвит `@import`-правила в `css_text`, возвращая текст с
/// **предпосланным** содержимым каждой импортированной таблицы.
///
/// Per CSS Cascade L4 §6.5: правила импортированного листа предшествуют
/// собственным правилам импортирующего листа (импорт «раньше» в порядке
/// каскада). URL резолвятся относительно `base` (расположения самого листа —
/// см. [`fetch_stylesheet_text`]), поэтому вложенные импорты корректны.
/// Импорты, чей media-query не матчит `media_ctx` (Media Queries L4), не
/// загружаются вовсе — их правила всё равно неприменимы. `seen` хранит уже
/// разрешённые URL и защищает от циклов (`a → b → a`) и повторной загрузки;
/// `depth` ограничивает глубину вложенности.
///
/// Директивы `@import …;` остаются в исходном тексте — парсер каскада
/// собирает их в `Stylesheet::imports` и игнорирует (повторной загрузки нет),
/// так что двойного применения не происходит.
pub(crate) fn inline_css_imports(
    css_text: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    media_ctx: &lumen_css_parser::MediaContext,
    seen: &mut std::collections::HashSet<String>,
    depth: u32,
) -> String {
    // Быстрый путь: нет токена `@import` вовсе → лишний парс не нужен
    // (подавляющее большинство листов). Ложные срабатывания (например
    // `@import` внутри комментария) безопасны — последующий парс правильно
    // не найдёт импорта и вернёт текст как есть.
    if !contains_ignore_ascii_case(css_text.as_bytes(), b"@import") {
        return css_text.to_owned();
    }
    let parsed = lumen_css_parser::parse(css_text);
    if parsed.imports.is_empty() {
        return css_text.to_owned();
    }
    if depth >= MAX_CSS_IMPORT_DEPTH {
        eprintln!("Пропуск @import: превышена глубина вложенности ({MAX_CSS_IMPORT_DEPTH})");
        return css_text.to_owned();
    }

    let mut prefix = String::new();
    for imp in &parsed.imports {
        // Media Queries L4: не матчащий контекст импорт не применяется.
        if !imp.media.matches(media_ctx) {
            continue;
        }
        // Цикл/дубликат: ключ = абсолютный резолв URL относительно текущего листа.
        let key = base.resolve_str(&imp.url);
        if !seen.insert(key) {
            continue;
        }
        let Some((text, imp_base)) =
            fetch_stylesheet_text(&imp.url, base, sink, cookie_jar.clone())
        else {
            continue;
        };
        let resolved = inline_css_imports(
            &text,
            &imp_base,
            sink,
            cookie_jar.clone(),
            media_ctx,
            seen,
            depth + 1,
        );
        prefix.push_str(&resolved);
        if !prefix.ends_with('\n') {
            prefix.push('\n');
        }
    }

    if prefix.is_empty() {
        return css_text.to_owned();
    }
    prefix.push_str(css_text);
    prefix
}

/// ASCII-case-insensitive поиск подстроки `needle` в `haystack` без аллокаций.
pub(crate) fn contains_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return needle.is_empty();
    }
    haystack
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

/// Собрать `(узел, href)` каждого `<link rel=stylesheet>`, который попадёт в
/// каскад.
///
/// Узел нужен BUG-804: по нему [`load_linked_stylesheets`] потом сообщает
/// JS-стороне исход загрузки, чтобы элемент выстрелил `load`/`error`. Раньше
/// собирались одни адреса, и связи «этот лист — этот элемент» не существовало.
pub(crate) fn collect_link_hrefs(doc: &Document, id: NodeId, out: &mut Vec<(NodeId, String)>, media_ctx: &lumen_css_parser::MediaContext) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "link"
    {
        let rel = attrs
            .iter()
            .find(|a| a.name.local == "rel")
            .map(|a| a.value.as_str())
            .unwrap_or("");
        let href = attrs
            .iter()
            .find(|a| a.name.local == "href")
            .map(|a| a.value.as_str())
            .unwrap_or("");
        // BUG-268: print-only (и вообще не матчащие контекст) листы не
        // вливаются в каскад — их правила не обёрнуты в `@media`, каскад
        // сам их не отфильтрует.
        let media = attrs
            .iter()
            .find(|a| a.name.local == "media")
            .map(|a| a.value.as_str())
            .unwrap_or("");
        if rel.split_ascii_whitespace().any(|r| r.eq_ignore_ascii_case("stylesheet"))
            && !href.is_empty()
            && link_media_matches(media, media_ctx)
        {
            out.push((id, href.to_owned()));
        }
        return;
    }
    for &child in &node.children {
        collect_link_hrefs(doc, child, out, media_ctx);
    }
}
