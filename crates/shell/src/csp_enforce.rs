//! Content Security Policy enforcement — срез 1 (GAP-CSPENF): `script-src`
//! против инлайновых `<script>`/module-скриптов, взятых из `<meta
//! http-equiv="Content-Security-Policy">`. Срез 4 добавил `img-src`/
//! `default-src` против `<img src>` (host/scheme/`'self'`-источники, не
//! только keyword). Срез 5 добавил заголовок `Content-Security-Policy`
//! ответа: он доезжает до документа (`Document::csp_header`) и сливается с
//! `<meta>`-политиками в [`document_csp_policy`], поэтому все точки
//! enforcement видят его без изменений в них самих. Срез 6 добавил
//! `script-src`/`default-src` против внешнего `<script src>` — та же
//! host/scheme/`'self'` проверка, что срез 4 сделал для `img-src`, теперь
//! останавливает fetch внешнего скрипта до сети.
//!
//! Срез 7 добавил `style-src`/`default-src` против внешнего `<link
//! rel=stylesheet>` — тот же host/scheme/`'self'` фетч-гейт, что срезы 4 и 6
//! дали `img-src`/`script-src`, применённый к `load_linked_stylesheets`
//! (`crates/shell/src/stylesheets.rs`); заблокированный лист не фетчится и
//! становится тем же `error`-исходом, что уже даёт сетевая неудача (BUG-804).
//!
//! Срез 9 закрыл последний непроверенный производитель картинок:
//! `loading="lazy"` (`Lumen::fetch_and_register_lazy_images`, `page_load.rs`)
//! теперь гейтится тем же `img_src_blocked`, что срез 4 уже дал eager- и
//! streaming-путям.
//!
//! Срез 10 добавил `connect-src` против `fetch()`/`XMLHttpRequest` — не в
//! этом файле: у JS-инициированного запроса нет точки кода с `&Document` под
//! рукой (в отличие от парсер-/страница-производителей выше), поэтому гейт
//! живёт в `lumen-network::HttpClient::fetch_request_impl`
//! (`with_connect_src_policy`, `crates/network/src/lib.rs`), а
//! `document_csp_policy` из этого модуля используется лишь один раз — в
//! `page_pipeline.rs::parse_and_layout`, чтобы собрать политику для этого
//! `HttpClient` перед тем, как он станет `fetch_provider`. Детали —
//! `bugs/BUG-811-OPEN.md` срез 10.
//!
//! Срезы 11/12 (тоже вне этого файла, по той же причине, что срез 10) добавили
//! `connect-src` против WebSocket/EventSource (`crates/network/src/lib.rs`'s
//! `JsWebSocketProvider`/`JsSseProvider`) и `sendBeacon` (`check_connect_src`,
//! `crates/core/src/ext.rs`) — оба делят один `HttpClient` и один
//! `connect_src_policy` с `fetch()`.
//!
//! Срез 13 добавил `worker-src` (falling back to `default-src` — `worker-src`
//! не получил своего child-src/script-src промежуточного шага CSP3 §6.4, тот
//! же однократный фолбэк на `default-src`, что и у всех директив здесь) против
//! `new Worker(url)`/`new SharedWorker(url)`: тоже вне этого файла — у
//! `_lumen_worker_fetch_script`/`_lumen_sw_fetch_script` (`crates/js/src/
//! worker.rs`/`shared_worker.rs`) нет `&Document`, гейт живёт в
//! `lumen-network::HttpClient::check_worker_src` (`with_worker_src_policy`),
//! тот же `document_csp_policy` из `page_pipeline.rs::parse_and_layout`, что
//! срез 10 уже собирает для `connect_src_policy`. Детали — `bugs/
//! BUG-811-OPEN.md` срез 13.
//!
//! Срез 14 (`crates/js/src/csp.rs`, вне этого файла — JS-only) добавил
//! доставку отчётов `report-uri`: `_lumen_dispatch_csp_violation`
//! переизвлекает директиву из уже доехавшей `originalPolicy` и шлёт
//! `fetch(..., {method:'POST'})` на каждый URI. `report-to` не тронут.
//!
//! Срез 15 добавил `frame-src`/`default-src` против навигации `<iframe>`/
//! `<frame>` — тот же host/scheme/`'self'` фетч-гейт, что срезы 4/6/7 дали
//! `img-src`/`script-src`/`style-src`, применённый в `frames.rs::spawn_frame`
//! перед вызовом `fetch_iframe_source` (не в этом файле — у гейта нет
//! готового `&Document`/`ResourceBase` без явной проводки, тот же повод, что
//! у срезов 10-13). Проверяются оба пути (первичная вставка и навигация,
//! включая переприсваивание `.src`); `about:blank`/пустой `src` исключены
//! заранее — CSP3 §6.5 их не ограничивает, они не долетают до сети/диска.
//!
//! Срез 18 добавил `img-src`/`default-src` против `background-image:
//! url(...)` страницы (`subresources.rs::fetch_and_decode_background_images`)
//! — переиспользует уже существующий [`img_src_blocked`] (срез 4), гейт
//! только на top-level документе; фон под-документа `<iframe>`
//! (`frames.rs::fetch_frame_background_images`) не тронут этим срезом.
//!
//! Срез 19 добавил `font-src`/`default-src` против `@font-face url()` — эта
//! директива не была даже распарсена до этого среза (`CspDirective::FontSrc`
//! — новый вариант). Тот же host/scheme/`'self'` фетч-гейт, что дают
//! [`img_src_blocked`]/[`media_src_blocked`], под именем [`font_src_blocked`];
//! вызывается не отсюда — единственный фетчер (`page_load.rs::
//! apply_loaded_page`'s `pending_web_fonts`-цикл) грузит байты на детач-потоке
//! без `&Document`, поэтому решение «фетчить или нет» принимается на главном
//! потоке до `std::thread::spawn`, той же одноразовой схемой чтения политики,
//! что срез 9 уже даёт `loading="lazy"`. Шрифты внутри `<iframe>`
//! (`frames.rs::load_frame_fonts`) не тронуты.
//!
//! Срез 20 добавил `'sha256-…'`/`'sha384-…'`/`'sha512-…'` (CSP3 §8.1) к
//! [`inline_script_blocked`] — до этого среза `CspSource::Hash` разбирался
//! (`crates/network/src/csp.rs`), но не участвовал в проверке: инлайновый
//! скрипт под политикой, чей единственный разрешённый источник — хэш, читался
//! как всегда заблокированный. Тело скрипта хэшируется новым
//! `HashAlgorithm::digest_base64` (`sha2`, уже в дереве зависимостей
//! `lumen-network` — TLS-цепочка сертификатов) и сравнивается со значением
//! каждого `Hash`-источника директивы; совпадение любого допускает
//! исполнение, тем же принципом «одного достаточно», что уже есть у
//! nonce/`'unsafe-inline'`. Только классические/модульные инлайновые
//! `<script>` (`scripts.rs`) — не событийные атрибуты (`onclick=…`, которых
//! `'unsafe-hashes'` касается отдельно) и не `style`-src.
//!
//! Срез 21 добавил `style-src`/`default-src` против инлайновых `<style>` —
//! [`inline_style_blocked`], тот же `'unsafe-inline'`/`'nonce-…'`/
//! `'sha256-…'`-набор, что [`inline_script_blocked`] уже даёт скриптам,
//! только применённый к `CspDirective::StyleSrc`. Гейт стоит в
//! `doc_extract::walk_style_blocks`, до склейки каскада: заблокированный
//! `<style>`-узел не попадает в текст, который парсит [`lumen_css_parser`],
//! вовсе — тот же принцип «не применённый CSS», что срез 7 уже даёт
//! заблокированному внешнему `<link>`. Атрибут `style=` этим не покрыт.
//!
//! Срез 22 закрыл ровно тот пробел, что срез 21 назвал не покрытым:
//! инлайновый `<style>` внутри `<iframe>` (`frames.rs::
//! fetch_frame_subresources` вызывала `extract_style_blocks(doc, None)` —
//! политика ребёнка там уже считалась для `img-src`/`style-src` внешних
//! листов (срез 8), просто не передавалась в этот вызов). Гейт — тот же
//! [`inline_style_blocked`], что срез 21 даёт top-level документу, против
//! CHILD'а собственной политики; `securitypolicyviolation` диспатчится из
//! `spawn_frame` тем же one-shot-push путём, что уже несёт
//! `blocked_by_img_src`/`blocked_by_style_src` для этого ребёнка.
//!
//! Срез 23 закрыл последний класс инлайна, названный не покрытым срезами
//! 21/22: атрибут `style=""` на произвольном элементе. Архитектурно другое
//! решение, чем срезы 21/22: точка потребления (`lumen_layout::style::
//! cascade`) — единственный choke point, единый для парсер-, скрипт- и
//! CSSOM-вставленного значения атрибута, — но `layout` не зависит от
//! `lumen-network`/`CspPolicy` (layering `dom → layout`, а не `network →
//! layout`), поэтому решение считается один раз в shell'е и travels down as
//! bare node ids (`Document::style_attr_csp_blocked`, новое поле —
//! `layout`/`dom` не знают о CSP вовсе). [`style_attribute_blocked`] — тот же
//! host/scheme-независимый гейт по духу, что [`inline_style_blocked`], но с
//! другим набором источников (CSP3 §6.4.2/§8.1): нет nonce (атрибут не может
//! нести `nonce=` для самого себя), а хэш-источник допускает совпадение
//! только вместе с `'unsafe-hashes'` — голого хэша достаточно для `<style>`
//! элемента, но никогда не для атрибута. Фолбэк на один уровень глубже
//! остальных директив этого файла: `style-src-attr` → `style-src` →
//! `default-src` (CSP3 §6.4 granular chain). Вычисляется один раз в
//! `build_page_cascade` (та же точка, что срез 21 уже даёт `<style>`), гейт
//! стоит в `cascade.rs` перед `parse_inline_style` — атрибут остаётся в DOM
//! нетронутым (`getAttribute('style')` не меняется), исключается только его
//! эффект на каскад. Покрывает только элементы дерева на момент вычисления
//! каскада (начальный парсинг + пересборка при `scripts_changed_css`) — узел,
//! получивший `style=""` другим путём после этого момента (простой
//! `setAttribute`/`style.cssText`, не трогающий `<style>`/`<link>`), гейт не
//! видит; `<iframe>` не тронут этим срезом.
//!
//! Срез 25 закрыл ровно тот пробел, что срез 18/19 назвали не покрытым:
//! `@font-face url()` и `background-image: url()` внутри `<iframe>` —
//! `load_frame_fonts`/`fetch_frame_background_images` (`frames.rs`) фетчили
//! оба ребёнком без единой проверки политики. Тот же `font_src_blocked`/
//! [`img_src_blocked`], что уже даёт top-level документу, применённый к
//! CHILD'у собственной политики (`document_csp_policy` того же `child_doc_arc`,
//! читается один раз перед обоими вызовами в `spawn_frame`); `local()`-шрифты
//! не затронуты — `font-src` гейтит только сетевой фетч.
//!
//! Срез 26 добавил разбор директивы `child-src` (`CspDirective::ChildSrc`
//! — до этого среза не распознавалась вовсе, падала в `_ => continue`) и
//! CSP3 §6.4 granular-фолбэк для `frame-src`/`worker-src`: обе раньше падали
//! напрямую на `default-src` через общий [`CspPolicy::effective_sources`],
//! минуя промежуточный `child-src`, который спека требует проверить первым.
//! Новый [`CspPolicy::fetch_directive_allows_via_child_src`] — тот же метод,
//! что срез 23 уже даёт `style-src-attr` (одним уровнем глубже общего
//! случая), применённый к [`frame_src_blocked`] (этот файл) и
//! `worker_src_gate` (`crates/network/src/lib.rs`, вне этого файла — та же
//! причина, что у срезов 10/13: нет `&Document` в точке принятия решения).
//!
//! Что НЕ покрыто (следующие срезы): остальные директивы (`manifest-src`/…),
//! `report-to` (Reporting API,
//! нужны группы эндпоинтов из `Report-To`, этот движок его не разбирает),
//! атрибут `style=` внутри `<iframe>` после точечной DOM-мутации (см.
//! выше), `@import` внутри уже загруженного листа (наследует
//! политику владельца, отдельно не проверяется), честная независимая
//! проверка заголовка и `<meta>` вместо их слияния, `importScripts()`
//! внутри уже запущенного воркера (`worker-src` гейтит только начальный
//! скрипт конструктора, не последующие `importScripts`), `frame-ancestors`
//! (распознаётся парсером, но нигде не проверяется). См.
//! `bugs/BUG-811-OPEN.md`.

