//! `spawn_frame`: loading ONE nested sub-document — the body of the
//! `load_frame_sub_documents` loop shared with frame navigation (slice 19).
//!
//! Moved verbatim out of `frames.rs` (file-size gate, docs/lint-policy.md
//! §5.1). A child module of `frames`, so `use super::*;` sees the private
//! helpers it calls exactly as before; `frames` re-exports `spawn_frame` so
//! every `frames::spawn_frame` path keeps resolving.

use super::*;

/// Загрузить ОДИН под-документ и вернуть его хэндл вместе с хэндлами его
/// вложенных фреймов (вложенные идут перед ним — порядок исходного цикла).
///
/// Выделено из тела цикла [`load_frame_sub_documents`] срезом 19: навигация
/// фрейма — тот же самый путь, отличающийся ровно одним, откуда взят адрес.
///
/// `dest`: `None` — адрес из разметки (`srcdoc`/`src`); `Some((href, base))` —
/// навигация, где `href` разрешается относительно базы СТАРОГО под-документа
/// (ссылку резолвит документ, в котором по ней кликнули), а не относительно
/// документа-хозяина, и где `srcdoc` уже ни при чём: элемент показывает
/// результат навигации, а не свою разметку.
///
/// `uir_override` — GAP-CSPENF срез 55: `None` — UIR-заголовок решает
/// `csp_gate` ХОЗЯИНА (`parent`, вычислен ниже), корректно для инициации
/// СВЕРХУ (первичная вставка, `<a target=имя_фрейма>`/переприсваивание
/// `.src` документом, который и есть хозяин целевого `<iframe>`) —
/// `maybe_upgrade_frame_src` уже опирается на тот же `csp_gate` для того же
/// множества путей. `Some(flag)` — вызывающая сторона уже прочитала политику
/// НАСТОЯЩЕГО инициатора и он не совпадает с хозяином: единственный
/// сегодняшний случай — ссылка ВНУТРИ самого фрейма (`frame_links.rs`),
/// решает `navigate-to` РЕБЁНКА, а не хозяина цели, той же причиной, что
/// срез 53 уже разводит источники для апгрейда схемы
/// (`resolve_and_upgrade_frame_href`).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
#[allow(clippy::unwrap_used)] // короткий лок дерева; poisoned mutex = паника потока загрузки, docs/lint-policy.md §10
pub(crate) fn spawn_frame(
    info: &lumen_dom::IframeInfo,
    dest: Option<(&str, &ResourceBase)>,
    parent: &Arc<Mutex<Document>>,
    depth: usize,
    base: &ResourceBase,
    top_doc: &Arc<Mutex<Document>>,
    env: &FrameLoadEnv,
    parent_js: Option<&Arc<dyn PersistentJs>>,
    uir_override: Option<bool>,
    nav_body: Option<&lumen_network::NavigationBody>,
) -> Vec<FrameHandle> {
    // URL родителя и верха для фасадов location/URL у предков (срез 3).
    let parent_url = base_url_string(base);
    let top_url = base_url_string(&env.page_base);
    let sink = &env.sink;
    let cookie_jar = env.cookie_jar.clone();
    let mut handles = Vec::new();
    // GAP-NAVCTX срез 6 (BUG-884): `javascript:` в `src` разметки при
    // первичной вставке host-элемента — парсерной (`load_frame_sub_documents`)
    // и через `document.createElement('iframe')` с сразу выставленным `src`
    // (`frame_dynamic_load.rs::run_new_frame_load`) — оба идут через
    // `dest: None`, исполняется ДО обычного пути `fetch_iframe_source`, а не
    // отклоняется как неподдерживаемая схема.
    //
    // GAP-NAVCTX срез 7 (BUG-884): `dest: Some((href, _))` — навигация
    // (клик/скрипт), включая ПЕРЕПРИСВАИВАНИЕ `.src` уже вставленного элемента
    // (`frame_dynamic.rs::poll_dynamic_frames`'s `delta.changed` →
    // `navigate_frame_to` → `run_frame_navigation`). Срез 6 предполагал, что
    // этот путь выполняется на фоновом потоке БЕЗ доступа к `parent_js` — это
    // было неверно: `FrameNavPrep::parent_js` уже несёт `Arc<dyn PersistentJs>`
    // через `std::thread::spawn` в `replace_frame_document`, и сама реализация
    // (`V8JsRuntime::run`, `crates/js/src/v8_runtime/runtime.rs`) тоннелирует
    // каждый вызов через `SyncSender` на выделенный JS-поток — она безопасна
    // с ЛЮБОГО вызывающего потока по конструкции, а не только с UI-потока
    // среза 6. Настоящая причина, по которой срез 6 это отложил, не
    // подтвердилась чтением кода.
    let js_url_result = match dest {
        // OBJECT-1 срез 2: `<object data="javascript:…">` не исполняется.
        None if info.embedded => None,
        None if info.srcdoc.is_none() => info
            .src
            .as_deref()
            .and_then(javascript_url_code)
            .map(|code| eval_iframe_javascript_url(code, parent_js)),
        Some((href, _)) => {
            javascript_url_code(href).map(|code| eval_iframe_javascript_url(code, parent_js))
        }
        None => None,
    };
    // GAP-NAVCTX срез 7 (BUG-884): не-строковое завершение `javascript:` при
    // НАВИГАЦИИ (`dest.is_some()`, в отличие от первичной вставки) — по HTML
    // LS §7.4.5 не навигация вовсе. Для первичной вставки «остаться на
    // прежнем документе» и «остаться на пустом `about:blank`» — одно и то же
    // (см. ветку `Some(None)` ниже), но здесь под старым документом уже есть
    // реальное состояние (свой JS-контекст, возможно, свои вложенные фреймы),
    // которое падение в тот же `Some(None)`-путь заменило бы на новый пустой
    // документ. Пустой `Vec` — тот же сигнал «навигация отклонена», который
    // `apply_frame_navigation` уже понимает для generation-гонки (см. его
    // doc-comment); `navigate_frame_to` в ответ просто не трогает историю.
    if dest.is_some() && matches!(js_url_result, Some(None)) {
        return Vec::new();
    }
    // GAP-CSPENF срез 15: `frame-src`/`default-src` против навигации этого
    // `<iframe>`/`<frame>` — считана один раз (та же форма, что срезы 4/9
    // уже дают `img-src`), проверяется ниже перед КАЖДЫМ реальным фетчем
    // (первичная вставка и навигация — оба пути идут через
    // `fetch_iframe_source`). `about:blank`/пустой `src` не проверяются:
    // CSP3 §6.5 их не ограничивает, они не долетают ни до сети, ни до диска.
    let (csp_gate, referrer_policy) = {
        let doc = parent.lock().unwrap();
        let root = doc.root();
        (
            crate::csp_enforce::document_csp_policy(&doc, root),
            // GAP-REFERRER срез 4: the `<iframe src>` navigation carries the
            // PARENT document's resolved referrer policy — same one-shot
            // lock as `csp_gate` above, same reasoning as `<link>`/`@import`
            // (`stylesheets.rs`) in this срез.
            crate::resource_base::document_referrer_policy(&doc),
        )
    };
    // GAP-REFERRER срез 6: a `referrerpolicy` attribute on the host element
    // overrides the document policy for this element's own request only
    // (spec §6.6 "referrer policy attribute") — parsed here rather than
    // in `lumen-dom` since only this crate depends on `lumen-network`.
    let referrer_policy = info
        .referrer_policy
        .as_deref()
        .and_then(lumen_network::ReferrerPolicy::parse)
        .unwrap_or(referrer_policy);
    let self_origin = base.origin();
    // GAP-CSPENF срез 55: `uir_override` побеждает, когда вызывающая сторона
    // уже прочитала политику настоящего инициатора (см. doc-comment функции);
    // иначе — тот же `csp_gate` выше, которым уже пользуется
    // `maybe_upgrade_frame_src`.
    let send_uir_header = uir_override
        .unwrap_or_else(|| crate::csp_enforce::navigation_wants_uir_header(csp_gate.as_ref()));
    let frame_src_check = |src: &str, resolve_base: &ResourceBase| -> Option<FetchError> {
        let lowered = src.trim_start().to_ascii_lowercase();
        if lowered.is_empty() || lowered.starts_with("about:") {
            return None;
        }
        let (policy, _) = csp_gate.as_ref()?;
        let resolved = resolve_base.resolve_str(src);
        // Срез 56/58: `originalPolicy` — текст КАЖДОЙ нарушенной политики
        // (CSP3 §7.8/§3.4), блокировка остаётся однократной.
        let violated = crate::csp_enforce::violating_fetch_policy_via_child_src(
            policy,
            &lumen_network::csp::CspDirective::FrameSrc,
            &resolved,
            self_origin.as_ref(),
        );
        if violated.is_empty() {
            return None;
        }
        if let Some(js) = parent_js {
            for policy_text in &violated {
                js.fire_csp_violation("frame-src", &resolved, policy_text);
            }
        }
        Some(FetchError {
            reason: format!("frame-src запрещает '{resolved}'"),
            attempted_url: resolved,
        })
    };
    // OBJECT-1 срез 2: `<object>`/`<embed>` получают вложенный документ,
    // только если ответ им оказался (HTML LS §4.8.6/§4.8.7). Иначе фрейма нет
    // вовсе — ни страницы ошибки, ни `about:blank`: картинку рисует image-
    // конвейер, прочее — fallback `<object>`. Гейт — `object-src`, а не
    // `frame-src`; нарушение сообщает JS-шим (`_lumen_embed_object_scan`).
    let mut child_is_svg = false;
    let embedded_source = if info.embedded && js_url_result.is_none() {
        let (src, resolve_base) = match dest {
            Some((href, nav_base)) => (href, nav_base),
            None => (info.src.as_deref().unwrap_or(""), base),
        };
        let upgraded = maybe_upgrade_frame_src(csp_gate.as_ref(), src, resolve_base);
        let blocked = csp_gate.as_ref().is_some_and(|(policy, _)| {
            crate::csp_enforce::object_src_blocked(
                policy,
                &resolve_base.resolve_str(&upgraded),
                self_origin.as_ref(),
            )
        });
        let source = if blocked {
            None
        } else {
            fetch_embedded_source(
                &upgraded,
                resolve_base,
                sink,
                cookie_jar.clone(),
                send_uir_header,
                referrer_policy,
            )
        };
        // Вердикт пишется за сам атрибут (первичная вставка или его смена),
        // не за ссылку, кликнутую внутри уже показанного документа: layout
        // решает по `data`/`src`, а повторный скан фреймов по нему же
        // перестаёт предлагать «не документ» к загрузке.
        // SVG-документ фрейм-бокса не получает (вердикт «не фрейм»): его
        // рисует image-конвейер, а под-документ остаётся для скриптов.
        let attr_driven = dest.is_none_or(|(href, _)| info.src.as_deref() == Some(href));
        if attr_driven && let Some(raw) = info.src.as_deref() {
            let framed = source.as_ref().is_some_and(|(_, k)| *k != EmbeddedResourceKind::Svg);
            parent.lock().unwrap().set_embedded_document(info.node, raw, framed);
        }
        let Some((source, kind)) = source else { return Vec::new() };
        child_is_svg = kind == EmbeddedResourceKind::Svg;
        Some(source)
    } else {
        None
    };
    // Источник HTML + база ребёнка для его относительных URL.
    let fetched = if let Some(source) = embedded_source {
        Some(Ok(source))
    } else {
        match &js_url_result {
            // Строковое завершение — новый документ фрейма, тем же путём, что и
            // любой другой инлайн-источник (`about:blank`-адрес: `javascript:`
            // не создаёт запись истории, HTML LS §7.4.5).
            Some(Some(html)) => Some(Ok(FrameSource::Inline(html.clone()))),
            // Не-строковое завершение — по спеке НЕ навигация: фрейм остаётся на
            // прежнем документе (для первичной вставки — на пустом `about:blank`,
            // как если бы `src` не было вовсе), код уже отработал побочные эффекты.
            Some(None) => None,
            // GAP-CSPENF срез 52: `src`/`href` апгрейжены
            // (`upgrade_navigation_url`, UIR §4.1 шаг 5) ДО `frame_src_check` —
            // тот же порядок, что и у `frame-src` (шаг 6) выше по этой дорожке.
            // `maybe_upgrade_frame_src` оставляет пустой/`about:`/`data:`/
            // `javascript:` src нетронутым: и `frame_src_check`, и
            // `fetch_iframe_source` сами решают, что с ним делать (пустой/
            // `about:blank` — молча пустой документ, не сеть), а резолв в
            // абсолютный `http(s)`-адрес превратил бы пустую строку в адрес
            // РОДИТЕЛЯ (`ResourceBase::resolve("")` возвращает саму базу) — фрейм
            // бы засетевился на страницу-хозяина вместо пустого документа.
            None => match dest {
                Some((href, nav_base)) => {
                    let href = maybe_upgrade_frame_src(csp_gate.as_ref(), href, nav_base);
                    Some(
                        frame_src_check(&href, nav_base)
                            .map(Err)
                            .unwrap_or_else(|| {
                                fetch_iframe_source_with_body(
                                    &href,
                                    nav_body,
                                    nav_base,
                                    sink,
                                    cookie_jar.clone(),
                                    send_uir_header,
                                    referrer_policy,
                                )
                            }),
                    )
                }
                None if info.srcdoc.is_some() => None,
                None => info.src.as_deref().map(|src| {
                    let src = maybe_upgrade_frame_src(csp_gate.as_ref(), src, base);
                    frame_src_check(&src, base)
                        .map(Err)
                        .unwrap_or_else(|| {
                            fetch_iframe_source(
                                &src,
                                base,
                                sink,
                                cookie_jar.clone(),
                                send_uir_header,
                                referrer_policy,
                            )
                        })
                }),
            },
        }
    };
    // FRAME-4 срез 2: источник, который получить не удалось, больше не
    // обрывает загрузку фрейма — вместо неё под-документом становится
    // синтетическая страница ошибки, тем же путём (parse+layout+paint), что и
    // любой другой под-документ; `load_failed` — единственная разница,
    // видимая остальному коду (история, диагностика).
    let (html, child_base, child_url, load_failed): (String, ResourceBase, String, bool) = match fetched
    {
        Some(Ok(FrameSource::Inline(html))) => (html, base.clone(), "about:blank".to_owned(), false),
        Some(Ok(FrameSource::File { html, path })) => {
            let url = path_to_file_url(&path);
            (html, ResourceBase::File(path), url, false)
        }
        Some(Ok(FrameSource::Url { html, url })) => {
            (html, ResourceBase::Url(url.clone()), url, false)
        }
        Some(Err(e)) => (
            frame_error_document(&e.attempted_url, &e.reason),
            base.clone(),
            e.attempted_url,
            true,
        ),
        None => match &info.srcdoc {
            Some(srcdoc) => (srcdoc.clone(), base.clone(), "about:srcdoc".to_owned(), false),
            // Ни src, ни srcdoc — спека грузит about:blank немедленно.
            None => (String::new(), base.clone(), "about:blank".to_owned(), false),
        },
    };

    let mut child_doc = {
        let _s = lumen_core::trace::span("parse-html-frame", "parse");
        if child_is_svg {
            // OBJECT-1 срез 5: тот же XML-разбор, что у SVG верхнего уровня
            // (`is_xml_flavoured_document`), и тип, по которому
            // `getSVGDocument()` отличает SVG-документ от HTML.
            let mut d = lumen_html_parser::parse_xml_flavoured(&html);
            d.set_content_type("image/svg+xml".to_owned());
            d
        } else {
            lumen_html_parser::parse(&html)
        }
    };
    // BUG-1231: `document.referrer` ребёнка — то же значение, что ушло в
    // `Referer` запроса `<iframe src>` (`compute_referrer` от URL родителя
    // по политике выше). Только для сетевого источника: у file:/about:/srcdoc
    // запрос не шёл.
    if !load_failed
        && child_url != "about:blank"
        && child_url != "about:srcdoc"
        && let ResourceBase::Url(parent_url) = base
        && let Ok(parent_parsed) = lumen_core::url::Url::parse(parent_url)
        && let Ok(child_parsed) = lumen_core::url::Url::parse(&child_url)
    {
        child_doc.set_document_referrer(lumen_network::referrer_policy::compute_referrer(
            referrer_policy,
            &parent_parsed,
            &child_parsed,
        ));
    }
    // СРЕЗ 11 BUG-480: подресурсы парсерных элементов ребёнка (`<img src>`,
    // `<link rel=stylesheet>`). Сеть стартует ДО скриптов — парсерный порядок
    // (источник запроса — шаг разбора, а не исполнение); исходы держим до
    // создания рантайма и доставляем ниже, между DCL и window load.
    let subresources = {
        let _s = lumen_core::trace::span("fetch-frame-subresources", "net");
        fetch_frame_subresources(
            &mut child_doc,
            &child_base,
            sink,
            cookie_jar.clone(),
            &env.media_ctx,
            env.viewport,
            env.target,
            self_origin.as_ref(),
        )
    };
    // GAP-CSPENF срез 27: the child's own `frame-ancestors` directive
    // refused this embedder (`self_origin` above — the PARENT's own origin,
    // the same value `frame_src_check` already uses as the embedder side of
    // a CSP comparison). Same synthetic error page the `Some(Err(e))` fetch
    // failure branch above uses — no scripts/images/links to salvage, since
    // `fetch_frame_subresources` short-circuited before touching the
    // network for any of them.
    if subresources.frame_ancestors_blocked {
        child_doc = lumen_html_parser::parse(&frame_error_document(
            &child_url,
            "frame-ancestors запрещает встраивание этим родителем",
        ));
    }
    // Скрипты ребёнка собираются и (внешние) скачиваются ДО передачи
    // документа в рантайм: run_scripts_with_dom принимает doc по значению.
    let (classic_scripts, deferred_scripts) = {
        let mut classic_items = Vec::new();
        let mut deferred_items = Vec::new();
        collect_scripts_ordered(&child_doc, child_doc.root(), &mut classic_items, &mut deferred_items);
        (
            resolve_script_sources(&classic_items, &child_base, sink, cookie_jar.clone(), &child_doc),
            resolve_script_sources(&deferred_items, &child_base, sink, cookie_jar.clone(), &child_doc),
        )
    };
    // Opaque origin (sandbox без allow-same-origin) — без персистентных
    // хранилищ; провайдеры сети остаются: sandbox режет origin-доступ,
    // а не сеть (скрипты целиком гейтятся флагом SCRIPTS отдельно).
    let opaque = info.is_sandboxed && info.sandbox.contains(lumen_core::SandboxFlags::ORIGIN);
    // BUG-1198: доступность в обе стороны считается ДО скриптов ребёнка —
    // по ней [`crate::frame_ancestry::FrameAncestry`] регистрирует предков и сам биндинг ребёнка
    // раньше первой строки его скрипта.
    let accessible_parent = frame_access_allowed(base, &child_url, opaque);
    let accessible_top = frame_access_allowed(&env.page_base, &child_url, opaque);
    // BUG-921: снимок атрибута `name` хоста на момент создания контекста —
    // `window.name` ребёнка запоминает его один раз (HTML LS §7.2.3), а не
    // перечитывает атрибут при каждом обращении.
    // BUG-979: peer — родитель, only когда сам доступен same-origin
    // (`accessible_parent`) — глобалы читаются исключительно same-origin,
    // натив ещё раз гейтит это явно, но не полагаться на второй слой
    // защиты, когда первый доступен бесплатно.
    let parent_peer = accessible_parent.then(|| parent_js.and_then(|js| js.frame_peer_bridge())).flatten();
    let ancestry = crate::frame_ancestry::FrameAncestry {
        parent_js,
        host_nid: info.node.index() as u32,
        name: info.name.as_deref(),
        child_url: &child_url,
        // Для биндинга ребёнка в реестре РОДИТЕЛЯ доступность та же, что
        // у родителя для ребёнка: same-origin симметричен.
        accessible: accessible_parent,
        opaque,
        parent_doc: parent,
        parent_url: &parent_url,
        parent_peer,
        // Ребёнок глубины ≥ 1 получает отдельный слот top: его верх —
        // корень страницы, а не непосредственный родитель.
        // BUG-979: top's own runtime is not reachable here (only its doc
        // Arc is threaded down through `top_doc`) — `window.top`'s facade
        // keeps the IDL-only whitelist for now; scope stays contentWindow/
        // parent, the shapes this bug's WPT repro actually exercises.
        top: (depth >= 1).then_some((top_doc, top_url.as_str(), accessible_top)),
    };
    let image_hook = env.image_hook_channel.as_ref().map(|(generation, proxy)| {
        crate::dynamic_image_hook::DynamicImgFetchHook::for_document(
            &child_doc,
            child_base.clone(),
            Arc::clone(sink),
            cookie_jar.clone(),
            env.target,
            (env.viewport.width, env.viewport.height),
            crate::dynamic_image_hook::DynamicImageHookCtx {
                generation: *generation,
                dedup: Arc::default(),
                proxy: proxy.clone(),
            },
        )
    });
    let _runtime_span = lumen_core::trace::span("frame-runtime", "js");
    let (child_doc_arc, child_nav, child_js) = run_scripts_with_dom(
        child_doc,
        info.sandbox,
        &child_url,
        env.fetch_provider.clone(),
        env.ws_provider.clone(),
        env.sse_provider.clone(),
        env.ls_store.clone().filter(|_| !opaque),
        env.ss_store.clone().filter(|_| !opaque),
        env.idb_backend.clone().filter(|_| !opaque),
        env.sw_backend.clone().filter(|_| !opaque),
        env.sw_worker_store.clone().filter(|_| !opaque),
        env.cache_backend.clone().filter(|_| !opaque),
        env.push_backend.clone().filter(|_| !opaque),
        env.cookie_banner_dismiss,
        env.deterministic,
        env.cross_origin_isolated,
        &[],
        classic_scripts,
        deferred_scripts,
        // BUG-480 срез 8: фрейму рантайм нужен даже без единого парсерного
        // скрипта — иначе ему нечем принимать кросс-фреймовые postMessage/
        // события/RunScript (срезы 4–8), а статические iframe — самый
        // частый встраиваемый случай. Странице (второй вызов) хватает
        // старого поведения: без скриптов ей нечем отвечать.
        // BUG-1011: синтетическая страница ошибки (отклонённый `data:`/
        // `javascript:`/сетевой отказ) не несёт ни скриптов, ни собеседника для
        // postMessage — рантайм со всем шим-набором (~80 мс) там чистая трата;
        // 216 таких фреймов в `svg-embedded-sizing` упирались в таймаут теста.
        !load_failed,
        // BUG-443: a sub-document is laid out only after this call returns
        // (`layout_frame_document`), so there is no parse-time layout to offer.
        None,
        // CSSOM-1 срез 3: sub-documents don't build the per-node stylesheet
        // registry yet (mirrors the empty `stylesheet_nodes` bfcache/docking/
        // hibernation already fall back to) — a frame's `document.styleSheets`
        // reads as empty until a future slice wires this up.
        Vec::new(),
        // CSSOM-7 (BUG-977): mirrors the `None` `parse_time_layout` above —
        // no layout to offer yet, so nothing to flush against either.
        None,
        // BUG-1148: frame scripts get the immediate-`<img>` hook too, bound to
        // the frame's own base URL.
        image_hook,
        // BUG-1119: the frame's `document.cookie` uses the jar its requests
        // go through; an opaque origin has no cookies (HTML LS §3.1.3).
        env.cookie_jar.clone().filter(|_| !opaque),
        Some(&ancestry),
    );
    drop(_runtime_span);
    // PERF-14: same headless settle the page gets after its own scripts.
    crate::page_pipeline::settle_headless_fetches(child_js.as_ref());
    // Навигация из скриптов ребёнка (location.href= и т.п.) вне среза 1:
    // отклоняем с логом, не заваливая страницу.
    if let Some(nav) = child_nav {
        let target = match nav {
            JsNavigateRequest::Push(url) | JsNavigateRequest::Replace(url) => url,
            _ => "<reload/submit>".to_owned(),
        };
        eprintln!("iframe: навигация из под-документа ({child_url}) не поддерживается (BUG-480 срез 1), запрос '{target}' отклонён");
    }
    // Ссылки на предков в контексте ребёнка (срез 3 BUG-480) уже стоят:
    // `run_scripts_with_dom` ставит их через `ancestry` до первой строки
    // скрипта ребёнка (BUG-1198).
    // BUG-480 срез 12: cascade + layout ребёнка — контентная геометрия
    // внутри фрейма (getBoundingClientRect/offsetWidth/offsetHeight)
    // вместо честных нулей (см. frame_bridge.rs: «layout содержимого
    // фрейма — отдельный срез»). Вьюпорт — [`FRAME_UA_DEFAULT_SIZE`]
    // (реальный размер host-бокса ещё не известен на этом шаге).
    // Измеритель собран как у страницы ([`page_measurer`]), плюс
    // @font-face ребёнка (FRAME-5, [`load_frame_fonts`]).
    // Каскад ребёнка разбирается один раз и переезжает в хэндл: срез 13
    // пересчитывает layout под реальный host-бокс, и повторный разбор
    // того же текста на каждом relayout был бы чистой тратой. Сам layout
    // тоже едет в хэндл (срез 14): по нему рисуется содержимое фрейма и в
    // нём ищется host-бокс вложенного фрейма.
    let mut frame_sheet = lumen_css_parser::parse(&subresources.css);
    // BUG-1495: `@font-face` под ложным `@media`/`@supports` не регистрируется.
    frame_sheet.retain_active_font_faces(&env.media_ctx);
    // GAP-CSPENF срез 25: the CHILD's own policy, read once here so both
    // `load_frame_fonts` (`font-src`) and `fetch_frame_background_images`
    // (`img-src`) below can gate against it without each re-parsing —
    // same one-shot shape as `fetch_frame_subresources`'s `csp_gate`.
    let child_csp_gate = {
        let d = child_doc_arc.lock().unwrap();
        let root = d.root();
        crate::csp_enforce::document_csp_policy(&d, root)
    };
    let child_self_origin = child_base.origin();
    // GAP-REFERRER срез 5: same one-shot read as `child_csp_gate` above, the
    // CHILD's own resolved policy for its own `@font-face url()`/background
    // images — `load_frame_fonts`/`fetch_frame_background_images` below.
    let child_referrer_policy =
        crate::resource_base::document_referrer_policy(&child_doc_arc.lock().unwrap());
    // CSS Color L5 §5.3: ICC bytes for the child's `color(--name …)`.
    crate::subresources::load_sheet_color_profiles(
        &mut frame_sheet,
        &child_base,
        sink,
        cookie_jar.clone(),
        child_referrer_policy,
    );
    // FRAME-5: синхронно (см. doc-comment `load_frame_fonts`) — тем же
    // приёмом, что срез 11 уже применяет к картинкам и таблицам стилей
    // ребёнка выше в этой функции.
    let _fonts_span = lumen_core::trace::span("frame-fonts", "font");
    let child_codepoints = crate::web_font_usage::document_codepoints(&child_doc_arc.lock().unwrap());
    let (font_registry, web_fonts, blocked_by_font_src) = load_frame_fonts(
        &frame_sheet.font_faces,
        &child_base,
        sink,
        cookie_jar.clone(),
        child_csp_gate.as_ref(),
        child_self_origin.as_ref(),
        child_referrer_policy,
        Some(&child_codepoints),
    );
    drop(_fonts_span);
    let _layout_span = lumen_core::trace::span("frame-layout", "layout");
    let frame_layout = frame_measurer(&frame_sheet.font_faces, &font_registry, &web_fonts).map(|measurer| {
        layout_frame_document(
            &child_doc_arc,
            &frame_sheet,
            FRAME_UA_DEFAULT_SIZE,
            child_js.as_ref(),
            &measurer,
            // Только что созданный фрейм не может быть ни под курсором, ни в
            // фокусе: его хэндла ещё нет в списке, адресовать его нечем.
            FrameNodeState::default(),
        )
    });
    drop(_layout_span);
    // FRAME-5: CSS Backgrounds L3 §3.10 — собираем `background-image: url(...)`
    // ребёнка уже после его layout-а (см. doc-comment
    // `fetch_frame_background_images` — картинки фона не влияют на расчёт
    // коробок, тот же порядок, что и у страницы в `parse_and_layout`).
    let (bg_images, bg_image_keys, blocked_by_bg_img_src) = frame_layout
        .as_ref()
        .map(|layout| {
            fetch_frame_background_images(
                layout,
                &child_base,
                sink,
                cookie_jar.clone(),
                env.target,
                child_csp_gate.as_ref(),
                child_self_origin.as_ref(),
                child_referrer_policy,
            )
        })
        .unwrap_or_default();
    // Lifecycle ребёнка: DOMContentLoaded сразу после parse+inline-скриптов
    // (тот же порядок, что у top-level в parse_and_layout); window load —
    // следом, НО после исходов подресурсов (срез 11): «load» документа
    // следует за его подресурсами, и тест, где внутри window load читают
    // загруженный `<img>`/`link.onload`, работает.
    if let Some(js) = &child_js {
        js.notify_dom_content_loaded();
        deliver_frame_subresource_events(js, &subresources);
        // GAP-CSPENF срез 8: `securitypolicyviolation` for every `img-src`/
        // `style-src`-blocked URL `fetch_frame_subresources` collected before
        // this runtime existed — same one-shot-push shape as
        // `page_pipeline.rs`'s `blocked_by_img_src`/`blocked_by_style_src`
        // dispatch, just against the CHILD's own runtime/policy instead of
        // the page's.
        // GAP-CSPENF срез 22/57: same push, extended with the inline-`<style>`
        // policy text `fetch_frame_subresources` now also collects for this
        // CHILD (`blocked_uri = "inline"`, same convention as
        // `page_pipeline.rs`'s `blocked_inline_style_policies` push).
        // GAP-CSPENF срез 24/57: same push again, extended with the
        // `style=""` attribute policy text — `violatedDirective=style-src-attr`,
        // same convention as `page_pipeline.rs`'s `blocked_style_attr_policies`
        // push (срез 23).
        // GAP-CSPENF срез 25: same push again, extended with `font-src`
        // (`@font-face url()`) and `img-src` (`background-image: url()`)
        // inside this frame — reuses `child_csp_gate` computed above instead
        // of re-locking `child_doc_arc` (that recompute predates this срез,
        // which already needed the policy earlier for `load_frame_fonts`/
        // `fetch_frame_background_images`).
        #[cfg(feature = "v8")]
        if (!subresources.blocked_by_img_src.is_empty()
            || !subresources.blocked_by_style_src.is_empty()
            || !subresources.blocked_inline_style_policies.is_empty()
            || !subresources.blocked_style_attr_policies.is_empty()
            || !blocked_by_font_src.is_empty()
            || !blocked_by_bg_img_src.is_empty())
            && let Some((policy, original_policy)) = &child_csp_gate
        {
            // Срез 56/57/58: `originalPolicy` — текст КАЖДОЙ нарушенной
            // политики (CSP3 §7.8/§3.4) для каждого URL/узла, известного по
            // отдельности; `original_policy` — фолбэк на случай, если
            // `violating_fetch_policy` ничего не находит (не должно случаться
            // для уже известного заблокированным URL, но дешевле остаться
            // корректным, чем не выстрелить событие вовсе).
            for url in &subresources.blocked_by_img_src {
                let texts = crate::csp_enforce::violating_fetch_policy(
                    policy, &lumen_network::csp::CspDirective::ImgSrc, url, child_self_origin.as_ref(),
                );
                if texts.is_empty() {
                    js.fire_csp_violation("img-src", url, original_policy);
                } else {
                    for text in &texts {
                        js.fire_csp_violation("img-src", url, text);
                    }
                }
            }
            for url in &subresources.blocked_by_style_src {
                let texts = crate::csp_enforce::violating_fetch_policy(
                    policy, &lumen_network::csp::CspDirective::StyleSrcElem, url, child_self_origin.as_ref(),
                );
                if texts.is_empty() {
                    js.fire_csp_violation("style-src-elem", url, original_policy);
                } else {
                    for text in &texts {
                        js.fire_csp_violation("style-src-elem", url, text);
                    }
                }
            }
            for text in &subresources.blocked_inline_style_policies {
                js.fire_csp_violation("style-src-elem", "inline", text);
            }
            for text in &subresources.blocked_style_attr_policies {
                js.fire_csp_violation("style-src-attr", "inline", text);
            }
            for url in &blocked_by_font_src {
                let texts = crate::csp_enforce::violating_fetch_policy(
                    policy, &lumen_network::csp::CspDirective::FontSrc, url, child_self_origin.as_ref(),
                );
                if texts.is_empty() {
                    js.fire_csp_violation("font-src", url, original_policy);
                } else {
                    for text in &texts {
                        js.fire_csp_violation("font-src", url, text);
                    }
                }
            }
            for url in &blocked_by_bg_img_src {
                let texts = crate::csp_enforce::violating_fetch_policy(
                    policy, &lumen_network::csp::CspDirective::ImgSrc, url, child_self_origin.as_ref(),
                );
                if texts.is_empty() {
                    js.fire_csp_violation("img-src", url, original_policy);
                } else {
                    for text in &texts {
                        js.fire_csp_violation("img-src", url, text);
                    }
                }
            }
        }
        js.notify_window_loaded();
    }
    // Вложенные фреймы ребёнка обрабатываем, пока известна его база.
    // Хэндлы уплощаются в общий список страницы; навигация или удаление
    // хоста уносит фрейм со всем поддеревом (`drop_frame_subtree`).
    if depth < MAX_FRAME_DEPTH {
        let nested = load_frame_sub_documents(
            &child_doc_arc,
            depth + 1,
            &child_base,
            top_doc,
            env,
            child_js.as_ref(),
        );
        handles.extend(nested);
    }
    // BUG-480 срез 2: биндинг «хост → под-документ» для contentWindow/
    // contentDocument родителя — строго до trusted `load` на хосте,
    // чтобы обработчики читали фасады сразу из обработчика. Срез 3:
    // имя хоста едет вместе с биндингом (ключ window[name]).
    //
    // BUG-1198: у ребёнка с рантаймом биндинг уже стоит с до-скриптовой
    // регистрации ([`crate::frame_ancestry::FrameAncestry::register`]); повтор здесь замещает его
    // на месте, добавляя `peer`, которого до создания рантайма ребёнка не
    // было, и сохраняет идентичность непрозрачного происхождения документа.
    if let Some(js) = parent_js {
        // BUG-979: peer only when same-origin — see the symmetric comment on
        // `parent_peer` above.
        let child_peer = accessible_parent.then(|| child_js.as_ref().and_then(|js| js.frame_peer_bridge())).flatten();
        js.register_iframe_document(
            info.node.index() as u32,
            Arc::clone(&child_doc_arc),
            &child_url,
            info.name.as_deref(),
            accessible_parent,
            opaque,
            child_peer,
        );
    }
    // OBJECT-1 срез 2: `load` на `<object>`/`<embed>` шлёт JS-шим
    // (`_lumen_embed_object_scan`) — второй был бы дублем.
    if !info.embedded {
        fire_iframe_load_event(parent_js, info.node);
    }
    let frame_scroll_containers = frame_layout
        .as_ref()
        .map(lumen_layout::collect_scroll_containers)
        .unwrap_or_default();
    handles.push(FrameHandle {
        host: info.node,
        url: child_url,
        load_failed,
        // BUG-480 срез 19: база ребёнка — сторона резолва его ссылок.
        base: child_base,
        doc: Arc::clone(&child_doc_arc),
        js: child_js,
        depth,
        sheet: frame_sheet,
        viewport: FRAME_UA_DEFAULT_SIZE,
        parent_doc: (depth > 0).then(|| Arc::clone(parent)),
        layout: frame_layout,
        content_dl: DisplayList::new(),
        interactive: FrameNodeState::default(),
        host_rect: None,
        host_src: info.src.clone().unwrap_or_default(),
        is_svg_embed: child_is_svg,
        images: {
            let mut images = subresources.decoded_images;
            images.extend(bg_images);
            images
        },
        image_keys: {
            let mut image_keys = subresources.image_keys;
            image_keys.extend(bg_image_keys);
            image_keys
        },
        scroll_y: 0.0,
        scroll_x: 0.0,
        scroll_containers: frame_scroll_containers,
        font_registry,
        web_fonts,
        animated_gifs: subresources.animated_gifs,
        lazy_requests: subresources.lazy_requests,
        pending_lazy: Vec::new(),
    });
    handles
}
