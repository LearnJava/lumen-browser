//! Everything about `<script>` elements on the page-load path: collecting them
//! in document order, resolving and fetching their sources, the import map,
//! the parser-insertion log replayed to `MutationObserver`, and the two
//! executors that hand the collected bodies to the JS runtime.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

/// A `<script>` to execute: either an inline body or an external `src`.
///
/// Produced by [`collect_scripts_ordered`] in document order; external entries
/// are resolved + fetched by [`resolve_script_sources`].
pub(crate) enum ScriptSource {
    /// Inline `<script>` body (concatenated text children) plus the id of the
    /// `<script>` element itself, which backs `document.currentScript` while the
    /// body runs (BUG-486).
    Inline(NodeId, String),
    /// External `<script src="...">` — raw `src` attribute, resolved relative
    /// to the document base, plus the element's id (see [`ScriptSource::Inline`]).
    External(NodeId, String),
}

/// True for `type` values that designate an executable classic script
/// (HTML LS §2.1.5 "JavaScript MIME type"). An absent/empty `type` is classic.
/// Everything else (`module`, `importmap`, `application/json`,
/// `application/ld+json`, `speculationrules`, templates) is data, not code.
pub(crate) fn is_classic_script_type(t: Option<&str>) -> bool {
    match t {
        None => true,
        Some(t) => {
            let t = t.trim();
            t.is_empty()
                || matches!(
                    t.to_ascii_lowercase().as_str(),
                    "text/javascript"
                        | "application/javascript"
                        | "application/ecmascript"
                        | "application/x-ecmascript"
                        | "application/x-javascript"
                        | "text/ecmascript"
                        | "text/javascript1.0"
                        | "text/javascript1.1"
                        | "text/javascript1.2"
                        | "text/javascript1.3"
                        | "text/javascript1.4"
                        | "text/javascript1.5"
                        | "text/jscript"
                        | "text/livescript"
                        | "text/x-ecmascript"
                        | "text/x-javascript"
                )
        }
    }
}

/// Walk the DOM in document order, classifying `<script>` elements into
/// `classic` and `module` execution lists (HTML LS §8.1.3.1). Unlike
/// [`collect_inline_scripts`], external `<script src>` are recorded as
/// [`ScriptSource::External`] so the caller can fetch and execute their bodies
/// (BUG-164). `defer`/`async` are not modelled separately — the shell runs
/// every script synchronously in document order, which matches the eventual
/// classic-then-module execution in [`run_scripts_with_dom`].
pub(crate) fn collect_scripts_ordered(
    doc: &Document,
    id: NodeId,
    classic: &mut Vec<ScriptSource>,
    modules: &mut Vec<ScriptSource>,
) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "script"
    {
        let script_type = node.get_attr("type").map(|t| t.trim());
        let is_module = script_type.is_some_and(|t| t.eq_ignore_ascii_case("module"));
        // Only module + classic-JS scripts execute; everything else is data.
        if !is_module && !is_classic_script_type(script_type) {
            return;
        }
        // `nomodule` (HTML LS §4.12.1): классический скрипт с этим атрибутом —
        // запасная сборка для движка БЕЗ модулей, и движок с модулями обязан её
        // пропустить. Пока не пропускали, сайт с парой module/nomodule получал
        // обе сборки разом: legacy-бандл и современный монтировались в один и
        // тот же корень и гасили друг друга (живой пример — форма входа
        // id.tbank.ru, 2026-08-17).
        if !is_module && node.get_attr("nomodule").is_some() {
            return;
        }
        let target = if is_module { modules } else { classic };
        // `src` wins over inline body (HTML LS §4.12.1 — inline ignored if set).
        if let Some(src) = node.get_attr("src") {
            let src = src.trim();
            if !src.is_empty() {
                target.push(ScriptSource::External(id, src.to_owned()));
            }
            return;
        }
        let mut text = String::new();
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                text.push_str(s);
            }
        }
        if !text.trim().is_empty() {
            target.push(ScriptSource::Inline(id, text));
        }
        return;
    }
    for &child in &node.children {
        collect_scripts_ordered(doc, child, classic, modules);
    }
}