use lumen_network::csp::{CspDirective, CspPolicy, CspSource};
use lumen_network::Origin;

use crate::*;

/// Собрать текст каждой `<meta http-equiv="Content-Security-Policy">`
/// документа, в порядке документа. `Content-Security-Policy-Report-Only`
/// не поддерживается через `<meta>` — это и в спеке недопустимо (HTML LS
/// не даёт `http-equiv` репортинг-варианту).
fn collect_meta_csp(doc: &Document, id: NodeId, out: &mut Vec<String>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "meta"
    {
        let http_equiv = attrs
            .iter()
            .find(|a| a.name.local == "http-equiv")
            .map(|a| a.value.as_str())
            .unwrap_or("");
        if http_equiv.eq_ignore_ascii_case("content-security-policy")
            && let Some(content) = attrs.iter().find(|a| a.name.local == "content")
        {
            out.push(content.value.clone());
        }
        return;
    }
    for &child in &node.children {
        collect_meta_csp(doc, child, out);
    }
}

/// Действующая политика документа: заголовок `Content-Security-Policy` ответа
/// (срез 5, `Document::csp_header`) плюс каждая `<meta
/// http-equiv="Content-Security-Policy">` (срез 1), в порядке «заголовок,
/// затем документ».
///
/// Заголовок и каждая `<meta>` по спецификации (CSP3 §3.4) — независимые
/// политики, каждая проверяется отдельно, и нарушение любой из них —
/// нарушение; здесь они упрощённо сливаются в одну строку через `;` — для
/// одиночной политики (подавляющее большинство случаев) результат совпадает,
/// для нескольких политик со связанными ослаблениями (например,
/// `'unsafe-inline'` в одной и `'self'` в другой) это может дать более мягкий
/// эффективный результат, чем спецификация. Честная независимая проверка —
/// отдельная работа (`bugs/BUG-811-OPEN.md`).
///
/// `Content-Security-Policy-Report-Only` не учитывается ни с той, ни с другой
/// стороны: у `<meta>` репортинг-вариант недопустим по HTML LS, а заголовок
/// отфильтрован в `page_source::content_security_policy_header` — здесь
/// enforcement, а report-only по определению ничего не блокирует.
///
/// The returned `String` is the combined raw policy text — carried through to
/// `SecurityPolicyViolationEvent.originalPolicy` (CSP3 §7.8), which the
/// parsed [`CspPolicy`] itself does not retain.
pub(crate) fn document_csp_policy(doc: &Document, root: NodeId) -> Option<(CspPolicy, String)> {
    let mut parts: Vec<String> = doc.csp_header().map(str::to_owned).into_iter().collect();
    collect_meta_csp(doc, root, &mut parts);
    if parts.is_empty() {
        return None;
    }
    let combined = parts.join("; ");
    let policy = lumen_network::csp::parse_csp_header(&combined);
    Some((policy, combined))
}

