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
use lumen_network::csp::CspPolicy;
use lumen_network::Origin;

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

/// `doc.character_set()` as an [`lumen_encoding::Encoding`] — the bottom tier
/// of CSS Syntax L3 "determine the fallback encoding" (BUG-509): an external
/// stylesheet whose own encoding can't be determined via BOM/HTTP/`@charset`/
/// `<link charset>` falls back to the encoding of the document that linked
/// it. `Document::character_set` always holds a value (defaults to `"UTF-8"`
/// — see `Document::new`), so this only reaches the literal `Utf8` default
/// for a label this crate doesn't carry a table for, which cannot happen
/// today since the document's own encoding was itself set from
/// [`lumen_encoding::detect`]'s output.
pub(crate) fn document_encoding(doc: &Document) -> lumen_encoding::Encoding {
    lumen_encoding::Encoding::from_label(doc.character_set()).unwrap_or(lumen_encoding::Encoding::Utf8)
}

/// Загрузить все `<link rel=stylesheet>` документа и склеить их текст.
///
/// Второй элемент результата — исход по каждому элементу (`узел`, `получен
/// ли лист`) в порядке объявления, для BUG-804: `load`/`error` принадлежат
/// элементу `<link>`, а знает исход только этот проход. Раньше провал просто
/// логировался, и страница не могла отличить загруженный лист от 404.
///
/// Третий элемент — GAP-CSPENF срез 7: resolved URL каждого `<link>`, чей
/// фетч `style-src`/`default-src` документа запретил. Фетч для них не
/// выполнялся вовсе (сеть их не видела), поэтому такой лист даёт тот же
/// `false`-исход, что и сетевая неудача — вызывающая сторона уже диспатчит
/// `error` по этому исходу (BUG-804); `securitypolicyviolation` — отдельно,
/// той же схемой, что `blocked_by_img_src` в `subresources.rs` (здесь для
/// него нет JS-рантайма).
///
/// Срез 38: тот же вектор теперь несёт и resolved URL каждого `@import`
/// внутри ДОПУЩЕННОГО листа, которое `style-src`/`default-src` отдельно
/// запретило (CSP3 §6.4.1 — `@import` — такой же фетч, как сам `<link>`, и
/// проверяется по своему целевому URL, а не наследует статус владельца).
/// [`inline_css_imports`] делает саму проверку; этот проход лишь передаёт ей
/// уже посчитанные `csp_gate`/`self_origin` и подмешивает возвращённый
/// список в общий `blocked_by_style_src`.
pub(crate) fn load_linked_stylesheets(doc: &Document, base: &ResourceBase, sink: &Arc<dyn EventSink>, cookie_jar: Option<Arc<lumen_storage::CookieJar>>, media_ctx: &lumen_css_parser::MediaContext) -> (String, Vec<(NodeId, bool)>, Vec<String>) {
    let mut hrefs = Vec::new();
    collect_link_hrefs(doc, doc.root(), &mut hrefs, media_ctx);
    let doc_encoding = document_encoding(doc);
    // GAP-REFERRER срез 4: `<link rel=stylesheet>`/`@import` now carry the
    // document's own resolved policy (`<meta name=referrer>`/`Referrer-Policy`
    // header), the same way `<script src>`/top-level `fetch()` already do
    // (срез 3) — one of the six call sites `http_client_for_subresource`'s
    // doc comment still listed as default-only.
    let doc_referrer_policy = crate::resource_base::document_referrer_policy(doc);

    // GAP-CSPENF срез 7: посчитать политику один раз здесь же, до параллельной
    // фазы — та же одноразовая точка, что срез 4 использует в
    // `fetch_and_decode_images` для `img-src`.
    let csp_gate = {
        let root = doc.root();
        crate::csp_enforce::document_csp_policy(doc, root)
    };
    let self_origin = base.origin();

    // Загружаем все таблицы параллельно (сеть — главный тормоз), затем
    // конкатенируем строго в порядке объявления, чтобы каскад не нарушился.
    // Каждый лист резолвит собственные `@import` относительно СВОЕГО URL
    // (`sheet_base`), чтобы вложенные импорты (`<link href="/css/a.css">` →
    // `@import "b.css"` = `/css/b.css`) разрешались корректно.
    let gate_ref = csp_gate.as_ref().map(|(p, _)| (p.as_slice(), self_origin.as_ref()));
    let parts = parallel_map(&hrefs, |_, (_, href, charset_attr, referrer_policy_attr)| {
        // GAP-REFERRER срез 6: `referrerpolicy` на этом конкретном `<link>`
        // переопределяет политику документа только для его собственного
        // запроса (и для `@import`-ов внутри его листа — тот же источник,
        // что спек-примеры используют для унаследованной политики импорта).
        let referrer_policy = referrer_policy_attr
            .as_deref()
            .and_then(lumen_network::ReferrerPolicy::parse)
            .unwrap_or(doc_referrer_policy);
        if let Some((policy, _original)) = &csp_gate {
            let resolved_url = base.resolve_str(href);
            // GAP-CSPENF срез 47: гейт `style-src` обязан видеть тот же
            // апгрейженный адрес, что и фактический фетч ниже
            // (`fetch_stylesheet_text`, теперь принимающая тот же
            // `gate_ref`) — тем же порядком Fetch §4.1, что срезы 43-45 уже
            // дали картинкам и `<script src>`.
            let upgraded = crate::csp_enforce::upgrade_insecure_url(policy, &resolved_url);
            let gate_url = upgraded.as_deref().unwrap_or(&resolved_url);
            if crate::csp_enforce::style_src_blocked(policy, gate_url, self_origin.as_ref()) {
                return Err(Some(gate_url.to_owned()));
            }
        }
        let (text, sheet_base, encoding) = fetch_stylesheet_text(
            href,
            base,
            sink,
            cookie_jar.clone(),
            charset_attr.as_deref(),
            doc_encoding,
            gate_ref,
            referrer_policy,
        )
        .ok_or(None)?;
        Ok(inline_css_imports(
            &text,
            &sheet_base,
            sink,
            cookie_jar.clone(),
            media_ctx,
            &mut std::collections::HashSet::new(),
            0,
            encoding,
            gate_ref,
            referrer_policy,
        ))
    });

    let mut css = String::new();
    let mut outcomes = Vec::with_capacity(parts.len());
    let mut blocked_by_style_src = Vec::new();
    for ((node, _, _, _), part) in hrefs.iter().zip(parts) {
        match part {
            Ok((text, blocked_imports)) => {
                outcomes.push((*node, true));
                css.push_str(&text);
                css.push('\n');
                blocked_by_style_src.extend(blocked_imports);
            }
            Err(blocked_url) => {
                outcomes.push((*node, false));
                if let Some(blocked_url) = blocked_url {
                    blocked_by_style_src.push(blocked_url);
                }
            }
        }
    }
    (css, outcomes, blocked_by_style_src)
}