/// Скрипт, готовый к исполнению: тело плюс собственный адрес внешнего файла.
///
/// Адрес нужен только модулям — он служит базой их относительных импортов
/// (`./chunk.js` бандла с CDN обязан резолвиться от CDN, а не от документа).
/// У inline-скриптов его нет.
pub(crate) struct ResolvedScript {
    /// Узел `<script>`, из которого тело взято (для `document.currentScript`).
    pub(crate) node: NodeId,
    /// Исходный текст скрипта.
    pub(crate) source: String,
    /// Абсолютный URL внешнего `<script src>`; `None` у inline и `file://`.
    pub(crate) url: Option<String>,
    /// Исход загрузки внешнего файла: `Some(true)` — тело получено,
    /// `Some(false)` — не получено (в `source` пусто, исполнять нечего),
    /// `None` — скрипт инлайновый, внешнего файла у него нет.
    ///
    /// BUG-804: HTML LS §4.12.1 требует выстрелить `load` на элементе после
    /// исполнения внешнего скрипта и `error` — если файл не пришёл, и делает
    /// это **независимо от того, кто вставил элемент**. Парсерный `<script>`
    /// не проходит через JS-хук вставки (`_lumen_resource_track` знает только
    /// об элементах из `createElement`), поэтому исход его загрузки известен
    /// только здесь — и передаётся на JS-сторону прямо в цикле исполнения,
    /// где порядок «выполнили тело → выстрелили `load`» получается даром.
    /// `None` не диспатчит ничего: у инлайнового скрипта «from an external
    /// file» ложно, и события по спецификации нет вовсе.
    pub(crate) external_ok: Option<bool>,
}

impl ResolvedScript {
    /// Внешний `<script src>`, тело которого получить не удалось.
    ///
    /// Остаётся в списке ровно ради своего `error` (BUG-804): тела нет, так что
    /// цикл исполнения его пропускает, но элемент по HTML LS §4.12.1 обязан
    /// сообщить странице об отказе. Заодно узел остаётся границей отрезка в
    /// [`ParserInsertLog`] — настоящий парсер тоже вставил его в дерево.
    fn failed(node: NodeId) -> Self {
        Self { node, source: String::new(), url: None, external_ok: Some(false) }
    }
}

/// Выстрелить `load`/`error` на элементе `<script>`, который вставил парсер.
///
/// Диспатч синхронный, а не задачей: §4.12.1 стреляет `load` сразу после того,
/// как тело отработало, то есть ДО следующего скрипта документа — страница,
/// которая в следующем же `<script>` читает выставленный обработчиком флаг,
/// обязана его увидеть. Отложить событие задачей значило бы доставить его
/// после всего разбора, что ломает этот порядок.
#[cfg(feature = "v8")]
pub(crate) fn fire_parser_script_event(
    rt: &lumen_js::v8_runtime::V8JsRuntime,
    node: NodeId,
    external_ok: Option<bool>,
) {
    use lumen_core::ext::JsRuntime as _;
    let Some(ok) = external_ok else { return };
    let kind = if ok { "load" } else { "error" };
    let _ = rt.eval(&format!("_lumen_resource_fire({}, '{kind}');", node.index()));
}

/// Журнал вставок, которые сделал парсер, — для `MutationObserver` (BUG-827).
///
/// Шелл разбирает документ целиком и только потом исполняет скрипты, поэтому к
/// моменту, когда страничный `new MutationObserver(…).observe(…)` вообще может
/// быть выполнен, дерево уже построено и «вставлять» нечего — записей о
/// парсерных узлах не возникало ни одной, хотя DOM §4.3 вешает постановку
/// записи на сам шаг «insert a node», а не на конкретный API: узел, написанный
/// парсером, обязан дать `childList`-запись ровно так же, как `appendChild`.
///
/// Журнал восстанавливает тот порядок, в котором потоковый парсер вставлял бы
/// узлы (обход дерева в document order), и режет его границами исполняемых
/// классических `<script>`: перед скриптом K на JS-сторону уходит всё, что
/// настоящий парсер вставил бы до него, включая сам элемент `<script>` и его
/// текст. Остаток документа уходит после последнего классического скрипта —
/// отложенные модули по HTML LS §8.1.3.1 исполняются уже после разбора.
pub(crate) struct ParserInsertLog {
    /// `(родитель, вставленный ребёнок)` в порядке дерева.
    pub(crate) pairs: Vec<(usize, usize)>,
    /// Для каждого исполняемого `<script>` — конец его поддерева в `pairs`.
    pub(crate) script_end: HashMap<NodeId, usize>,
    /// Сколько пар уже отдано (или пропущено) — граница следующего отрезка.
    pub(crate) cursor: usize,
}