/// `true`, если `script-src` (или `default-src`) документа запрещает
/// инлайновое исполнение с данным `nonce` (атрибут `nonce` элемента
/// `<script>`, `None` — атрибута нет) и телом `body` — срез 20 добавил
/// проверку `'sha256-…'`/`'sha384-…'`/`'sha512-…'` (CSP3 §8.1): тело
/// хэшируется под КАЖДЫМ алгоритмом, названным хотя бы одним источником
/// директивы (обычно один, но политика вправе перечислить несколько), а не
/// только под первым встреченным — совпадение любого достаточно.
///
/// Отсутствие директивы, применимой к скриптам, — не нарушение (страница не
/// объявляла ограничения). `'strict-dynamic'` без совпавшего nonce/хэша НЕ
/// разрешает голый инлайн (CSP3 §8.2) — здесь не учитывается умышленно, тем
/// самым инлайн без nonce/хэша остаётся заблокированным.
pub(crate) fn inline_script_blocked(policy: &CspPolicy, nonce: Option<&str>, body: &str) -> bool {
    inline_directive_blocked(policy, &CspDirective::ScriptSrc, nonce, body)
}

/// `true`, если `style-src` (или `default-src`) документа запрещает данный
/// инлайновый `<style>` — срез 21, тот же `'unsafe-inline'`/`'nonce-…'`/
/// `'sha256-…'` набор источников, что [`inline_script_blocked`] уже даёт
/// скриптам, применённый к `CspDirective::StyleSrc`. Атрибут `style=` и
/// событийные обработчики этим не покрыты — только тело `<style>`.
pub(crate) fn inline_style_blocked(policy: &CspPolicy, nonce: Option<&str>, body: &str) -> bool {
    inline_directive_blocked(policy, &CspDirective::StyleSrc, nonce, body)
}