/// Загружает текст одной таблицы стилей, разрешённой относительно `base`.
///
/// Обрабатывает локальные пути (`file://`/относительные — читаются с диска)
/// и `http(s)` (через prefetch-кэш, как `<link rel=stylesheet>`). Возвращает
/// текст листа, его разрешённый [`ResourceBase`] (чтобы вложенные `@import`
/// резолвились относительно собственного URL листа, а не документа) и
/// кодировку, в которой лист был декодирован — она же становится
/// `referring_encoding` для его собственных `@import` (BUG-509). При любой
/// ошибке resolve/чтения/сети — `None` (залогировано), поэтому один битый
/// `@import`/`<link>` не валит весь рендер.
///
/// `link_charset_attr`/`referring_encoding` — два нижних яруса CSS Syntax L3
/// «determine the fallback encoding»
/// (<https://drafts.csswg.org/css-syntax-3/#determine-the-fallback-encoding>):
/// значение атрибута `<link charset=…>` (`None` для `@import`, у него такого
/// атрибута нет) и кодировка ссылающегося документа/листа. Полный порядок
/// приоритетов реализует [`lumen_encoding::detect_stylesheet_encoding`].
///
/// `csp_gate` — GAP-CSPENF срез 47: `upgrade-insecure-requests` переписывает
/// схему `http:` → `https:` ДО фактического запроса (та же точка, что срезы
/// 43-45 уже дали картинкам и `<script src>`); `None` = политики без
/// `upgrade-insecure-requests` вовсе, тогда ветка `ResolvedResource::Url`
/// фетчит `url` как раньше, без изменений.
#[allow(clippy::too_many_arguments)] // fetch context threaded through, same shape as `inline_css_imports`
fn fetch_stylesheet_text(
    href: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    link_charset_attr: Option<&str>,
    referring_encoding: lumen_encoding::Encoding,
    csp_gate: Option<(&[CspPolicy], Option<&Origin>)>,
    referrer_policy: lumen_network::ReferrerPolicy,
) -> Option<(String, ResourceBase, lumen_encoding::Encoding)> {
    match base.resolve(href) {
        ResolvedResource::File(path) => match std::fs::read(&path) {
            Ok(bytes) => {
                eprintln!("Загружен CSS: {}", path.display());
                let encoding = lumen_encoding::detect_stylesheet_encoding(
                    &bytes,
                    None,
                    link_charset_attr,
                    Some(referring_encoding),
                );
                Some((
                    lumen_encoding::decode(encoding, &bytes),
                    ResourceBase::File(path),
                    encoding,
                ))
            }
            Err(e) => {
                eprintln!("Пропуск CSS {}: {e}", path.display());
                None
            }
        },
        ResolvedResource::Url(raw_url) => {
            use lumen_core::url::Url;
            use lumen_network::RequestDestination;

            // GAP-CSPENF срез 47: апгрейженный адрес идёт в фактический
            // запрос, тем же принципом, что `scripts.rs::resolve_script_sources`
            // уже даёт `<script src>` (срез 45) — гейт (вызывающая сторона) и
            // фетч обязаны видеть один и тот же `https://`-адрес.
            let url = csp_gate
                .and_then(|(policy, _)| crate::csp_enforce::upgrade_insecure_url(policy, &raw_url))
                .unwrap_or(raw_url);

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
            let resource = crate::prefetch::PREFETCH_CACHE.fetch_current(&url, || {
                let client = base.http_client_for_subresource_with_policy(
                    sink.clone(),
                    cookie_jar.clone(),
                    referrer_policy,
                );
                client
                    .fetch_subresource_with_content_type(&sub_url, RequestDestination::Style)
                    .map(|(body, content_type)| crate::prefetch::CachedResource {
                        body,
                        content_type,
                    })
                    .map_err(|e| e.to_string())
            });
            match resource {
                Ok(resource) => {
                    fetch_span.set_bytes(resource.body.len());
                    let encoding = lumen_encoding::detect_stylesheet_encoding(
                        &resource.body,
                        resource.content_type.as_deref(),
                        link_charset_attr,
                        Some(referring_encoding),
                    );
                    Some((
                        lumen_encoding::decode(encoding, &resource.body),
                        ResourceBase::Url(url),
                        encoding,
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
///
/// `csp_gate` — GAP-CSPENF срез 38: политика (и origin документа, для
/// `'self'`) владельца этого листа, та же пара, что все fetch-гейты этого
/// файла (`style_src_blocked` в [`load_linked_stylesheets`]) уже принимают.
/// До этого среза `@import` не проверялся вообще ни на одном из трёх сайтов
/// вызова (внешний `<link>` внутри самого себя, инлайновый `<style>`
/// страницы, инлайновый `<style>` `<iframe>`) — CSP3 §6.4.1 требует того же
/// `style-src`-гейта для цели `@import`, что и для самого `<link>`, но
/// разбор листа никогда не сверялся с политикой. Проверяется КАЖДЫЙ уровень
/// вложенности: `csp_gate` передаётся дальше без изменений в рекурсивный
/// вызов — политика одна на весь документ, а не своя у каждого
/// импортированного листа (импортированный лист не приносит своей CSP).
/// Возвращает вторым элементом resolved URL каждой заблокированной цели, в
/// том же общем формате, что [`load_linked_stylesheets`] уже даёт для
/// заблокированного `<link>` — вызывающая сторона подмешивает его в
/// `blocked_by_style_src` для одного и того же `securitypolicyviolation`-пути
/// (`violatedDirective="style-src"`, `blockedURI` = URL импорта).
#[allow(clippy::too_many_arguments)] // recursive helper threading fetch context — see BUG-509
pub(crate) fn inline_css_imports(
    css_text: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    media_ctx: &lumen_css_parser::MediaContext,
    seen: &mut std::collections::HashSet<String>,
    depth: u32,
    referring_encoding: lumen_encoding::Encoding,
    csp_gate: Option<(&[CspPolicy], Option<&Origin>)>,
    referrer_policy: lumen_network::ReferrerPolicy,
) -> (String, Vec<String>) {
    let mut blocked = Vec::new();
    // Быстрый путь: нет токена `@import` вовсе → лишний парс не нужен
    // (подавляющее большинство листов). Ложные срабатывания (например
    // `@import` внутри комментария) безопасны — последующий парс правильно
    // не найдёт импорта и вернёт текст как есть.
    if !contains_ignore_ascii_case(css_text.as_bytes(), b"@import") {
        return (css_text.to_owned(), blocked);
    }
    let parsed = lumen_css_parser::parse(css_text);
    if parsed.imports.is_empty() {
        return (css_text.to_owned(), blocked);
    }
    if depth >= MAX_CSS_IMPORT_DEPTH {
        eprintln!("Пропуск @import: превышена глубина вложенности ({MAX_CSS_IMPORT_DEPTH})");
        return (css_text.to_owned(), blocked);
    }

    let mut prefix = String::new();
    for imp in &parsed.imports {
        // Media Queries L4: не матчащий контекст импорт не применяется.
        if !imp.media.matches(media_ctx) {
            continue;
        }
        // Цикл/дубликат: ключ = абсолютный резолв URL относительно текущего листа.
        let key = base.resolve_str(&imp.url);
        if !seen.insert(key.clone()) {
            continue;
        }
        // GAP-CSPENF срез 38: `style-src`/`default-src` против цели `@import`,
        // до сети — тот же принцип «заблокированный фетч не идёт в сеть
        // вовсе», что `load_linked_stylesheets` уже даёт `<link>`.
        //
        // Срез 47: `upgrade-insecure-requests` переписывает `key` ДО этого
        // гейта (та же схема, что `load_linked_stylesheets` уже даёт
        // `<link>`) — гейт и фактический фетч (`fetch_stylesheet_text`,
        // принимающая тот же `csp_gate`) обязаны видеть один адрес. `seen`
        // остаётся на сыром `key` — дедуп циклов не вопрос безопасности.
        if let Some((policy, self_origin)) = csp_gate {
            let upgraded = crate::csp_enforce::upgrade_insecure_url(policy, &key);
            let gate_url = upgraded.as_deref().unwrap_or(&key);
            if crate::csp_enforce::style_src_blocked(policy, gate_url, self_origin) {
                blocked.push(gate_url.to_owned());
                continue;
            }
        }
        let Some((text, imp_base, imp_encoding)) = fetch_stylesheet_text(
            &imp.url,
            base,
            sink,
            cookie_jar.clone(),
            None, // `@import` has no `<link charset>`-equivalent attribute
            referring_encoding,
            csp_gate,
            referrer_policy,
        ) else {
            continue;
        };
        let (resolved, nested_blocked) = inline_css_imports(
            &text,
            &imp_base,
            sink,
            cookie_jar.clone(),
            media_ctx,
            seen,
            depth + 1,
            imp_encoding,
            csp_gate,
            referrer_policy,
        );
        blocked.extend(nested_blocked);
        prefix.push_str(&resolved);
        if !prefix.ends_with('\n') {
            prefix.push('\n');
        }
    }

    if prefix.is_empty() {
        return (css_text.to_owned(), blocked);
    }
    prefix.push_str(css_text);
    (prefix, blocked)
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

/// Один `<style>`/`<link rel=stylesheet>`, отдельно распарсенный — единица,
/// которую `document.styleSheets`/`element.sheet` (CSSOM-1 срез 3) читают по
/// одной на элемент, в отличие от единого смерженного [`PageCascade::sheet`].
/// См. «Архитектурный пробел» в `docs/tasks/p1-cssom-1-stylesheets.md`.
///
/// Канонический тип — [`lumen_css_parser::StylesheetNodeEntry`]: `crates/js`
/// читает тот же реестр (`V8JsRuntime::update_stylesheet_nodes`), а
/// `lumen-js` не может зависеть от `lumen-shell` (наслоение), поэтому тип
/// живёт в общем для обоих сиблинге — `lumen-css-parser`. Реэкспорт здесь
/// оставляет имя `StylesheetNodeEntry` рабочим для существующих импортов
/// (`main.rs`, `page_pipeline.rs`) без правки их `use`.
pub(crate) use lumen_css_parser::StylesheetNodeEntry;

/// Один `<style>`/`<link rel=stylesheet>` в порядке документа, ещё без
/// разобранного текста — промежуточное значение
/// [`build_stylesheet_node_registry`].
enum StylesheetOwner {
    Style(NodeId),
    Link(NodeId, String, Option<String>),
}

/// Строит [`StylesheetNodeEntry`] по одному на `<style>`/`<link
/// rel=stylesheet>`, в порядке документа — реестр, который срез 3 отдаёт как
/// `document.styleSheets`.
///
/// Парсит текст каждого элемента отдельно от смерженного листа
/// [`build_page_cascade`]: тело `<link>` берётся из того же
/// `PREFETCH_CACHE`, что уже прогрет каскадом (`fetch_stylesheet_text`), так
/// что цена — лишний разбор CSS на элемент, не лишняя сеть. `media` здесь
/// осознанно не проверяется — принадлежность листа `document.styleSheets` не
/// зависит от того, матчит ли сейчас его `media` (CSSOM); этот гейт остаётся
/// только в [`collect_link_hrefs`] для каскада. Лист, чей `<link>` не
/// загрузился, в реестр не попадает — `element.sheet` для него останется
/// `null` на стороне JS (срез 3), как и для реального провала загрузки.
pub(crate) fn build_stylesheet_node_registry(
    doc: &Document,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Vec<StylesheetNodeEntry> {
    let mut owners = Vec::new();
    collect_stylesheet_owners(doc, doc.root(), &mut owners);
    let doc_encoding = document_encoding(doc);

    // GAP-CSPENF срез 47: тот же `csp_gate`, что `load_linked_stylesheets`
    // уже считает для того же документа — без него этот проход резолвил бы
    // и лукапил `PREFETCH_CACHE` по сырому (не апгрейженному) URL, промахнулся
    // мимо записи, сделанной апгрейженным фетчем, и тихо сходил бы в сеть по
    // `http://` второй раз только ради заполнения `document.styleSheets`.
    let csp_gate = {
        let root = doc.root();
        crate::csp_enforce::document_csp_policy(doc, root)
    };
    let self_origin = base.origin();
    let gate_ref = csp_gate.as_ref().map(|(p, _)| (p.as_slice(), self_origin.as_ref()));
    // GAP-REFERRER срез 4: same document-resolved policy as
    // `load_linked_stylesheets` — this registry hits the same
    // `PREFETCH_CACHE` entry, so the two must agree on the request that
    // filled it.
    let referrer_policy = crate::resource_base::document_referrer_policy(doc);

    let mut out = Vec::with_capacity(owners.len());
    for owner in owners {
        match owner {
            StylesheetOwner::Style(id) => {
                let text = style_element_text(doc, id);
                out.push(StylesheetNodeEntry {
                    node: id.index() as u32,
                    sheet: Arc::new(lumen_css_parser::parse(&text)),
                    disabled: false,
                });
            }
            StylesheetOwner::Link(id, href, charset_attr) => {
                if let Some((text, _, _)) = fetch_stylesheet_text(
                    &href,
                    base,
                    sink,
                    cookie_jar.clone(),
                    charset_attr.as_deref(),
                    doc_encoding,
                    gate_ref,
                    referrer_policy,
                ) {
                    out.push(StylesheetNodeEntry {
                        node: id.index() as u32,
                        sheet: Arc::new(lumen_css_parser::parse(&text)),
                        disabled: false,
                    });
                }
            }
        }
    }
    out
}

/// Рекурсивная половина [`build_stylesheet_node_registry`] — один обход,
/// а не два (как у `walk_style_blocks`/`collect_link_hrefs`), чтобы `<style>`
/// и `<link>` вышли в истинном порядке документа, а не сгруппированными по
/// тегу.
fn collect_stylesheet_owners(doc: &Document, id: NodeId, out: &mut Vec<StylesheetOwner>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data {
        if name.local == "style" {
            out.push(StylesheetOwner::Style(id));
            return;
        }
        if name.local == "link" {
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
            if rel.split_ascii_whitespace().any(|r| r.eq_ignore_ascii_case("stylesheet"))
                && !href.is_empty()
            {
                // BUG-509: legacy `<link charset=…>` — one tier of CSS
                // Syntax L3 "determine the fallback encoding".
                let charset = attrs
                    .iter()
                    .find(|a| a.name.local == "charset")
                    .map(|a| a.value.clone());
                out.push(StylesheetOwner::Link(id, href.to_owned(), charset));
            }
            return;
        }
    }
    for &child in &node.children {
        collect_stylesheet_owners(doc, child, out);
    }
}

/// Текст прямых текстовых детей одного `<style>`-узла (без обхода вглубь —
/// как [`walk_style_blocks`], но для одного узла, а не всех сразу).
fn style_element_text(doc: &Document, id: NodeId) -> String {
    let mut out = String::new();
    for &child in &doc.get(id).children {
        if let NodeData::Text(s) = &doc.get(child).data {
            out.push_str(s);
        }
    }
    out
}

/// Разобрать псевдо-атрибуты `<?xml-stylesheet ...?>` (`name="value"` /
/// `name='value'`, XML §2.3 `Attribute`-грамматика без декларации DTD) — та
/// же синтаксическая форма, что и у обычных XML-атрибутов, но живёт в теле
/// processing instruction, а не в теге, поэтому обычный аттрибут-парсер
/// токенизатора сюда не дотягивается.
fn parse_pi_pseudo_attrs(data: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = data.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if name_start == i {
            break;
        }
        let name = data[name_start..i].to_owned();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            break;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let Some(&quote) = bytes.get(i).filter(|b| **b == b'"' || **b == b'\'') else {
            break;
        };
        i += 1;
        let value_start = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let value = data[value_start..i].to_owned();
        i += 1;
        out.push((name, value));
    }
    out
}

/// Собрать `(узел, href, charset-атрибут)` каждого `<link rel=stylesheet>`,
/// который попадёт в каскад.
///
/// Узел нужен BUG-804: по нему [`load_linked_stylesheets`] потом сообщает
/// JS-стороне исход загрузки, чтобы элемент выстрелил `load`/`error`. Раньше
/// собирались одни адреса, и связи «этот лист — этот элемент» не существовало.
/// `charset` — легаси-атрибут `<link>` (HTML LS), один из ярусов CSS Syntax L3
/// «determine the fallback encoding» (BUG-509).
pub(crate) fn collect_link_hrefs(doc: &Document, id: NodeId, out: &mut Vec<(NodeId, String, Option<String>, Option<String>)>, media_ctx: &lumen_css_parser::MediaContext) {
    let node = doc.get(id);
    if let NodeData::ProcessingInstruction { target, data } = &node.data {
        if target == "xml-stylesheet" {
            let pseudo = parse_pi_pseudo_attrs(data);
            let get = |k: &str| pseudo.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
            let sheet_type = get("type").unwrap_or("text/css");
            let href = get("href").unwrap_or("");
            let media = get("media").unwrap_or("");
            let alternate = get("alternate").unwrap_or("no");
            // `alternate="yes"` без `<link rel="alternate stylesheet">`-эквивалента
            // выбора пользователем — тот же принцип, что и обычный alternate
            // `<link>`: не входит в каскад по умолчанию.
            if (sheet_type.is_empty() || sheet_type.eq_ignore_ascii_case("text/css"))
                && !href.is_empty()
                && !alternate.eq_ignore_ascii_case("yes")
                && link_media_matches(media, media_ctx)
            {
                // `<?xml-stylesheet?>` не несёт `referrerpolicy`-псевдоатрибута
                // ни по одной спеке — всегда на политику документа.
                out.push((id, href.to_owned(), None, None));
            }
        }
        return;
    }
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
            let charset = attrs
                .iter()
                .find(|a| a.name.local == "charset")
                .map(|a| a.value.clone());
            // GAP-REFERRER срез 6: `referrerpolicy` на `<link>` переопределяет
            // политику документа только для запроса этого листа.
            let referrer_policy = attrs
                .iter()
                .find(|a| a.name.local == "referrerpolicy")
                .map(|a| a.value.clone())
                .filter(|s| !s.is_empty());
            out.push((id, href.to_owned(), charset, referrer_policy));
        }
        return;
    }
    for &child in &node.children {
        collect_link_hrefs(doc, child, out, media_ctx);
    }
}