impl ParserInsertLog {
    /// Обойти дерево `doc` и запомнить границы поддеревьев узлов `scripts`.
    pub(crate) fn build(doc: &Document, scripts: &[ResolvedScript]) -> Self {
        let mut log = Self { pairs: Vec::new(), script_end: HashMap::new(), cursor: 0 };
        // Без классических скриптов наблюдателя ставить некому: модули по
        // §8.1.3.1 отложены и исполняются, когда парсер уже всё вставил.
        if scripts.is_empty() {
            return log;
        }
        let boundaries: std::collections::HashSet<NodeId> =
            scripts.iter().map(|s| s.node).collect();
        // Сам корень документа ниоткуда не вставляется — начинаем с его детей.
        log.walk(doc, doc.root(), &boundaries);
        log
    }

    fn walk(&mut self, doc: &Document, id: NodeId, boundaries: &std::collections::HashSet<NodeId>) {
        for &child in &doc.get(id).children {
            self.pairs.push((id.index(), child.index()));
            self.walk(doc, child, boundaries);
            if boundaries.contains(&child) {
                self.script_end.insert(child, self.pairs.len());
            }
        }
    }

    /// Граница отрезка: конец поддерева `upto` либо весь остаток при `None`.
    pub(crate) fn segment_end(&self, upto: Option<NodeId>) -> usize {
        match upto {
            Some(n) => self.script_end.get(&n).copied().unwrap_or(self.pairs.len()),
            None => self.pairs.len(),
        }
    }
}

/// Отдать JS-стороне парсерные вставки вплоть до `upto` (см. [`ParserInsertLog`]).
///
/// Наблюдателей нет — строку не строим вовсе: запись, поставленная до
/// `observe()`, всё равно никому не доставляется, а сериализация вставок целого
/// документа не бесплатна. Курсор двигается в обоих случаях.
#[cfg(feature = "v8")]
pub(crate) fn flush_parser_inserts(
    log: &mut ParserInsertLog,
    upto: Option<NodeId>,
    rt: &lumen_js::v8_runtime::V8JsRuntime,
) {
    use lumen_core::ext::JsRuntime as _;
    use std::fmt::Write as _;

    let end = log.segment_end(upto);
    if end <= log.cursor {
        return;
    }
    let observing = matches!(
        rt.eval("_lumen_mo_observing()"),
        Ok(lumen_core::ext::JsValue::Bool(true))
    );
    if observing {
        let mut js = String::with_capacity((end - log.cursor) * 12 + 32);
        js.push_str("_lumen_mo_parser_inserted([");
        for (i, (parent, child)) in log.pairs[log.cursor..end].iter().enumerate() {
            if i > 0 {
                js.push(',');
            }
            let _ = write!(js, "{parent},{child}");
        }
        js.push_str("]);");
        if let Err(e) = rt.eval(&js) {
            eprintln!("MutationObserver: парсерные вставки не доставлены: {e}");
        }
    }
    log.cursor = end;
}