/// `true` if `style-src-attr`/`style-src`/`default-src` forbids the value of
/// a `style=""` attribute whose text is `body` — срез 23, last inline class
/// срезы 21/22 named as not covered. Unlike [`inline_style_blocked`] (which
/// gates `<style>` element text), CSP3 §6.4.2 "inline check" treats an
/// attribute differently on two points: there is no nonce for a `style=`
/// attribute (an element cannot carry a nonce for its own attribute, only
/// for a `<style>`/`<script>` element's own `nonce=` attribute), and a hash
/// source only matches an attribute if the policy also carries
/// `'unsafe-hashes'` (CSP3 §8.1) — a bare hash source is enough for `<style>`
/// element text but never for an attribute. Fallback chain is the CSP3 §6.4
/// granular one (`style-src-attr` → `style-src` → `default-src`), one step
/// deeper than [`inline_directive_blocked`]'s single `directive` →
/// `default-src` step used by every other directive in this file.
pub(crate) fn style_attribute_blocked(policy: &CspPolicy, body: &str) -> bool {
    let Some(sources) = policy
        .directives
        .get(&CspDirective::StyleSrcAttr)
        .or_else(|| policy.directives.get(&CspDirective::StyleSrc))
        .or_else(|| policy.directives.get(&CspDirective::DefaultSrc))
    else {
        return false;
    };
    let unsafe_hashes = sources.contains(&CspSource::UnsafeHashes);
    let allowed = sources.iter().any(|s| match s {
        CspSource::UnsafeInline => true,
        CspSource::Hash { algorithm, value } => {
            unsafe_hashes && algorithm.digest_base64(body.as_bytes()) == *value
        }
        _ => false,
    });
    !allowed
}