/// Resolve [`ScriptSource`] items to JS source strings in document order,
/// fetching external `<script src>` bodies via the subresource fetcher
/// (mirrors [`load_linked_stylesheets`]). A failed fetch is logged and kept in
/// the list with an empty body and `external_ok: Some(false)` — one broken
/// script must not abort the rest of the page, but it still owes its element an
/// `error` event (BUG-804), so it may not be dropped here.
pub(crate) fn resolve_script_sources(
    items: &[ScriptSource],
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Vec<ResolvedScript> {
    // Внешние `<script src>` грузятся параллельно (сеть — главный тормоз), но
    // результат собирается строго в исходном порядке: классические скрипты
    // обязаны выполняться в порядке документа (HTML LS §8.1.3.1). Inline-тела
    // проходят насквозь без сети.
    let fetched = parallel_map(items, |_, item| match item {
        ScriptSource::Inline(nid, body) => Some(ResolvedScript {
            node: *nid,
            source: body.clone(),
            url: None,
            external_ok: None,
        }),
        ScriptSource::External(nid, src) => match base.resolve(src) {
            ResolvedResource::File(path) => match std::fs::read_to_string(&path) {
                Ok(content) => {
                    eprintln!("Загружен скрипт: {}", path.display());
                    Some(ResolvedScript {
                        node: *nid,
                        source: content,
                        url: None,
                        external_ok: Some(true),
                    })
                }
                Err(e) => {
                    eprintln!("Пропуск скрипта {}: {e}", path.display());
                    Some(ResolvedScript::failed(*nid))
                }
            },
            ResolvedResource::Url(url) => {
                use lumen_core::url::Url;
                use lumen_network::RequestDestination;
                let sub_url = match Url::parse(&url) {
                    Ok(u) => u,
                    Err(e) => {
                        eprintln!("Пропуск скрипта {url}: {e}");
                        return Some(ResolvedScript::failed(*nid));
                    }
                };
                // BUG-171: read through the prefetch cache so a script already
                // warmed by the streaming thread returns instantly instead of
                // blocking the UI thread on the socket. On a miss this fetches the
                // exact same bytes via the same client (script order preserved).
                // PERF-1: one span per external script fetch.
                let mut fetch_span = lumen_core::trace::span(format!("script {url}"), "net");
                let bytes = crate::prefetch::PREFETCH_CACHE.fetch_current(&url, || {
                    let client = base.http_client_for_subresource(sink.clone(), cookie_jar.clone());
                    client
                        .fetch_subresource(&sub_url, RequestDestination::Script)
                        .map_err(|e| e.to_string())
                });
                match bytes {
                    Ok(bytes) => {
                        eprintln!("Загружен скрипт: {url}");
                        fetch_span.set_bytes(bytes.len());
                        Some(ResolvedScript {
                            node: *nid,
                            source: String::from_utf8_lossy(&bytes[..]).into_owned(),
                            // Абсолютный адрес самого скрипта — база
                            // относительных импортов внутри модуля.
                            url: Some(url.clone()),
                            external_ok: Some(true),
                        })
                    }
                    Err(e) => {
                        eprintln!("Пропуск скрипта {url}: {e}");
                        Some(ResolvedScript::failed(*nid))
                    }
                }
            }
        },
    });
    fetched.into_iter().flatten().collect()
}

/// Collect `<script>` elements from the DOM, separating classic from module scripts.
///
/// `scripts` receives classic `<script>` bodies (no `type` attribute, or `type=text/javascript`).
/// `module_scripts` receives `<script type=module>` bodies (HTML LS §8.1.3.1).
/// Both skip `<script src="...">` (external-only) and empty inline bodies.
pub(crate) fn collect_inline_scripts(
    doc: &Document,
    id: NodeId,
    scripts: &mut Vec<String>,
    module_scripts: &mut Vec<String>,
) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "script"
    {
        let script_type = node.get_attr("type").map(|t| t.trim());
        let is_module = script_type.is_some_and(|t| t.eq_ignore_ascii_case("module"));
        let is_importmap = script_type.is_some_and(|t| t.eq_ignore_ascii_case("importmap"));
        // Тот же пропуск `nomodule`, что и в `collect_scripts_ordered`.
        if !is_module && !is_importmap && node.get_attr("nomodule").is_some() {
            return;
        }

        let mut text = String::new();
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                text.push_str(s);
            }
        }
        if !text.trim().is_empty() {
            if is_importmap {
                // Import maps are handled separately by the caller
                // For now, skip them here; caller will collect them separately
            } else if is_module {
                module_scripts.push(text);
            } else {
                scripts.push(text);
            }
        }
        return;
    }
    for &child in &node.children {
        collect_inline_scripts(doc, child, scripts, module_scripts);
    }
}