/// Общая проверка [`inline_script_blocked`]/[`inline_style_blocked`]: любой
/// совпавший источник (`'unsafe-inline'` ИЛИ nonce ИЛИ хэш) допускает
/// инлайн; отсутствие директивы, применимой к `directive`, — не нарушение.
fn inline_directive_blocked(
    policy: &CspPolicy,
    directive: &CspDirective,
    nonce: Option<&str>,
    body: &str,
) -> bool {
    let Some(sources) = policy.effective_sources(directive) else {
        return false;
    };
    let allowed = sources.iter().any(|s| match s {
        CspSource::UnsafeInline => true,
        CspSource::Nonce(n) => nonce.is_some_and(|actual| actual == n),
        CspSource::Hash { algorithm, value } => algorithm.digest_base64(body.as_bytes()) == *value,
        _ => false,
    });
    !allowed
}

/// Вызвать уже определённый JS-хук `_lumen_dispatch_csp_violation`
/// (`crates/js/src/csp.rs`) — единственная точка диспетчеризации
/// `securitypolicyviolation`, срез 1 зовёт её впервые для инлайна
/// (`blocked_uri = "inline"`); срез 6 обобщил на внешний `<script src>`
/// (`blocked_uri` = резолвленный адрес файла).
pub(crate) fn fire_script_src_violation(
    rt: &lumen_js::v8_runtime::V8JsRuntime,
    blocked_uri: &str,
    original_policy: &str,
) {
    use lumen_core::ext::JsRuntime as _;
    let _ = rt.eval(&format!(
        "_lumen_dispatch_csp_violation({}, {}, {}, 'enforce');",
        js_string_literal("script-src"),
        js_string_literal(blocked_uri),
        js_string_literal(original_policy),
    ));
}

/// `true` if `script-src` (or `default-src`) forbids fetching the external
/// `<script src>` at `url` — срез 6, external counterpart to
/// [`inline_script_blocked`]. Same "don't invent a violation" stance as
/// [`img_src_blocked`]: a `url` that fails to parse is treated as allowed.
pub(crate) fn script_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows(&CspDirective::ScriptSrc, &parsed, self_origin)
}

/// `true` if `img-src` (or `default-src`) forbids fetching `url` — срез 4.
/// Absence of a policy is not checked here (the caller only calls this when
/// a policy exists); a `url` that fails to parse is treated as allowed — the
/// fetch proceeds and hits the normal network-failure path instead of a CSP
/// one, same "don't invent a violation" stance as the rest of this module.
pub(crate) fn img_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows(&CspDirective::ImgSrc, &parsed, self_origin)
}

/// `true` if `style-src` (or `default-src`) forbids fetching the external
/// `<link rel=stylesheet>` at `url` — срез 7, same fetch-gate shape as
/// [`img_src_blocked`]/[`script_src_blocked`]: absence of a policy is not
/// checked here (the caller only calls this when a policy exists), and a
/// `url` that fails to parse is treated as allowed.
pub(crate) fn style_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows(&CspDirective::StyleSrc, &parsed, self_origin)
}

/// `true` if `frame-src` (falling back to `child-src`, then `default-src` —
/// срез 26) forbids navigating a nested `<iframe>`/`<frame>` to `url` — срез
/// 15, same fetch-gate shape as
/// [`img_src_blocked`]/[`script_src_blocked`]/[`style_src_blocked`]: absence
/// of a policy is not checked here (the caller only calls this when a policy
/// exists), and a `url` that fails to parse is treated as allowed (the
/// caller's own scheme special-cases — `about:blank`, empty `src` — are
/// expected to have already been filtered out before this is called, since
/// those never reach the network/filesystem and CSP3 §6.5 does not restrict
/// them).
pub(crate) fn frame_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows_via_child_src(&CspDirective::FrameSrc, &parsed, self_origin)
}

/// `true` if `media-src` (or `default-src`) forbids fetching `url` as a
/// `<track src>` WebVTT body — срез 17, same fetch-gate shape as
/// [`img_src_blocked`]/[`style_src_blocked`]/[`frame_src_blocked`].
///
/// This is the shell's half of the `media-src` gate, and it exists because
/// `<track>` bodies are fetched **twice** by this engine from two unrelated
/// places: the JS shim's own `readTrackBody` (gated by the native
/// `_lumen_check_media_src` binding, `lumen-network`) and — before any JS runs
/// — `tracks::load_video_tracks`, the shell's overlay snapshot, which has a
/// `&Document` and so is gated here instead. Gating only the shim's half left
/// the bytes going out anyway.
pub(crate) fn media_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows(&CspDirective::MediaSrc, &parsed, self_origin)
}