/// Collect the first `<script type="importmap">` import map from the document.
///
/// Returns the parsed ImportMap if found, or None if not present or invalid JSON.
#[cfg(feature = "v8")]
pub(crate) fn collect_import_map(doc: &Document) -> Option<lumen_js::esm::ImportMap> {
    collect_import_map_impl(doc, doc.root())
}

#[cfg(feature = "v8")]
fn collect_import_map_impl(
    doc: &Document,
    id: NodeId,
) -> Option<lumen_js::esm::ImportMap> {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "script"
    {
        let script_type = node.get_attr("type").map(|t| t.trim());
        let is_importmap = script_type.is_some_and(|t| t.eq_ignore_ascii_case("importmap"));

        if is_importmap {
            let mut text = String::new();
            for &child in &node.children {
                if let NodeData::Text(s) = &doc.get(child).data {
                    text.push_str(s);
                }
            }
            if let Some(map) = lumen_js::esm::ImportMap::parse(&text) {
                return Some(map);
            }
        }
    }
    for &child in &node.children {
        if let Some(map) = collect_import_map_impl(doc, child) {
            return Some(map);
        }
    }
    None
}

/// Выполнить inline `<script>` блоки с DOM-доступом (V8 + install_dom).
///
/// Принимает `doc` по значению, оборачивает в `Arc<Mutex<>>` на время выполнения
/// Выполняет inline `<script>` блоки через V8 (если feature включён),
/// возвращает `(Arc<Mutex<Document>>, Option<JsNavigateRequest>, Option<Arc<dyn PersistentJs>>)`.
///
/// Документ оборачивается в `Arc<Mutex>` чтобы JS-замыкания и layout-код
/// могли разделить доступ без лишних клонов. Persistent runtime возвращается
/// как `PersistentJs` для диспатча событий после загрузки страницы.
///
/// `page_url` пробрасывается в `window.location` (инициализация).
/// `fetch_provider` пробрасывается в `window.fetch()`.
/// `ws_provider` пробрасывается в `new WebSocket(url)`.
/// `sse_provider` пробрасывается в `new EventSource(url)`.
/// `ls_store` — localStorage partition для текущего origin (persists across reloads).
/// `ss_store` — sessionStorage partition вкладки для того же origin (BUG-836):
/// живёт, пока жива вкладка, и переживает смену документа.
/// `None` = no network (sandboxed context или отключён v8 feature).
/// `scripts` / `module_scripts` — уже разрешённые тела classic / module скриптов
/// в порядке документа, включая дозагруженные внешние `<script src>` (BUG-164);
/// собираются вызывающим через [`collect_scripts_ordered`] + [`resolve_script_sources`].
#[allow(clippy::needless_return)] // `return` inside #[cfg] block is needed for correct control flow
#[allow(unused_variables, clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn run_scripts_with_dom(
    doc: Document,
    sandbox: lumen_core::SandboxFlags,
    page_url: &str,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
    sse_provider: Option<Arc<dyn lumen_core::ext::JsSseProvider>>,
    ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    ss_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
    // PH3-20: shared Cache API backend. Passed to `install_dom` so the page's
    // `caches` API and any activating SW execution thread read/write the same
    // store (the SW serves cache-first responses the page previously cached).
    cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
    cookie_banner_dismiss: bool,
    deterministic: deterministic::DetConfig,
    cross_origin_isolated: bool,
    extra_scripts: &[String],
    scripts: Vec<ResolvedScript>,
    module_scripts: Vec<ResolvedScript>,
    // BUG-480 срез 8: создать рантайм даже при отсутствии парсерных скриптов
    // (фреймы: получатель кросс-фреймовых конвертов). Sandbox=SCRIPTS всё
    // равно побеждает — он запрещает исполнение целиком.
    always_runtime: bool,
    // BUG-443: geometry + computed style of the document as it stands right
    // now, pushed into the runtime before the first script line executes.
    // `None` = the caller has no layout to offer (frame/thaw paths), which is
    // the pre-BUG-443 behaviour: a parse-time read answers `""` / a zero rect.
    parse_time_layout: Option<JsLayoutSnapshot>,
) -> (Arc<Mutex<Document>>, Option<JsNavigateRequest>, Option<Arc<dyn PersistentJs>>) {
    // `scripts` / `module_scripts` are already resolved by the caller in
    // document order, including fetched external `<script src>` bodies (BUG-164).
    // Import map must be captured before `doc` moves into the Arc and applied
    // to the runtime before any module evaluation (HTML LS §8.1.6.2).
    #[cfg(feature = "v8")]
    let import_map = collect_import_map(&doc);
    // BUG-827: порядок парсерных вставок надо снять до того, как `doc` уйдет в
    // Arc, — дальше исполнение скриптов уже начнет менять дерево.
    #[cfg(feature = "v8")]
    let mut parser_inserts = ParserInsertLog::build(&doc, &scripts);

    let doc_arc = Arc::new(Mutex::new(doc));

    if !always_runtime && scripts.is_empty() && module_scripts.is_empty() && extra_scripts.is_empty()
    {
        return (doc_arc, None, None);
    }
    if sandbox.contains(lumen_core::SandboxFlags::SCRIPTS) {
        eprintln!(
            "sandbox: заблокировано {} скрипт(ов) + {} модул(ей) (sandbox=scripts)",
            scripts.len(), module_scripts.len()
        );
        return (doc_arc, None, None);
    }

    // Ph3 V8 migration S4. Since S12b-23 the import map and `eval_module` are
    // wired here too; since S12b-G6 (BUG-548) `set_cookie_banner_dismiss` is
    // wired too.
    #[cfg(feature = "v8")]
    {
        use lumen_core::ext::JsRuntime as _;
        match lumen_js::v8_runtime::V8JsRuntime::new() {
            Ok(mut rt) => {
                rt.set_cookie_banner_dismiss(cookie_banner_dismiss);
                if deterministic.enabled {
                    rt.set_deterministic_mode(true, deterministic.rng_seed, deterministic.monotonic_clock);
                }
                if let Some(store) = sw_worker_store {
                    rt = rt.with_sw_worker_store(store);
                }
                // BUG-836: the tab owns sessionStorage, not the document.
                if let Some(store) = ss_store {
                    rt = rt.with_session_storage(store);
                }
                if let Err(e) = rt.install_dom(Arc::clone(&doc_arc), page_url, fetch_provider, ws_provider, sse_provider, ls_store, idb_backend, sw_backend, cache_backend, None, cross_origin_isolated) {
                    eprintln!("JS DOM init failed: {e}");
                }
                // Must precede module evaluation: bare specifiers resolve
                // through the map (HTML LS §8.1.6.2).
                if let Some(map) = import_map {
                    rt.set_import_map(map);
                }
                // BUG-443: publish the parse-time layout before the first
                // script line runs. Without it `getComputedStyle` answers `""`
                // and `getBoundingClientRect` a zero rect for every read made
                // during parsing — an inline `<script>` or a
                // `DOMContentLoaded` handler, i.e. where most page code
                // initializes. The tables are the same four
                // `apply_loaded_page` pushes after the load completes.
                if let Some(snap) = parse_time_layout {
                    rt.update_layout_rects(snap.rects);
                    rt.update_hit_test_tree(snap.tree);
                    rt.update_computed_styles(snap.styles);
                    rt.update_custom_properties(snap.customs);
                    rt.update_viewport_size(snap.viewport.0, snap.viewport.1);
                }
                // BUG-839: hand over the subresource loads that already
                // finished, before the page's first script runs. The document's
                // stylesheets and scripts are fetched *during parsing*, i.e.
                // before this runtime exists, and WPT's
                // `performance-timeline/case-sensitivity.any.js` reads
                // `getEntriesByType('resource')` synchronously at the top of
                // that first script — the shell's once-per-event-loop-step
                // drain is far too late for it. That drain still covers the
                // tail (images, anything started later); this take is
                // unconditional because the suspend flag exists to keep those
                // very rows away from the *outgoing* document, and this caller
                // is the incoming one.
                if let Some(json) = crate::resource_timing::rows_to_json(
                    &crate::resource_timing::take_rows_unconditionally(),
                ) {
                    let _ = rt.eval(&format!(
                        "_lumen_deliver_resource_timings({})",
                        js_string_literal(&json)
                    ));
                }
                // Classic scripts run first (HTML LS §8.1.3 execution order).
                for ResolvedScript { node: nid, source: src, external_ok, .. } in &scripts {
                    // BUG-827: к этому моменту настоящий парсер уже вставил всё,
                    // что стоит в документе выше этого скрипта, и сам его
                    // элемент — наблюдатель, поставленный предыдущим скриптом,
                    // обязан увидеть эти вставки записями.
                    flush_parser_inserts(&mut parser_inserts, Some(*nid), &rt);
                    // BUG-804: внешний файл не пришёл — исполнять нечего, но
                    // элемент обязан сообщить об отказе на своём месте в
                    // порядке документа.
                    if *external_ok == Some(false) {
                        fire_parser_script_event(&rt, *nid, *external_ok);
                        continue;
                    }
                    // BUG-486: `document.currentScript` must name the element
                    // being executed for the whole body and nothing else, so the
                    // push/pop pair brackets the eval — including the error paths
                    // below, or one throwing script would leave a stale value
                    // behind for every script after it.
                    let _ = rt.eval(&format!("_lumen_push_current_script({});", nid.index()));
                    // eval_and_report (not the plain trait eval()) — this is
                    // the genuine top-level page-script execution boundary,
                    // so an uncaught exception must also reach the page's own
                    // window 'error'/onerror listeners (BUG-591), not just
                    // this stderr line.
                    match rt.eval_and_report(src) {
                        Ok(_) => {}
                        Err(lumen_core::JsError::NotImplemented) => {
                            eprintln!(
                                "script: engine=v8, выполнение пропущено ({} байт)",
                                src.len()
                            );
                        }
                        Err(e) => eprintln!("script error: {e}"),
                    }
                    let _ = rt.eval("_lumen_pop_current_script();");
                    // §4.12.1 «execute the script block», последний шаг:
                    // внешний классический скрипт стреляет `load` сразу после
                    // тела. Инлайновый — ничего (`external_ok` = `None`).
                    fire_parser_script_event(&rt, *nid, *external_ok);
                }
                // BUG-827: хвост документа парсер вставил ещё до того, как
                // отложенные модули начали исполняться, — отдаём его одним
                // отрезком здесь, пока наблюдатель последнего классического
                // скрипта ещё может его услышать.
                flush_parser_inserts(&mut parser_inserts, None, &rt);
                // Module scripts run after classic scripts (HTML LS §8.1.3.1 deferred).
                // No `currentScript` bracket: it is `null` inside a module by spec.
                for item in &module_scripts {
                    // BUG-804: внешний модуль, чей файл не пришёл, обязан
                    // выстрелить `error` ровно так же, как классический.
                    if item.external_ok == Some(false) {
                        fire_parser_script_event(&rt, item.node, item.external_ok);
                        continue;
                    }
                    let src = &item.source;
                    // Внешний модуль исполняется под СВОИМ адресом: от него
                    // считаются его относительные импорты. У inline-модуля
                    // адреса нет — база остаётся адресом страницы.
                    // eval_module_at_and_report/eval_module_and_report (not the
                    // plain trait methods) — this is the top-level page-script
                    // boundary, so a runtime error in the module body must also
                    // reach window 'error'/onerror (BUG-591); a load/link
                    // failure stays unreported here (belongs to the script
                    // element's own 'error' event instead).
                    let outcome = match &item.url {
                        Some(url) => rt.eval_module_at_and_report(url, src),
                        None => rt.eval_module_and_report(src),
                    };
                    match outcome {
                        Ok(()) => {}
                        Err(lumen_core::JsError::NotImplemented) => {
                            eprintln!(
                                "module: engine=v8, выполнение пропущено ({} байт)",
                                src.len()
                            );
                        }
                        Err(e) => eprintln!("module error: {e}"),
                    }
                    // BUG-804: внешний модуль стреляет `load` после вычисления
                    // — включая случай, когда тело бросило: исключение уходит в
                    // window `error` (BUG-591), а элемент всё равно сообщает об
                    // успешной загрузке. Остаток: провал СВЯЗЫВАНИЯ (не нашёлся
                    // импорт внутри) по спецификации должен дать `error`, но
                    // `ModuleFailure` до сюда не доходит — `JsResult` его
                    // схлопывает, и здесь тоже выйдет `load`.
                    fire_parser_script_event(&rt, item.node, item.external_ok);
                }
                // Extension content scripts run last (after all page scripts).
                for src in extra_scripts {
                    match rt.eval(src) {
                        Ok(_) => {}
                        Err(lumen_core::JsError::NotImplemented) => {
                            eprintln!(
                                "extension: engine=v8, выполнение пропущено ({} байт)",
                                src.len()
                            );
                        }
                        Err(e) => eprintln!("extension script error: {e}"),
                    }
                }
                let nav_req = rt.take_navigate_request().map(|r| match r {
                    lumen_js::NavigateRequest::Push(u)    => JsNavigateRequest::Push(u),
                    lumen_js::NavigateRequest::Replace(u) => JsNavigateRequest::Replace(u),
                    lumen_js::NavigateRequest::Reload     => JsNavigateRequest::Reload,
                    lumen_js::NavigateRequest::SubmitForm { form, submitter } =>
                        JsNavigateRequest::SubmitForm { form, submitter },
                });
                // Keep rt alive: return as PersistentJs so event handlers work after load.
                let ctx: Arc<dyn PersistentJs> = Arc::new(V8PersistentJs { rt });
                return (doc_arc, nav_req, Some(ctx));
            }
            Err(e) => {
                eprintln!("V8 init failed: {e}");
                return (doc_arc, None, None);
            }
        }
    }

    #[cfg(not(feature = "v8"))]
    {
        let _ = page_url;
        let _ = fetch_provider;
        let _ = ws_provider;
        let _ = sse_provider;
        use lumen_core::ext::JsRuntime as _;
        for (_, src) in &scripts {
            match lumen_core::NullJsRuntime.eval(src) {
                Ok(_) => {}
                Err(lumen_core::JsError::NotImplemented) => {
                    eprintln!(
                        "script: engine=null, выполнение пропущено ({} байт)",
                        src.len()
                    );
                }
                Err(e) => eprintln!("script error: {e}"),
            }
        }
        (doc_arc, None, None)
    }
}