/// `true` if `font-src` (or `default-src`) forbids fetching `url` as an
/// `@font-face url()` body — срез 19, same fetch-gate shape as
/// [`img_src_blocked`]/[`media_src_blocked`]: absence of a policy is not
/// checked here (the caller only calls this when a policy exists), and a
/// `url` that fails to parse is treated as allowed.
pub(crate) fn font_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows(&CspDirective::FontSrc, &parsed, self_origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_policy_allows_inline() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!inline_script_blocked(&p, None, ""));
    }

    #[test]
    fn script_src_none_blocks_inline() {
        let p = lumen_network::csp::parse_csp_header("script-src 'none'");
        assert!(inline_script_blocked(&p, None, ""));
    }

    #[test]
    fn script_src_unsafe_inline_allows() {
        let p = lumen_network::csp::parse_csp_header("script-src 'self' 'unsafe-inline'");
        assert!(!inline_script_blocked(&p, None, ""));
    }

    #[test]
    fn default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'self'");
        assert!(inline_script_blocked(&p, None, ""));
    }

    #[test]
    fn matching_nonce_allows() {
        let p = lumen_network::csp::parse_csp_header("script-src 'nonce-abc123'");
        assert!(!inline_script_blocked(&p, Some("abc123"), ""));
    }

    #[test]
    fn mismatched_nonce_blocks() {
        let p = lumen_network::csp::parse_csp_header("script-src 'nonce-abc123'");
        assert!(inline_script_blocked(&p, Some("other"), ""));
    }

    /// GAP-CSPENF срез 20: `'sha256-…'` matching the actual inline body allows it.
    #[test]
    fn matching_sha256_hash_allows() {
        let p = lumen_network::csp::parse_csp_header(
            "script-src 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(!inline_script_blocked(&p, None, "alert(1)"));
    }

    /// A hash source for a *different* body still blocks — one match is not
    /// "any hash source present".
    #[test]
    fn mismatched_hash_blocks() {
        let p = lumen_network::csp::parse_csp_header(
            "script-src 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(inline_script_blocked(&p, None, "alert(2)"));
    }

    /// `sha384`/`sha512` are matched too, not only `sha256` — CSP3 §8.1 does
    /// not privilege one algorithm.
    #[test]
    fn matching_sha384_hash_allows() {
        let p = lumen_network::csp::parse_csp_header(
            "script-src 'sha384-HT2E9NfWiuQ/w1PRai+hTyqW16NIoCGA/m8VQDUopfAtcz6YQjtsMmQd5uRbVDpW'",
        );
        assert!(!inline_script_blocked(&p, None, "alert(1)"));
    }

    /// A policy naming both a nonce and a hash source accepts either — the
    /// match loop must not short-circuit on the first source kind it sees.
    #[test]
    fn hash_matches_even_when_nonce_source_also_present() {
        let p = lumen_network::csp::parse_csp_header(
            "script-src 'nonce-unrelated' 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(!inline_script_blocked(&p, None, "alert(1)"));
    }

    // GAP-CSPENF срез 21: `inline_style_blocked` shares the exact match logic
    // `inline_script_blocked` already has (`inline_directive_blocked`) — these
    // only prove it is wired to `CspDirective::StyleSrc`, not `ScriptSrc`
    // (`style_src_none_blocks_inline_style` and `no_style_src_allows_inline`),
    // plus one nonce/hash spot-check that a sibling directive does not leak
    // its sources into `style-src`.

    #[test]
    fn no_style_src_allows_inline_style() {
        let p = lumen_network::csp::parse_csp_header("script-src 'none'");
        assert!(!inline_style_blocked(&p, None, ""));
    }

    #[test]
    fn style_src_none_blocks_inline_style() {
        let p = lumen_network::csp::parse_csp_header("style-src 'none'");
        assert!(inline_style_blocked(&p, None, ""));
    }

    #[test]
    fn style_src_unsafe_inline_allows_inline_style() {
        let p = lumen_network::csp::parse_csp_header("style-src 'self' 'unsafe-inline'");
        assert!(!inline_style_blocked(&p, None, ""));
    }

    #[test]
    fn script_src_unsafe_inline_does_not_allow_inline_style() {
        let p = lumen_network::csp::parse_csp_header("script-src 'unsafe-inline'; style-src 'none'");
        assert!(inline_style_blocked(&p, None, ""));
    }

    #[test]
    fn style_src_matching_nonce_allows_inline_style() {
        let p = lumen_network::csp::parse_csp_header("style-src 'nonce-abc123'");
        assert!(!inline_style_blocked(&p, Some("abc123"), ""));
    }

    #[test]
    fn style_src_matching_sha256_hash_allows_inline_style() {
        let p = lumen_network::csp::parse_csp_header(
            "style-src 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(!inline_style_blocked(&p, None, "alert(1)"));
    }

    #[test]
    fn no_img_src_allows() {
        let p = lumen_network::csp::parse_csp_header("script-src 'self'");
        assert!(!img_src_blocked(&p, "https://example.com/x.png", None));
    }

    #[test]
    fn img_src_none_blocks() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(img_src_blocked(&p, "https://example.com/x.png", None));
    }

    #[test]
    fn img_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("img-src cdn.example.com");
        assert!(!img_src_blocked(&p, "https://cdn.example.com/x.png", None));
        assert!(img_src_blocked(&p, "https://other.example.com/x.png", None));
    }

    /// GAP-CSPENF срез 5: a document with no `<meta>` CSP still has a policy
    /// when the response carried the header.
    #[test]
    fn response_header_alone_is_a_policy() {
        let mut doc = Document::new();
        doc.set_csp_header(Some("script-src 'none'".to_owned()));
        let root = doc.root();
        let (policy, original) =
            document_csp_policy(&doc, root).expect("header alone must produce a policy");
        assert!(inline_script_blocked(&policy, None, ""));
        assert_eq!(original, "script-src 'none'");
    }

    /// No header and no `<meta>` — no policy at all, so nothing is blocked.
    #[test]
    fn no_header_and_no_meta_is_no_policy() {
        let doc = Document::new();
        let root = doc.root();
        assert!(document_csp_policy(&doc, root).is_none());
    }

    #[test]
    fn img_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!img_src_blocked(&p, "not a url", None));
    }

    /// GAP-CSPENF срез 6: `script-src` against an external `<script src>`.
    #[test]
    fn no_script_src_allows_external() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!script_src_blocked(&p, "https://example.com/a.js", None));
    }

    #[test]
    fn script_src_none_blocks_external() {
        let p = lumen_network::csp::parse_csp_header("script-src 'none'");
        assert!(script_src_blocked(&p, "https://example.com/a.js", None));
    }

    #[test]
    fn script_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("script-src cdn.example.com");
        assert!(!script_src_blocked(&p, "https://cdn.example.com/a.js", None));
        assert!(script_src_blocked(&p, "https://other.example.com/a.js", None));
    }

    #[test]
    fn script_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("script-src 'none'");
        assert!(!script_src_blocked(&p, "not a url", None));
    }

    /// GAP-CSPENF срез 7: `style-src` against an external `<link
    /// rel=stylesheet>`.
    #[test]
    fn no_style_src_allows_external() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!style_src_blocked(&p, "https://example.com/a.css", None));
    }

    #[test]
    fn style_src_none_blocks_external() {
        let p = lumen_network::csp::parse_csp_header("style-src 'none'");
        assert!(style_src_blocked(&p, "https://example.com/a.css", None));
    }

    #[test]
    fn style_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("style-src cdn.example.com");
        assert!(!style_src_blocked(&p, "https://cdn.example.com/a.css", None));
        assert!(style_src_blocked(&p, "https://other.example.com/a.css", None));
    }

    #[test]
    fn style_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(style_src_blocked(&p, "https://example.com/a.css", None));
    }

    #[test]
    fn style_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("style-src 'none'");
        assert!(!style_src_blocked(&p, "not a url", None));
    }

    /// GAP-CSPENF срез 15: `frame-src` against `<iframe>`/`<frame>` navigation.
    #[test]
    fn no_frame_src_allows_navigation() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!frame_src_blocked(&p, "https://example.com/frame.html", None));
    }

    #[test]
    fn frame_src_none_blocks_navigation() {
        let p = lumen_network::csp::parse_csp_header("frame-src 'none'");
        assert!(frame_src_blocked(&p, "https://example.com/frame.html", None));
    }

    #[test]
    fn frame_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("frame-src cdn.example.com");
        assert!(!frame_src_blocked(&p, "https://cdn.example.com/frame.html", None));
        assert!(frame_src_blocked(&p, "https://other.example.com/frame.html", None));
    }

    #[test]
    fn frame_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(frame_src_blocked(&p, "https://example.com/frame.html", None));
    }

    /// GAP-CSPENF срез 26: `child-src` sits between `frame-src` and
    /// `default-src` in the CSP3 §6.4 fallback chain — an allowing
    /// `child-src` must win over a blocking `default-src` when `frame-src`
    /// itself is absent.
    #[test]
    fn frame_src_falls_back_to_child_src_before_default_src() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'; child-src cdn.example.com");
        assert!(!frame_src_blocked(&p, "https://cdn.example.com/frame.html", None));
        assert!(frame_src_blocked(&p, "https://other.example.com/frame.html", None));
    }

    #[test]
    fn frame_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("frame-src 'none'");
        assert!(!frame_src_blocked(&p, "not a url", None));
    }

    /// GAP-CSPENF срез 17: `media-src` against the shell's `<track src>` fetch.
    #[test]
    fn no_media_src_allows_track_fetch() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!media_src_blocked(&p, "https://example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_none_blocks_track_fetch() {
        let p = lumen_network::csp::parse_csp_header("media-src 'none'");
        assert!(media_src_blocked(&p, "https://example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("media-src cdn.example.com");
        assert!(!media_src_blocked(&p, "https://cdn.example.com/cap.vtt", None));
        assert!(media_src_blocked(&p, "https://other.example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(media_src_blocked(&p, "https://example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("media-src 'none'");
        assert!(!media_src_blocked(&p, "not a url", None));
    }

    /// A stricter sibling directive must not stand in for `media-src`: a page
    /// that locks down `img-src` only has said nothing about its media.
    #[test]
    fn img_src_none_does_not_block_media() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'; media-src *");
        assert!(!media_src_blocked(&p, "https://example.com/cap.vtt", None));
    }

    /// GAP-CSPENF срез 19: `font-src` against `@font-face url()`.
    #[test]
    fn no_font_src_allows_font_fetch() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!font_src_blocked(&p, "https://example.com/font.woff2", None));
    }

    #[test]
    fn font_src_none_blocks_font_fetch() {
        let p = lumen_network::csp::parse_csp_header("font-src 'none'");
        assert!(font_src_blocked(&p, "https://example.com/font.woff2", None));
    }

    #[test]
    fn font_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("font-src cdn.example.com");
        assert!(!font_src_blocked(&p, "https://cdn.example.com/font.woff2", None));
        assert!(font_src_blocked(&p, "https://other.example.com/font.woff2", None));
    }

    #[test]
    fn font_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(font_src_blocked(&p, "https://example.com/font.woff2", None));
    }

    #[test]
    fn font_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("font-src 'none'");
        assert!(!font_src_blocked(&p, "not a url", None));
    }

    /// A stricter sibling directive must not stand in for `font-src`.
    #[test]
    fn media_src_none_does_not_block_font() {
        let p = lumen_network::csp::parse_csp_header("media-src 'none'; font-src *");
        assert!(!font_src_blocked(&p, "https://example.com/font.woff2", None));
    }

    // GAP-CSPENF срез 23: `style-src-attr` against the `style=""` attribute.

    #[test]
    fn no_policy_allows_style_attribute() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!style_attribute_blocked(&p, "color:red"));
    }

    #[test]
    fn style_src_attr_none_blocks_attribute() {
        let p = lumen_network::csp::parse_csp_header("style-src-attr 'none'");
        assert!(style_attribute_blocked(&p, "color:red"));
    }

    #[test]
    fn style_src_attr_unsafe_inline_allows() {
        let p = lumen_network::csp::parse_csp_header("style-src-attr 'unsafe-inline'");
        assert!(!style_attribute_blocked(&p, "color:red"));
    }

    /// `style-src` (no `-attr` split) falls back for the attribute too — CSP3
    /// §6.4's granular chain, one step before `default-src`.
    #[test]
    fn style_src_fallback_allows_attribute() {
        let p = lumen_network::csp::parse_csp_header("style-src 'unsafe-inline'");
        assert!(!style_attribute_blocked(&p, "color:red"));
    }

    #[test]
    fn default_src_fallback_blocks_attribute() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(style_attribute_blocked(&p, "color:red"));
    }

    /// A hash source alone does not allow a `style=""` attribute — CSP3 §8.1
    /// requires `'unsafe-hashes'` alongside it, unlike `<style>` element text
    /// ([`inline_style_blocked`]'s `matching_sha256_hash_allows`-equivalent).
    #[test]
    fn bare_hash_does_not_allow_attribute() {
        let p = lumen_network::csp::parse_csp_header(
            "style-src-attr 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(style_attribute_blocked(&p, "alert(1)"));
    }

    /// `'unsafe-hashes'` plus a matching hash allows it.
    #[test]
    fn unsafe_hashes_plus_matching_hash_allows_attribute() {
        let p = lumen_network::csp::parse_csp_header(
            "style-src-attr 'unsafe-hashes' 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(!style_attribute_blocked(&p, "alert(1)"));
        assert!(style_attribute_blocked(&p, "alert(2)"));
    }

    /// A nonce source never applies to a `style=""` attribute — there is no
    /// attribute to carry one, unlike `<style nonce="…">`.
    #[test]
    fn nonce_source_does_not_allow_attribute() {
        let p = lumen_network::csp::parse_csp_header("style-src-attr 'nonce-abc123'");
        assert!(style_attribute_blocked(&p, "color:red"));
    }
}