/// Выполнить inline `<script>` блоки если sandbox позволяет, иначе заблокировать.
///
/// `SandboxFlags::SCRIPTS` установлен — скрипты запрещены; функция логирует
/// количество заблокированных и возвращает 0. Иначе каждый скрипт передаётся
/// в `runtime.eval()`; без feature `v8` это NullJsRuntime → `NotImplemented`.
/// Возвращает число скриптов, переданных в runtime.
#[cfg(test)]
pub(crate) fn run_scripts(
    doc: &Document,
    sandbox: lumen_core::SandboxFlags,
    runtime: &dyn lumen_core::JsRuntime,
) -> usize {
    let mut scripts: Vec<String> = Vec::new();
    let mut _module_scripts: Vec<String> = Vec::new();
    collect_inline_scripts(doc, doc.root(), &mut scripts, &mut _module_scripts);
    if scripts.is_empty() {
        return 0;
    }
    if sandbox.contains(lumen_core::SandboxFlags::SCRIPTS) {
        eprintln!(
            "sandbox: заблокировано {} скрипт(ов) (sandbox=scripts)",
            scripts.len()
        );
        return 0;
    }
    for src in &scripts {
        match runtime.eval(src) {
            Ok(_) => {}
            Err(lumen_core::JsError::NotImplemented) => {
                eprintln!(
                    "script: engine={}, выполнение пропущено ({} байт)",
                    runtime.engine_name(),
                    src.len()
                );
            }
            Err(e) => {
                eprintln!("script error: {e}");
            }
        }
    }
    scripts.len()
}
