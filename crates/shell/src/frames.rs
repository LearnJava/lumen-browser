//! Nested browsing contexts: the `<iframe>`/`<frame>` sandbox gates, where a
//! child document's HTML comes from, the same-origin check its parent is
//! allowed to reach it through, its own subresource pass, and the `load` event
//! fired back at the host element.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;
use crate::relayout::page_measurer;
use lumen_paint::DisplayCommand;

/// Apply sandbox restrictions for all `<iframe sandbox>` elements in the document.
///
/// Two paths depending on whether the iframe has a `srcdoc` attribute:
/// - **`srcdoc` iframes** — inline HTML is parsed and sandbox gates are applied to
///   the inner document: scripts blocked (if `SCRIPTS`), forms blocked (if `FORMS`),
///   navigation blocked (if `NAVIGATION`), popups blocked (if `AUXILIARY_NAVIGATION`).
/// - **URL-based iframes** — Phase 0: sub-document is not loaded; logs each active
///   restriction to stderr without applying gates to the host document.
///
/// Returns the total number of blocked capabilities across all sandboxed iframes
/// (script count + form count + navigation link count + popup gate hits).
pub(crate) fn apply_iframe_sandbox_gates(doc: &Document) -> usize {
    let iframes = collect_iframes(doc);
    let mut blocked = 0usize;
    for info in &iframes {
        if !info.is_sandboxed {
            continue;
        }
        let sb = info.sandbox;

        if let Some(html) = &info.srcdoc {
            // srcdoc iframe: parse inline HTML and apply gates to the inner document.
            let inner = lumen_html_parser::parse(html);

            if sb.contains(lumen_core::SandboxFlags::SCRIPTS) {
                let mut scripts = Vec::new();
                let mut modules = Vec::new();
                collect_inline_scripts(&inner, inner.root(), &mut scripts, &mut modules);
                let n = scripts.len() + modules.len();
                if n > 0 {
                    eprintln!(
                        "sandbox: srcdoc iframe — заблокировано {n} скрипт(ов) (sandbox=scripts)"
                    );
                    blocked += n;
                }
            }
            if sb.contains(lumen_core::SandboxFlags::FORMS) {
                blocked += check_form_gate(&inner, sb);
            }
            if sb.contains(lumen_core::SandboxFlags::NAVIGATION) {
                blocked += check_navigation_gate(&inner, sb);
            }
            if check_popup_gate(sb) {
                blocked += 1;
            }
        } else {
            // URL-based iframe: Phase 0 — sub-document not loaded, log restrictions only.
            let src = info.src.as_deref().unwrap_or("<no src>");
            if sb.contains(lumen_core::SandboxFlags::SCRIPTS) {
                eprintln!("sandbox: iframe '{src}' — скрипты запрещены (sandbox=scripts)");
            }
            if sb.contains(lumen_core::SandboxFlags::FORMS) {
                eprintln!("sandbox: iframe '{src}' — формы запрещены (sandbox=forms)");
            }
            if sb.contains(lumen_core::SandboxFlags::NAVIGATION) {
                eprintln!(
                    "sandbox: iframe '{src}' — навигация запрещена (sandbox=top-navigation)"
                );
            }
            check_popup_gate(sb);
        }
    }
    blocked
}

// ── iframe sub-документы (BUG-480) ───────────────────────────────────────────

/// Откуда брать HTML sub-документа фрейма.
///
/// `pub(crate)`, а не модульно-приватный: [`fetch_iframe_source`] сам
/// `pub(crate)` (нужен тесту в `tests/scripts_and_frames.rs`), и тип его
/// `Ok`-варианта обязан быть виден не менее широко.
pub(crate) enum FrameSource {
    /// Готовый HTML (атрибут `srcdoc` / пустой `about:blank`).
    Inline(String),
    /// Прочитанный файл.
    File { html: String, path: std::path::PathBuf },
    /// Тело ответа по сети.
    Url { html: String, url: String },
}

/// Итог неудачной попытки получить исходник под-документа фрейма (FRAME-4
/// срез 2): причина — для error-документа и уже привычного stderr-лога,
/// `attempted_url` — адрес, который так и не открылся, он же становится
/// [`FrameHandle::url`] вместо адреса, ушедшего в никуда молча.
pub(crate) struct FetchError {
    pub(crate) reason: String,
    pub(crate) attempted_url: String,
}

/// Резолвит `src` относительно `resolve_base` и, если владелец фрейма несёт
/// `upgrade-insecure-requests`, апгрейжит `http:` в `https:`
/// (`csp_enforce::upgrade_navigation_url`, UIR §4.1 шаг 5) — GAP-CSPENF срез
/// 52. Пустой/`about:`/`data:`/`javascript:` `src` возвращается КАК ЕСТЬ:
/// `frame_src_check` и [`fetch_iframe_source`] сами узнают эти формы и не
/// уходят в сеть, а резолв пустой строки против `resolve_base` дал бы адрес
/// самого владельца (`ResourceBase::resolve("")` — пустая ссылка резолвится
/// в саму базу, RFC 3986 §5.3), что подменило бы «фрейм без содержимого»
/// сетевым запросом к странице-хозяину.
fn maybe_upgrade_frame_src(
    csp_gate: Option<&(Vec<lumen_network::csp::CspPolicy>, String)>,
    src: &str,
    resolve_base: &ResourceBase,
) -> String {
    let lowered = src.trim_start().to_ascii_lowercase();
    if lowered.is_empty()
        || lowered.starts_with("about:")
        || lowered.starts_with("data:")
        || lowered.starts_with("javascript:")
    {
        return src.to_owned();
    }
    let resolved = resolve_base.resolve_str(src);
    crate::csp_enforce::upgrade_navigation_url(csp_gate, &resolved)
}

/// Получить исходник под-документа для `src`-фрейма: разрешить относительно
/// `base`, файл прочитать с диска, URL скачать через subresource-клиент с
/// `RequestDestination::Document` (тот же mixed-content/SW-интерсептор, что у
/// остальных подресурсов). `Err` — источник получить нельзя (лог уже
/// напечатан внутри; вызывающая сторона показывает причину в самом фрейме —
/// FRAME-4 срез 2, до него `spawn_frame` на этой ошибке просто не заводил
/// хэндл, и фрейм молча оставался прежним документом либо серой заглушкой).
///
/// `send_uir_header` — GAP-CSPENF срез 55: `Upgrade-Insecure-Requests: 1` на
/// сетевом запросе, когда родитель фрейма объявил
/// `upgrade-insecure-requests` — `frames.rs`'s собственный аналог
/// `csp_enforce::navigation_wants_uir_header`, вычисленный вызывающей
/// стороной один раз из уже читаемого `csp_gate` (тот же порядок, что срез 52
/// уже даёт апгрейду схемы через `maybe_upgrade_frame_src`).
#[allow(clippy::too_many_arguments)] // fetch context threaded through, same shape as stylesheets.rs
pub(crate) fn fetch_iframe_source(
    src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    send_uir_header: bool,
    referrer_policy: lumen_network::ReferrerPolicy,
) -> Result<FrameSource, FetchError> {
    if src.trim().is_empty() {
        return Ok(FrameSource::Inline(String::new()));
    }
    let lowered = src.trim_start().to_ascii_lowercase();
    if lowered.starts_with("javascript:") {
        let reason = "javascript:-URL не поддерживаются (BUG-480 срез 1)".to_owned();
        eprintln!("iframe: {reason}, пропуск '{src}'");
        return Err(FetchError { reason, attempted_url: src.to_owned() });
    }
    if lowered.starts_with("data:") {
        let reason = "data:-URL не поддерживаются (BUG-480 срез 1)".to_owned();
        eprintln!("iframe: {reason}, пропуск '{src}'");
        return Err(FetchError { reason, attempted_url: src.to_owned() });
    }
    // HTML §7.6: `about:blank` не скачивается — фрейм немедленно получает
    // пустой документ, ровно как фрейм вообще без `src` (ветка `None` в
    // `spawn_frame` уже помечает такой под-документ адресом `about:blank`).
    //
    // BUG-1018: до этого строка проваливалась в сетевой резолвер, возвращалась
    // как `unsupported scheme: about`, и фрейм показывал синтетическую страницу
    // «Не удалось загрузить фрейм» с `load_failed = true` — видимая коробка с
    // ошибкой там, где спека требует пустой документ. Форма записи не должна
    // ничего менять: `<iframe>` и `<iframe src="about:blank">` — один и тот же
    // под-документ.
    //
    // Хвост из query/fragment (`about:blank?x`, `about:blank#y`) — всё ещё
    // about:blank-документ, а вот `about:config` и прочие about:-адреса
    // по-прежнему отказывают, как и раньше.
    if lowered == "about:blank"
        || lowered.starts_with("about:blank?")
        || lowered.starts_with("about:blank#")
    {
        return Ok(FrameSource::Inline(String::new()));
    }
    match base.resolve(src) {
        ResolvedResource::File(path) => {
            let attempted_url = path_to_file_url(&path);
            match std::fs::read_to_string(&path) {
                Ok(html) => Ok(FrameSource::File { html, path }),
                Err(e) => {
                    let reason = format!("файл {} не читается: {e}", path.display());
                    eprintln!("iframe: {reason}");
                    Err(FetchError { reason, attempted_url })
                }
            }
        }
        ResolvedResource::Url(url) => {
            use lumen_core::url::Url as _Url;
            let sub_url = match _Url::parse(&url) {
                Ok(u) => u,
                Err(e) => {
                    let reason = format!("битый URL '{url}': {e}");
                    eprintln!("iframe: {reason}");
                    return Err(FetchError { reason, attempted_url: url });
                }
            };
            let client = base.http_client_for_subresource_with_policy(
                Arc::clone(sink),
                cookie_jar,
                referrer_policy,
            );
            match client.fetch_subresource_document(&sub_url, send_uir_header) {
                Ok(bytes) => Ok(FrameSource::Url {
                    html: String::from_utf8_lossy(&bytes).into_owned(),
                    url,
                }),
                Err(e) => {
                    let reason = format!("загрузка '{url}' не удалась: {e}");
                    eprintln!("iframe: {reason}");
                    Err(FetchError { reason, attempted_url: url })
                }
            }
        }
    }
}

/// Исполнить `javascript:` `src` фрейма (GAP-NAVCTX срез 6, BUG-884) в
/// контексте РОДИТЕЛЯ (`parent_js`), а не ребёнка.
///
/// HTML LS §7.4.5 хочет Realm навигируемого объекта (ребёнка), но у ребёнка
/// на этот момент ещё нет JS-контекста, и строить его загодя — отдельный
/// V8-изолят на том же потоке ДО того, как известен финальный HTML —
/// зависает движок насмерть (испробовано и отброшено в этом же срезе: два
/// живых изолята на одном потоке друг друга блокируют где-то в рантайме
/// V8/rusty_v8, страница не печатает ни одного `setInterval`-тика после
/// `script-start`). Тот же компромисс, каким BUG-883 срез 2 уже пожертвовал
/// для `window.open()` («код исполняется в контексте ОПЕНЕРА, верно по
/// спеке» — там это было верно и по спеке; здесь это упрощение):
/// `parent.javascriptUrlRan++` из `iframe_javascript_url_initial_insertion`
/// читает `parent` РОДИТЕЛЯ, а не ребёнка — для фрейма глубины 0 это то же
/// самое окно (`window.parent === window` на верхней странице), поэтому
/// тест не различает два пути; для глубины ≥ 1 без поправки код читал бы
/// деда, а не родителя (родительский `window.parent` — это ОДИН уровень
/// выше настоящего родителя исполняемого фрейма). GAP-NAVCTX срез 17
/// закрывает именно этот хвост: локально затеняем `parent` перед кодом на
/// `window` исполняющего контекста — это и есть настоящий родитель ребёнка,
/// в чьём `src` лежит `javascript:`. `let` в теле `eval()` создаёт биндинг
/// лексического окружения этого конкретного вызова, который стоит в цепочке
/// областей видимости ВЫШЕ акцессора `window.parent` на глобальном объекте
/// (`installHierarchyAccessors`, `frame_bridge.rs`), поэтому подмена не
/// требует трогать сам акцессор и не переживает этот единственный `eval`.
///
/// Возвращает `Some(html)`, если завершение — строка (становится
/// финальным HTML фрейма, как обычный `FrameSource::Inline`); `None` —
/// не-строковое завершение (typical: `void`/`undefined`) не заменяет
/// документ по спеке, но код уже отработал свои побочные эффекты.
fn eval_iframe_javascript_url(code: &str, parent_js: Option<&Arc<dyn PersistentJs>>) -> Option<String> {
    let scoped = format!("let parent = window;\n{code}");
    parent_js?.eval_js_completion(&scoped).ok().flatten()
}

/// Экранирует символы, опасные в HTML-тексте (то же правило, что
/// `newtab.rs::escape_html` — своя копия по тому же соглашению модуля).
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Синтетический документ «навигация фрейма не удалась» (FRAME-4 срез 2) —
/// тот же приём, что уже показывает `about:blank`/`srcdoc` инлайном, только
/// текст не с диска, а собран здесь. `url`/`reason` экранируются: оба несут
/// сырой ввод страницы (адрес ссылки, текст сетевой ошибки).
pub(crate) fn frame_error_document(url: &str, reason: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head>\
         <body style=\"font-family:sans-serif;color:#5f6368;background:#fff;\
         padding:16px;margin:0\">\
         <p style=\"margin:0 0 4px;font-weight:bold\">Не удалось загрузить фрейм</p>\
         <p style=\"margin:0;word-break:break-all\">{}</p>\
         <p style=\"margin:8px 0 0;color:#888\">{}</p>\
         </body></html>",
        html_escape(url),
        html_escape(reason),
    )
}

/// Origin-строка абсолютного URL (`scheme://host:port`, host в нижнем регистре).
///
/// Порты по умолчанию (http→80, https→443) опускаются — как в origin-алгоритме
/// HTML LS §7.5.3. `None` — URL не распарсился или без хоста (opaque origin,
/// как у `file://`).
fn url_origin_str(url: &str) -> Option<String> {
    let u = lumen_core::url::Url::parse(url).ok()?;
    if u.host().is_empty() {
        return None;
    }
    let scheme = u.scheme().to_ascii_lowercase();
    let port = u
        .port()
        .filter(|p| !((scheme == "http" && *p == 80) || (scheme == "https" && *p == 443)))
        .map(|p| format!(":{p}"))
        .unwrap_or_default();
    Some(format!("{scheme}://{}{}", u.host().to_ascii_lowercase(), port))
}

/// Правило доступа родителя к под-документу фрейма (BUG-480 срез 2).
///
/// HTML LS §7.3.1.2: `contentDocument` доступен только same-origin; opaque
/// origin (`sandbox` без `allow-same-origin`) не совпадает ни с чем.
/// `about:blank`/`about:srcdoc` наследуют origin родителя. Локальные файлы
/// считаем взаимно доступными (упрощённая модель Firefox same-directory):
/// у `file://` нет хоста, и строгая проверка сделала бы недоступным самый
/// частый локальный сценарий; отклонение от спеки задокументировано в
/// bugs/BUG-480-OPEN.md.
/// URL базы в строковой форме для фасадов `location`/`URL` (BUG-480 срез 3).
///
/// Единственное каноническое правило вывода адреса из [`ResourceBase`] — то
/// же, что у `page_url` в `parse_and_layout`: сетевая база берётся как есть,
/// файловая получает схему `file://`.
pub(crate) fn base_url_string(base: &ResourceBase) -> String {
    match base {
        ResourceBase::Url(u) => u.clone(),
        ResourceBase::File(p) => path_to_file_url(p),
    }
}

pub(crate) fn frame_access_allowed(parent_base: &ResourceBase, child_url: &str, opaque_sandbox: bool) -> bool {    if opaque_sandbox {
        return false;
    }
    if child_url.starts_with("about:") {
        return true;
    }
    match parent_base {
        ResourceBase::Url(parent) => match (url_origin_str(parent), url_origin_str(child_url)) {
            (Some(p), Some(c)) => p == c,
            // Хотя бы одна сторона opaque: взаимно доступны только два файла.
            _ => parent.starts_with("file:") && child_url.starts_with("file:"),
        },
        // У родителя-файла origin opaque: доступен только ребёнок-файл
        // (у сетевого ребёнка есть хост — он никогда не равен opaque).
        ResourceBase::File(_) => child_url.starts_with("file:"),
    }
}

/// Диспетчеризовать `load` на `<iframe>`-элементе через родительский JS-контекст.
///
/// Событие не всплывает и не отменяется (HTML LS §4.8.5); `target` — сам
/// элемент. Вызов синхронный: к этому моменту скрипты ребёнка уже выполнены и
/// его DOMContentLoaded отправлен.
#[allow(unused_variables)] // parent_js читается только под feature = "v8"
fn fire_iframe_load_event(parent_js: Option<&Arc<dyn PersistentJs>>, host: NodeId) {
    #[cfg(feature = "v8")]
    if let Some(js) = parent_js {
        js.eval_js(&format!(
            "(function() {{ var e = new Event('load', {{bubbles:false, cancelable:false, isTrusted:true}}); \
             e.target = _lumen_make_element({}); _lumen_dispatch({}, e); }})()",
            host.index(),
            host.index(),
        ));
    }
}

/// Загрузить sub-документы всех `<iframe>`/`<frame>` документа и вернуть их
/// хэндлы.
///
/// BUG-854: `<frame>` проходит здесь тем же путём, что `<iframe>` — списком их
/// обоих отдаёт [`collect_iframes`]; отличия только в атрибутах, которых у
/// `<frame>` нет (`srcdoc`, `sandbox`, `loading`).
///
/// Срез 1 BUG-480: для каждого фрейма — собрать источник (`srcdoc` → inline,
/// `src` → файл/сеть; отсутствие обоих = `about:blank`), распарсить в
/// отдельный `Document`, выполнить его скрипты в собственном JS-контексте
/// (`run_scripts_with_dom`: тот же набор провайдеров сети и хранилищ, что у
/// страницы), отправить ребёнку DOMContentLoaded+load и диспектчнуть `load`
/// на элементе-хосте. `loading="lazy"` пропускается до появления
/// viewport-прокси (отдельный срез).
///
/// Срез 3 BUG-480: контексту ребёнка передаются документы предков
/// (`window.parent`/`window.top`), а родителю — биндинг под-документа с именем
/// хоста (`window[name]`). `top_doc`/`top_base` — документ и база ВЕРХНЕГО
/// окна страницы; при первом вызове совпадают с `parent`/`base`, в рекурсии
/// передаются без изменений.
///
/// Срез 11 BUG-480: подресурсы парсерных элементов ребёнка (`<img src>`,
/// `<link rel=stylesheet>`) запрашиваются сразу после разбора ([`fetch_frame_subresources`],
/// до скриптов), а их `load`/`error` доставляются контексту ребёнка после DCL
/// и до window load ([`deliver_frame_subresource_events`]). `media_ctx`/`viewport` —
/// экранный гейт media `<link>` и вьюпорт picker-а картинок: те же значения,
/// что страница использует для своих подресурсов.
///
/// Блокировки:
/// - глубина рекурсии ограничена [`MAX_FRAME_DEPTH`];
/// - `sandbox` без `allow-scripts` гейтится внутри `run_scripts_with_dom`;
/// - `sandbox` без `allow-same-origin` — opaque origin: ребёнку не выдаются
///   персистентные хранилища (localStorage/IDB/SW/Cache);
/// - навигационные запросы из скриптов ребёнка (`location.href=`) пока
///   отклоняются с логом — навигация фреймов вне среза 1.
///
/// Вызывать можно с любым состоянием блокировок снаружи: лок родителя
/// берётся коротко (только обход дерева); выполнение скриптов ребёнка и
/// диспектч `load` на хосте идут БЕЗ удержанных лаков — обработчики вправе
/// синхронно читать DOM обеих сторон.
/// Срез 12 BUG-480: сразу после регистрации `parent`/`top` (выше) —
/// cascade + layout ребёнка на UA-дефолтном вьюпорте [`FRAME_UA_DEFAULT_SIZE`]
/// (реальный host-бокс ещё не известен), результат уходит в
/// `update_layout_rects`/`update_viewport_size` JS-контекста ребёнка — первая
/// content-геометрия внутри фрейма (`getBoundingClientRect` и т.п.) вместо
/// честных нулей. Срез 13: как только layout родителя посчитан,
/// [`sync_frame_viewports`] пересчитывает ребёнка под РЕАЛЬНЫЙ контентный бокс
/// хоста. Paint (компоновка display list ребёнка в бокс `<iframe>` вместо
/// серой заглушки) и relayout при мутациях остаются в очереди среза.
///
/// Исходы подресурсов парсерных элементов под-документа фрейма (BUG-480 срез 11).
#[derive(Default)]
pub(crate) struct FrameSubresourceOutcomes {
    /// `(узел <link rel=stylesheet>, лист получен)` в порядке объявления —
    /// форма [`load_linked_stylesheets`].
    pub(crate) links: Vec<(NodeId, bool)>,
    /// `(узел <img>, байты получены)` в порядке DOM.
    pub(crate) images: Vec<(NodeId, bool)>,
    /// BUG-480 срез 15: декодированные картинки ребёнка — `(ключ регистрации,
    /// пиксели)`, форма `LoadedPage::images`. Ключ — РАЗРЕШЁННЫЙ адрес
    /// ([`frame_image_key`]), а не сырой `src`.
    pub(crate) decoded_images: Vec<(String, Arc<lumen_image::Image>)>,
    /// BUG-480 срез 15: `(сырой src, ключ регистрации)` для КАЖДОГО `<img>`
    /// ребёнка — в том числе не загрузившегося.
    ///
    /// По этой карте [`rekey_frame_images`] переписывает ключи в display list
    /// под-документа. Битые картинки в карте тоже: иначе ключ остался бы сырым
    /// и совпал бы с чужим зарегистрированным — во фрейме нарисовалась бы
    /// картинка страницы.
    pub(crate) image_keys: Vec<(String, String)>,
    /// BUG-480 срез 12: текст каскада ребёнка (инлайновые `<style>` с
    /// разрешённым `@import`, затем внешние `<link rel=stylesheet>`, в этом
    /// порядке — форма страницы, `parse_and_layout`). До среза 12 такой текст
    /// не собирался вовсе (фреймы не лежали в layout); теперь его парсит и
    /// использует `load_frame_sub_documents` сразу после этого прохода.
    pub(crate) css: String,
    /// FRAME-5: многокадровые GIF-анимации ребёнка — `(ключ регистрации,
    /// анимация)`, форма `LoadedPage::animated_gifs`. До этого среза наружу шёл
    /// только первый кадр ([`fetch_frame_subresources`] это документировал явно) —
    /// сама анимация отбрасывалась, и `Lumen::animated_gifs` (карта СТРАНИЦЫ) о
    /// ней не знала. Тикает по [`FrameHandle::animated_gifs`].
    pub(crate) animated_gifs: Vec<(String, lumen_image::AnimatedGif)>,
    /// FRAME-5 срез 2: `<img loading="lazy">` requests of the child, collected
    /// but NOT fetched — mirrors `ParsedPage::lazy_pairs` at the page level.
    /// `image_keys` above already carries a `(raw_src, frame_image_key)` entry
    /// for each of these too, so [`rekey_frame_images`] rewrites their
    /// not-yet-loaded placeholder `DrawImage` the same way it rewrites an
    /// eager one — the registration key is stable from the first paint, only
    /// the pixels behind it arrive later.
    pub(crate) lazy_requests: Vec<lumen_layout::ImageRequest>,
    /// GAP-CSPENF срез 8: resolved `<img src>` URLs the CHILD's own
    /// `img-src`/`default-src` policy blocked — fetch never ran for them
    /// (same "don't touch the network at all" shape as
    /// `subresources.rs::fetch_and_decode_images`'s `blocked_by_img_src`).
    /// The frame has no JS runtime yet at this point (`fetch_frame_subresources`
    /// runs before `run_scripts_with_dom`), so dispatching
    /// `securitypolicyviolation` for these is the caller's job, same as the
    /// page-level counterpart.
    pub(crate) blocked_by_img_src: Vec<String>,
    /// GAP-CSPENF срез 8: resolved `<link rel=stylesheet>` URLs the CHILD's
    /// own `style-src`/`default-src` policy blocked — `load_linked_stylesheets`
    /// already computed this (срез 7), it was just discarded here before this
    /// срез.
    pub(crate) blocked_by_style_src: Vec<String>,
    /// GAP-CSPENF срез 22: text of the `style-src`/`default-src` policy that
    /// blocked each inline `<style>` node of the CHILD — same gate срез 21
    /// already gives the top-level document's `extract_style_blocks` call,
    /// applied to the frame's own `csp_gate` (computed above for `img-src`
    /// since срез 8). No URL to report — `blockedURI` for inline is always
    /// `"inline"`, same shape as `page_pipeline.rs`'s
    /// `blocked_inline_style_policies`. Срез 57 turned this from a bare count
    /// into the violated policy's own text.
    pub(crate) blocked_inline_style_policies: Vec<String>,
    /// GAP-CSPENF срез 24: text of the policy that blocked each `style=""`
    /// attribute of the CHILD — same gate срез 23 already gives the
    /// top-level document's `collect_style_attr_csp_blocked` call, applied to
    /// the frame's own `csp_gate`. The blocked node id set itself is written
    /// directly onto `doc` inside [`fetch_frame_subresources`] (the cascade
    /// reads it off the document, same as the top-level path in
    /// `page_pipeline.rs`) — this vec exists only so the caller can dispatch
    /// one `securitypolicyviolation` per blocked node, same one-shot-push
    /// shape as `blocked_inline_style_policies`. Срез 57: was a bare count.
    pub(crate) blocked_style_attr_policies: Vec<String>,
    /// GAP-CSPENF срез 27: `true` if the CHILD's own `frame-ancestors`
    /// directive refuses embedding by `ancestor_origin` (the immediate
    /// embedder — CSP3 §6.4.2). When set, every other field above is left
    /// at its default: the check runs before any subresource fetch, the
    /// same "don't touch the network at all" shape срез 8 already gives
    /// `img-src`. The caller replaces the whole sub-document with a blocked
    /// page instead of running scripts/layout on the fetched one.
    pub(crate) frame_ancestors_blocked: bool,
}

/// Запросить подресурсы парсерных элементов под-документа фрейма (BUG-480
/// срез 11): `<link rel=stylesheet>` и `<img src>`.
///
/// До этого среза за URL картинок и листов ребёнка не ходил никто — сервер не
/// видел ни одного запроса (срез 24 зафиксировал это записью запросов), хотя
/// сами элементы в дереве были. Проход повторяет страницу: стили — тот же
/// [`load_linked_stylesheets`] (media-гейт по `media_ctx` страницы), картинки —
/// picker [`lumen_layout::collect_image_requests`] (`<picture>`/`srcset`), чей
/// ключ URL совпадает с тем, что эмитит layout.
///
/// Срез 12: текст каскада (инлайновые `<style>` через `extract_style_blocks`/
/// `inline_css_imports`, затем внешние листы) теперь возвращается вместо
/// отбрасывания — им пользуется layout ребёнка в `load_frame_sub_documents`
/// сразу после этого прохода.
///
/// Срез 15: картинки проходят весь путь страницы, а не только сеть —
/// [`decode_image`] через `IMAGE_CACHE`, intrinsic-размеры в дерево ребёнка
/// (иначе `<img>` без атрибутов лёг бы нулевым боксом) и пиксели наружу для
/// регистрации в рендерере. До среза брались только байты, которые никто не
/// декодировал: рисовать их было некому, пока содержимое фрейма не попадало на
/// экран (срез 14).
///
/// `loading="lazy"` не запрашивается ЗДЕСЬ — эта функция лишь СОБИРАЕТ такие
/// запросы (`lazy_requests` в возвращаемом значении) без сети, как страница
/// собирает `ParsedPage::lazy_pairs`. Доставка — FRAME-5 срез 2,
/// `crate::frame_lazy`: вьюпорт под-документа появляется только у
/// [`sync_frame_viewports`]/[`layout_frame_document`] (эта функция не знает
/// ни его, ни JS-контекста ребёнка), а рендерер/image-кэш СТРАНИЦЫ — только у
/// `&mut Lumen`, за пределами и этой функции, и `sync_frame_viewports`.
#[allow(clippy::too_many_arguments)] // same debt as `spawn_frame` above, docs/lint-policy.md §10
pub(crate) fn fetch_frame_subresources(
    doc: &mut Document,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    media_ctx: &lumen_css_parser::MediaContext,
    viewport: lumen_core::geom::Size,
    target: lumen_core::ColorSpace,
    ancestor_origin: Option<&lumen_network::Origin>,
) -> FrameSubresourceOutcomes {
    // GAP-CSPENF срез 8: same one-shot policy computation as
    // `subresources.rs::fetch_and_decode_images` — the CHILD document's OWN
    // policy (`<meta>`/header of the sub-document, not the parent's), so a
    // frame is gated by its own CSP. Срез 22 moved this above
    // `extract_style_blocks` (it used to be computed only for `img-src`,
    // further down) so the inline-`<style>` gate below can use it too.
    let csp_gate = {
        let root = doc.root();
        crate::csp_enforce::document_csp_policy(doc, root)
    };
    // GAP-REFERRER срез 5: this function already holds `&mut Document` (the
    // frame's own), same reasoning as `subresources.rs::fetch_and_decode_images`
    // — read straight off it rather than threading a parameter through.
    let referrer_policy = crate::resource_base::document_referrer_policy(doc);
    // GAP-CSPENF срез 27: `frame-ancestors` is a navigation directive, not a
    // fetch directive — it governs whether this sub-document may be
    // embedded AT ALL, not one of its own subresource fetches. Checked
    // first and unconditionally short-circuits: no image/style fetch below
    // is worth starting for a document that will not render.
    if let (Some((policy, _)), Some(ancestor)) = (csp_gate.as_ref(), ancestor_origin) {
        let self_origin = base.origin();
        if crate::csp_enforce::frame_ancestors_blocked(policy, ancestor, self_origin.as_ref()) {
            return FrameSubresourceOutcomes {
                frame_ancestors_blocked: true,
                ..Default::default()
            };
        }
    }
    // GAP-CSPENF срез 22: инлайновый `<style>` внутри `<iframe>` теперь
    // гейтится по политике РЕБЁНКА — тот же `inline_style_blocked`, что срез
    // 21 уже применяет к top-level документу; до этого среза `<style>` внутри
    // `<iframe>` не проверялся вовсе (та же граница, что срез 7 документирует
    // для внешнего `<link>` подфрейма до срез 8).
    let (inline, blocked_inline_style_policies) =
        extract_style_blocks(doc, csp_gate.as_ref().map(|(p, _)| p.as_slice()));
    // GAP-CSPENF срез 24: `style=""` attribute inside a frame — same one-shot
    // policy read as the inline `<style>` gate above, separate walk (the
    // attribute lives on arbitrary elements, not only `<style>` nodes); срез
    // 23 gives this to the top-level document, this срез is the gap it named
    // as not covered ("атрибут `style=` внутри `<iframe>`"). The blocked set
    // is written onto `doc` immediately — `lumen_layout`'s cascade reads it
    // straight off the document (`Document::is_style_attr_csp_blocked`), the
    // same way it does for the top-level document in
    // `page_pipeline.rs::build_page_cascade`.
    let (blocked_style_attr_nodes, blocked_style_attr_policies) =
        collect_style_attr_csp_blocked(doc, csp_gate.as_ref().map(|(p, _)| p.as_slice()));
    doc.set_style_attr_csp_blocked(blocked_style_attr_nodes);
    let self_origin = base.origin();
    // GAP-CSPENF срез 38: `style-src` also gates `@import` targets inside the
    // frame's own inline `<style>` — same CHILD policy, same one-shot
    // `csp_gate` this function already reads at the top for the image/style
    // gates above.
    let (mut css, blocked_by_style_src_imports) = inline_css_imports(
        &inline,
        base,
        sink,
        cookie_jar.clone(),
        media_ctx,
        &mut std::collections::HashSet::new(),
        0,
        crate::stylesheets::document_encoding(doc),
        csp_gate.as_ref().map(|(p, _)| (p.as_slice(), self_origin.as_ref())),
        crate::resource_base::document_referrer_policy(doc),
    );
    // GAP-CSPENF срез 7: `style-src` gates the fetch here (blocked sheets
    // return the same `false` outcome a network failure would); срез 8 stops
    // discarding the blocked-URL list `load_linked_stylesheets` already
    // computes and surfaces it via `FrameSubresourceOutcomes` for the caller
    // to dispatch `securitypolicyviolation` on (no JS runtime exists yet here).
    let (linked, links, mut blocked_by_style_src) =
        load_linked_stylesheets(doc, base, sink, cookie_jar.clone(), media_ctx);
    blocked_by_style_src.extend(blocked_by_style_src_imports);
    css.push_str(&linked);

    let (requests, lazy_requests): (Vec<lumen_layout::ImageRequest>, Vec<lumen_layout::ImageRequest>) =
        lumen_layout::collect_image_requests(doc, viewport)
            .into_iter()
            .partition(|req| !req.is_lazy);
    // Фаза 1 (параллельно): сеть + декодирование, `doc` не трогаем — форма
    // `fetch_and_decode_images` страницы. Третий элемент кортежа — резолвленный
    // URL, если `img-src` его заблокировал (`None` — не блокировался, фетч
    // (не)успешен обычным путём); отличает CSP-блок от сетевой неудачи, чтобы
    // фаза 2 могла отчитаться `securitypolicyviolation` только за первое.
    let decoded = parallel_map(&requests, |_, req| {
        let sink: &Arc<dyn EventSink> = &sink.clone();
        let key = frame_image_key(base, &req.url);
        // GAP-CSPENF срез 44: same order as the page (срез 43) — upgrade the
        // scheme before the `img-src` gate sees the URL (Fetch §4.1: upgrade
        // is step 5, the CSP check step 6). The registry key (`key`, above)
        // stays the raw resolved `req.url` either way — upgrade only changes
        // the address that is actually fetched.
        let upgraded = csp_gate
            .as_ref()
            .and_then(|(policy, _)| crate::csp_enforce::upgrade_insecure_url(policy, &key));
        if let Some((policy, _)) = &csp_gate {
            let resolved_url = upgraded.clone().unwrap_or_else(|| key.clone());
            // OBJECT-1: `<object>`/`<embed>` гейтятся `object-src`, и
            // `img-src`-нарушения для них нет.
            if req.embedded_content {
                if crate::csp_enforce::object_src_blocked(policy, &resolved_url, self_origin.as_ref()) {
                    return (key, None, None);
                }
            } else if crate::csp_enforce::img_src_blocked(policy, &resolved_url, self_origin.as_ref()) {
                return (key, None, Some(resolved_url));
            }
        }
        let fetch_src: &str = upgraded.as_deref().unwrap_or(&req.url);
        // GAP-REFERRER срез 7: same element-wins-over-document override as
        // `subresources.rs::fetch_and_decode_images` — a frame's own `<img>`
        // can carry `referrerpolicy` independently of the frame document's
        // policy above.
        let img_referrer_policy = req
            .referrer_policy_attr
            .as_deref()
            .and_then(lumen_network::ReferrerPolicy::parse)
            .unwrap_or(referrer_policy);
        let img = crate::image_cache::IMAGE_CACHE.get_or_decode_current(&key, || {
            decode_image(fetch_src, base, sink, cookie_jar.clone(), target, img_referrer_policy)
        });
        (key, img, None)
    });
    // Фаза 2 (последовательно): intrinsic-размеры в дерево ребёнка и сборка
    // выходных векторов в порядке DOM.
    let mut images = Vec::with_capacity(requests.len());
    let mut decoded_images = Vec::new();
    let mut image_keys = Vec::with_capacity(requests.len());
    let mut animated_gifs = Vec::new();
    let mut blocked_by_img_src = Vec::new();
    for (req, (key, img, blocked_url)) in requests.iter().zip(decoded) {
        if let Some(url) = blocked_url {
            blocked_by_img_src.push(url);
        }
        image_keys.push((req.url.clone(), key.clone()));
        // BUG-269, как у страницы: intrinsic нужен, если автор не задал ХОТЯ БЫ
        // одно измерение — второе достраивается по соотношению сторон.
        let wants_intrinsic = !(req.has_explicit_width && req.has_explicit_height);
        let first = match &img {
            None => None,
            Some(crate::image_cache::DecodedImage::Static(i)) => Some(Arc::clone(i)),
            // FRAME-5: многокадровый GIF — первый кадр идёт в `decoded_images`
            // как раньше, но теперь вся анимация ЕДЕТ ДАЛЬШЕ (было: «наружу не
            // отдаётся» — сама `gif` отбрасывалась) под тем же ключом, чтобы
            // `redraw_requested` мог тикать её так же, как страничные GIF.
            Some(crate::image_cache::DecodedImage::Animated { first, gif }) => {
                animated_gifs.push((key.clone(), (**gif).clone()));
                Some(Arc::clone(first))
            }
        };
        images.push((req.node_id, first.is_some()));
        if let Some(image) = first {
            if wants_intrinsic {
                lumen_layout::apply_intrinsic_size(doc, req.node_id, image.width, image.height);
            }
            decoded_images.push((key, image));
        }
    }
    // FRAME-5 срез 2: lazy `<img>` get a registration key too (no fetch), so
    // their not-yet-loaded placeholder is already rewritten by
    // `rekey_frame_images` at first paint — see doc-comment on
    // `FrameSubresourceOutcomes::lazy_requests`.
    for req in &lazy_requests {
        image_keys.push((req.url.clone(), frame_image_key(base, &req.url)));
    }
    FrameSubresourceOutcomes {
        links,
        images,
        css,
        decoded_images,
        image_keys,
        animated_gifs,
        lazy_requests,
        blocked_by_img_src,
        blocked_by_style_src,
        blocked_inline_style_policies,
        blocked_style_attr_policies,
        frame_ancestors_blocked: false,
    }
}

/// Ключ регистрации картинки под-документа фрейма (BUG-480 срез 15):
/// РАЗРЕШЁННЫЙ относительно базы РЕБЁНКА адрес, а не сырой `src`.
///
/// Ключ картинки в `IMAGE_CACHE`, в `Renderer::register_image` и в
/// `DisplayCommand::DrawImage.src` у страницы — сырое значение атрибута, а оно
/// уникально только внутри ОДНОГО документа: страница и фрейм из другого
/// каталога легко держат каждый свой `<img src="pic.png">`. С общим ключом
/// побеждала бы картинка страницы, причём молча. Разрешённый адрес разводит их
/// и, наоборот, СХЛОПЫВАЕТ действительно один и тот же файл — тогда декод
/// разделяется, как и задумано кэшем.
pub(crate) fn frame_image_key(base: &ResourceBase, raw_src: &str) -> String {
    base.resolve_str(raw_src)
}

/// FRAME-5: fetch + decode `background-image: url(...)`/`cross-fade()` of a
/// frame's OWN layout tree — the same picker the page uses
/// ([`fetch_and_decode_background_images`]), keyed by [`frame_image_key`] so a
/// frame and the page (or two frames) referencing the same relative path do
/// not collide in the shared image registry, mirroring how
/// [`fetch_frame_subresources`] already keys `<img>`.
///
/// Runs against the layout tree ([`collect_background_image_requests`] reads
/// `LayoutBox::style.background_layers`, not the DOM), so — unlike `<img>`/
/// `<link>` (fetched pre-layout, срез 11) — the caller must have a layout
/// already. `spawn_frame` calls this once, right after its own synchronous
/// initial layout: a `background-image` set or changed by a later
/// relayout/mutation inside this frame is not picked up. BUG-939 fixed the
/// same gap for the top-level page (`spawn_dynamic_background_image_loads`,
/// hooked into every relayout via `relayout.rs`) but did not extend to
/// frames — this is that residual, not a regression.
///
/// Returns `(images, raw_to_key, blocked_by_img_src)`: `images` is the
/// `LoadedPage::images`-shaped list to fold into [`FrameHandle::images`];
/// `raw_to_key` extends [`FrameHandle::image_keys`] so
/// [`rekey_frame_images`] rewrites `DrawBackgroundImage`/`DrawCrossFade`
/// sources the same way it already rewrites `DrawImage`; `blocked_by_img_src`
/// is the resolved URL of every `background-image` the CHILD's own
/// `img-src`/`default-src` policy blocked (GAP-CSPENF срез 25 — срез 18 named
/// this exact gap as not covered: it gated the top-level page's background
/// images only). `csp_gate` is the CHILD's own policy, computed once by the
/// caller (same one-shot read as [`fetch_frame_subresources`]'s `csp_gate`).
#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)] // GAP-REFERRER срез 5 added the 8th, docs/lint-policy.md §10
pub(crate) fn fetch_frame_background_images(
    layout: &lumen_layout::LayoutBox,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    target: lumen_core::ColorSpace,
    csp_gate: Option<&(Vec<lumen_network::csp::CspPolicy>, String)>,
    self_origin: Option<&lumen_network::Origin>,
    referrer_policy: lumen_network::ReferrerPolicy,
) -> (
    Vec<(String, Arc<lumen_image::Image>)>,
    Vec<(String, String)>,
    Vec<String>,
) {
    let urls = lumen_layout::collect_background_image_requests(layout, 1.0);
    let decoded = parallel_map(&urls, |_, url| {
        // GAP-CSPENF срез 44: same upgrade-before-gate order as `<img>` above
        // and the page's `fetch_and_decode_background_images` (срез 43 left
        // this exact producer named as not covered).
        let resolved = base.resolve_str(url);
        let upgraded =
            csp_gate.and_then(|(policy, _)| crate::csp_enforce::upgrade_insecure_url(policy, &resolved));
        if let Some((policy, _)) = csp_gate {
            let resolved = upgraded.clone().unwrap_or_else(|| resolved.clone());
            if crate::csp_enforce::img_src_blocked(policy, &resolved, self_origin) {
                return (None, Some(resolved));
            }
        }
        let fetch_url: &str = upgraded.as_deref().unwrap_or(url);
        let bytes = match fetch_image_bytes(fetch_url, base, sink, cookie_jar.clone(), referrer_policy) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("iframe: пропуск bg-картинки {url}: {e}");
                return (None, None);
            }
        };
        let image = match lumen_image::decode_to(&bytes, target) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("iframe: не декодируется bg-картинка {url}: {e}");
                return (None, None);
            }
        };
        eprintln!(
            "iframe: загружена bg-картинка: {url} ({}×{}, {:?})",
            image.width, image.height, image.format
        );
        (Some((url.clone(), frame_image_key(base, url), Arc::new(image))), None)
    });
    let mut images = Vec::new();
    let mut raw_to_key = Vec::new();
    let mut blocked_by_img_src = Vec::new();
    for (loaded, blocked_url) in decoded {
        if let Some(url) = blocked_url {
            blocked_by_img_src.push(url);
        }
        if let Some((raw, key, image)) = loaded {
            raw_to_key.push((raw, key.clone()));
            images.push((key, image));
        }
    }
    (images, raw_to_key, blocked_by_img_src)
}

/// Доставить исходы подресурсов фрейма ([`fetch_frame_subresources`]) его
/// JS-контексту (BUG-480 срез 11).
///
/// Стили идут через `_lumen_deliver_parser_link_events` — тот же проход, что у
/// top-level после каскада (пер-узловой флаг «уже отчитался» внутри шима гасит
/// двойной отчёт для ссылок, вставленных скриптом ребёнка); картинки — через
/// `_lumen_resource_fire`, как парсерные `<script src>` (BUG-804). Зеркало
/// среза 10 внутри `_lumen_resource_fire` автоматически доставит те же события
/// обработчикам фасадов родителя.
fn deliver_frame_subresource_events(js: &Arc<dyn PersistentJs>, sub: &FrameSubresourceOutcomes) {
    use std::fmt::Write as _;
    if !sub.links.is_empty() {
        let mut arg = String::with_capacity(sub.links.len() * 8 + 40);
        arg.push_str("_lumen_deliver_parser_link_events([");
        for (i, (node, ok)) in sub.links.iter().enumerate() {
            if i > 0 {
                arg.push(',');
            }
            let _ = write!(arg, "{},{}", node.index(), u8::from(*ok));
        }
        arg.push_str("]);");
        js.eval_js(&arg);
    }
    for (node, ok) in &sub.images {
        let kind = if *ok { "load" } else { "error" };
        js.eval_js(&format!("_lumen_resource_fire({}, '{kind}');", node.index()));
    }
}

/// Измеритель для layout под-документа фрейма: bundled Inter + системные
/// face-ы, как у страницы ([`page_measurer`]), плюс `@font-face`-шрифты ребёнка
/// (FRAME-5) — `local()` из `font_registry`, `url()` из уже скачанных
/// `web_fonts` ([`load_frame_fonts`]).
///
/// Вызывается на КАЖДОМ пересчёте layout ребёнка (как страница пересобирает
/// свой измеритель на каждом relayout в `compute_layout`), но без сети:
/// `font_registry`/`web_fonts` посчитаны один раз в `spawn_frame` и просто
/// читаются здесь — иначе `sync_frame_viewports` бил бы по сети на каждый
/// ресайз/скролл страницы.
///
/// `None` — шрифт не разобрался; вызывающая сторона тогда просто не считает
/// geometry (лог в stderr), а не валит загрузку страницы.
fn frame_measurer(
    font_faces: &[lumen_css_parser::FontFaceRule],
    font_registry: &lumen_font::FontRegistry,
    web_fonts: &[LoadedWebFont],
) -> Option<lumen_paint::MultiFontMeasurer> {
    match lumen_font::Font::parse(INTER_FONT) {
        Ok(font) => {
            let mut measurer = page_measurer(&font, web_fonts);
            for rule in font_faces {
                if !rule.family.is_empty()
                    && let Some(bytes) = font_registry.face_bytes_for_family(&rule.family)
                {
                    let ranges = rule
                        .unicode_range
                        .as_deref()
                        .map(lumen_font::parse_unicode_ranges)
                        .unwrap_or_default();
                    // CSS Fonts L4 §14 (FONTLOAD-11/12/13, BUG-467): ascent/descent/line-gap-override, size-adjust.
                    let ascent_override = rule.ascent_override.as_deref()
                        .and_then(lumen_font::parse_metric_override_percent);
                    let descent_override = rule.descent_override.as_deref()
                        .and_then(lumen_font::parse_metric_override_percent);
                    let size_adjust = rule.size_adjust.as_deref()
                        .and_then(lumen_font::parse_metric_override_percent);
                    let line_gap_override = rule.line_gap_override.as_deref()
                        .and_then(lumen_font::parse_metric_override_percent);
                    measurer.register_family_with_overrides(
                        &rule.family, bytes, ranges, ascent_override, descent_override, size_adjust,
                        line_gap_override,
                    );
                }
            }
            Some(measurer)
        }
        Err(e) => {
            eprintln!("iframe: сбой измерителя шрифта, geometry ребёнка не посчитана: {e}");
            None
        }
    }
}

/// Синхронно грузит `@font-face` ребёнка (FRAME-5): `local()` — уже
/// синхронно внутри [`load_font_faces`] (системный индекс в памяти), `url()` —
/// блокирующим fetch здесь же, тем же приёмом, что [`fetch_frame_subresources`]
/// уже применяет к картинкам ребёнка.
///
/// В отличие от страницы (PH3-19: async fetch + `FontLoaded` + FOUT-relayout,
/// чтобы не держать первый paint), у фрейма загрузка и так уже синхронная до
/// первого layout (срезы 11/12 — картинки и стили). Заводить отдельный
/// async+relayout канал ради одних лишь шрифтов было бы непропорционально
/// M-размеру этой задачи; расплата — фрейм с медленным веб-шрифтом чуть дольше
/// показывает первый paint, а не мигает FOUT (в обмен де-факто лучший UX).
/// GAP-CSPENF срез 25: `csp_gate` is the CHILD's own policy (computed once by
/// the caller, same one-shot read as [`fetch_frame_subresources`]'s
/// `csp_gate`) — `font-src`/`default-src` against a frame's own `@font-face
/// url()` was named as not covered by срез 19 (which only gated the
/// top-level page's fonts). `local()` sources are unaffected, same as the
/// top-level path — CSP's fetch directives govern network fetches, not the
/// system font lookup `load_font_faces` already resolved above. Returns the
/// resolved URL of every blocked source alongside the registry/web-fonts, so
/// the caller can dispatch `securitypolicyviolation` once its JS runtime
/// exists (this function runs before that, same ordering constraint as
/// `fetch_frame_subresources`'s `blocked_by_img_src`).
pub(crate) fn load_frame_fonts(
    font_faces: &[lumen_css_parser::FontFaceRule],
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    csp_gate: Option<&(Vec<lumen_network::csp::CspPolicy>, String)>,
    self_origin: Option<&lumen_network::Origin>,
    referrer_policy: lumen_network::ReferrerPolicy,
) -> (lumen_font::FontRegistry, Vec<LoadedWebFont>, Vec<String>) {
    let (registry, pending) = load_font_faces(font_faces, base, sink, cookie_jar.clone());
    let mut blocked_by_font_src = Vec::new();
    let mut web_fonts = Vec::with_capacity(pending.len());
    for pf in pending {
        let resolved = base.resolve_str(&pf.url);
        // GAP-CSPENF срез 48: то же переписывание `http://` в `https://`
        // до гейта `font-src`, что срез 48 дал top-level `@font-face` —
        // `gate_url` и есть адрес, который реально уходит в `fetch_font_bytes`.
        let gate_url = csp_gate
            .and_then(|(policy, _)| crate::csp_enforce::upgrade_insecure_url(policy, &resolved))
            .unwrap_or_else(|| resolved.clone());
        if let Some((policy, _)) = csp_gate
            && crate::csp_enforce::font_src_blocked(policy, &gate_url, self_origin)
        {
            blocked_by_font_src.push(gate_url);
            continue;
        }
        let Ok(raw) = fetch_font_bytes(&gate_url, base, sink, cookie_jar.clone(), referrer_policy) else {
            continue;
        };
        let bytes = match lumen_font::maybe_decode_font(&raw) {
            Ok(Some(d)) => d,
            Ok(None) => raw,
            Err(e) => {
                eprintln!("iframe @font-face «{}»: WOFF-декод провалился: {e}", pf.family);
                continue;
            }
        };
        if lumen_font::Font::parse(&bytes).is_err() {
            eprintln!("iframe @font-face «{}»: невалидный sfnt {}", pf.family, pf.url);
            continue;
        }
        let unicode_range = pf
            .unicode_range_str
            .as_deref()
            .map(lumen_font::parse_unicode_ranges)
            .unwrap_or_default();
        // CSS Fonts L4 §14 (FONTLOAD-11/12/13, BUG-467): ascent/descent/line-gap-override, size-adjust.
        let ascent_override = pf.ascent_override_str.as_deref()
            .and_then(lumen_font::parse_metric_override_percent);
        let descent_override = pf.descent_override_str.as_deref()
            .and_then(lumen_font::parse_metric_override_percent);
        let size_adjust = pf.size_adjust_str.as_deref()
            .and_then(lumen_font::parse_metric_override_percent);
        let line_gap_override = pf.line_gap_override_str.as_deref()
            .and_then(lumen_font::parse_metric_override_percent);
        web_fonts.push(LoadedWebFont {
            family: pf.family, weight: pf.weight, style: pf.style, unicode_range,
            ascent_override, descent_override, size_adjust, line_gap_override, bytes,
        });
    }
    (registry, web_fonts, blocked_by_font_src)
}

/// Посчитать cascade + layout под-документа фрейма на заданном вьюпорте и
/// отдать снимок прямоугольников JS-контексту ребёнка (BUG-480 срезы 12/13/14).
///
/// Результат ВОЗВРАЩАЕТСЯ, а не выбрасывается (срез 14): по нему рисуется
/// display list ребёнка и в нём же ищется host-бокс вложенного фрейма
/// (`NodeId` уникален только внутри своего документа, поэтому вложенному фрейму
/// нужен именно layout его собственного родителя, а не страницы).
///
/// `js` необязателен: у фрейма без скриптов JS-контекста нет, но layout ему
/// нужен ровно так же — его содержимое всё равно попадает на экран.
///
/// Интерактивное состояние ОДНОГО под-документа: узлы его собственного дерева,
/// под которыми курсор, в которых фокус и которые нажаты (BUG-480 срез 23).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FrameNodeState {
    /// Узел под курсором — `:hover`.
    pub(crate) hovered: Option<NodeId>,
    /// Узел с фокусом — `:focus` (и `:focus-within` у его предков).
    pub(crate) focused: Option<NodeId>,
    /// Нажатый узел — `:active`.
    pub(crate) active: Option<NodeId>,
}

/// Интерактивное состояние ВСЕХ под-документов — то, что знает [`crate::lumen::Lumen`]
/// и что должно доехать до каскада конкретного ребёнка (BUG-480 срез 23).
///
/// Каждое поле адресует узел парой `(индекс фрейма, узел ЕГО документа)` —
/// `NodeId` уникален лишь внутри своего документа, поэтому одного `NodeId`
/// здесь недостаточно (та же причина, по которой у `hovered_frame`/
/// `focused_frame` эта пара уже была).
#[derive(Clone, Copy, Default)]
pub(crate) struct FrameInteractive {
    /// `Lumen::hovered_frame` — узел под курсором внутри под-документа.
    pub(crate) hovered: Option<(usize, NodeId)>,
    /// `Lumen::focused_frame` — узел с фокусом внутри под-документа.
    pub(crate) focused: Option<(usize, NodeId)>,
    /// `Lumen::active_frame` — нажатый узел внутри под-документа.
    pub(crate) active: Option<(usize, NodeId)>,
}

impl FrameInteractive {
    /// Состояние КОНКРЕТНОГО фрейма: узел соседнего фрейма для этого прохода —
    /// просто «ничего», а не чужой `NodeId` с совпавшим индексом.
    fn for_frame(self, idx: usize) -> FrameNodeState {
        let pick =
            |v: Option<(usize, NodeId)>| v.filter(|(i, _)| *i == idx).map(|(_, n)| n);
        FrameNodeState {
            hovered: pick(self.hovered),
            focused: pick(self.focused),
            active: pick(self.active),
        }
    }
}

/// Лок дерева держится ровно на время прохода: `update_layout_rects` уходит уже
/// без него, потому что это вызов на JS-поток ребёнка.
///
/// `state` — интерактивное состояние ЭТОГО под-документа (BUG-480 срез 23).
/// Ставится и снимается вокруг одного прохода: `lumen_layout` держит его в
/// thread-local на весь процесс, поэтому оставленное состояние ребёнка
/// досталось бы следующему проходу страницы (та же причина, по которой так
/// делает хром — см. `relayout_chrome_host`).
///
/// Вычисленные стили публикуются здесь же, рядом с прямоугольниками: без них
/// `getComputedStyle` внутри фрейма отдавал пустую строку для ЛЮБОГО свойства
/// любого узла — независимо от интерактивного состояния (измерено пробой
/// `verify_frame_focus_style.py` до правки).
#[allow(clippy::unwrap_used)] // короткий лок дерева, docs/lint-policy.md §10
fn layout_frame_document(
    doc: &Arc<Mutex<Document>>,
    sheet: &lumen_css_parser::Stylesheet,
    viewport: lumen_core::geom::Size,
    js: Option<&Arc<dyn PersistentJs>>,
    measurer: &lumen_paint::MultiFontMeasurer,
    state: FrameNodeState,
) -> lumen_layout::LayoutBox {
    let (frame_layout, rects, client_rects, styles, pseudo_styles) = {
        let d = doc.lock().unwrap();
        lumen_layout::set_interactive_state(state.hovered, state.focused, state.active);
        let (frame_layout, counters) =
            lumen_layout::layout_measured_with_counters(&d, sheet, viewport, measurer);
        lumen_layout::clear_interactive_state();
        let rects = lumen_layout::collect_layout_rects(&frame_layout, &d);
        let client_rects = lumen_layout::collect_client_rects(&frame_layout, &d);
        let styles = lumen_layout::collect_computed_styles(&frame_layout, &d, Some(&counters), viewport);
        let pseudo_styles = lumen_layout::collect_pseudo_computed_styles(&frame_layout);
        (frame_layout, rects, client_rects, styles, pseudo_styles)
    };
    if let Some(js) = js {
        js.update_layout_rects(rects);
        js.update_client_rects(client_rects);
        js.update_hit_test_tree(Arc::new(frame_layout.clone()));
        js.update_computed_styles(styles);
        js.update_pseudo_computed_styles(pseudo_styles);
        js.update_viewport_size(viewport.width, viewport.height);
    }
    frame_layout
}

/// FRAME-5 срез 2: re-register the frame's lazy `<img>` set with its OWN
/// `IntersectionObserver` shim and drain whatever entered the proximity
/// margin since the last call. `register_lazy_images` is idempotent per node
/// id (`_lazy_io_urls[nid] === undefined` guard, `web_api_shim_tail_mc.js`),
/// so calling it again on every relayout is harmless — the observer itself is
/// what tracks "already fired".
///
/// Must run AFTER [`layout_frame_document`] has pushed fresh rects/viewport
/// into `js` — `deliver_layout_observers()` is what fires the observer's
/// entries, and it reads that geometry. `pub(crate)`, not module-private:
/// `Lumen::apply_frame_scroll` (`lumen/scrolling.rs`) also calls it — a
/// scrolled-into-view lazy `<img>` needs the SAME re-check as a relayout,
/// geometry unchanged but scroll position (already pushed by
/// `set_page_scroll_y` there) is what moved.
pub(crate) fn harvest_frame_lazy_requests(
    js: Option<&Arc<dyn PersistentJs>>,
    lazy_requests: &[lumen_layout::ImageRequest],
) -> Vec<(u32, String)> {
    let Some(js) = js else { return Vec::new() };
    if lazy_requests.is_empty() {
        return Vec::new();
    }
    let pairs: Vec<(u32, &str)> =
        lazy_requests.iter().map(|r| (r.node_id.index() as u32, r.url.as_str())).collect();
    js.register_lazy_images(&pairs);
    js.deliver_layout_observers();
    js.deliver_lazy_images();
    js.take_lazy_image_requests()
}

/// КОНТЕНТНЫЙ бокс host-элемента `<iframe>`/`<frame>` в layout родителя —
/// вьюпорт под-документа по HTML LS §4.8.5 и одновременно место, куда
/// вклеивается его display list (срез 14).
///
/// `LayoutBox::rect` — border-бокс, поэтому вычитаются рамки и padding. Порядок
/// операций повторяет приватную `content_box_rect` из `display_list.rs`
/// побитово: срез 14 ищет по этому прямоугольнику команду-заглушку в готовом
/// display list родителя, а сравнение чисел с плавающей точкой переживает
/// перестановку слагаемых не всегда.
pub(crate) fn host_content_rect(b: &lumen_layout::LayoutBox) -> Rect {
    let s = &b.style;
    Rect::new(
        b.rect.x + s.border_left_width + s.padding_left.px(),
        b.rect.y + s.border_top_width + s.padding_top.px(),
        (b.rect.width
            - s.border_left_width
            - s.border_right_width
            - s.padding_left.px()
            - s.padding_right.px())
        .max(0.0),
        (b.rect.height
            - s.border_top_width
            - s.border_bottom_width
            - s.padding_top.px()
            - s.padding_bottom.px())
        .max(0.0),
    )
}

/// Пересчитать layout под-документов фреймов под РЕАЛЬНЫЙ размер их host-бокса
/// (BUG-480 срез 13).
///
/// Срез 12 считал geometry ребёнка на UA-дефолтном [`FRAME_UA_DEFAULT_SIZE`],
/// потому что [`load_frame_sub_documents`] идёт ДО layout страницы-родителя и
/// настоящего размера бокса ещё не знает. Здесь он уже известен: проход
/// вызывается сразу после layout родителя — и на первой загрузке
/// (`parse_and_layout`), и на каждом последующем relayout
/// ([`Lumen::apply_relayout_result`]), поэтому `width:100%`-фрейм переживает
/// ресайз окна, смену зума и любое движение вёрстки над ним.
///
/// Пересчёт идёт ТОЛЬКО когда контентный бокс хоста реально изменился
/// (`FrameHandle::viewport` — размер последнего посчитанного прохода): relayout
/// случается на каждый кадр анимации, а layout под-документа стоит примерно
/// столько же, сколько layout страницы его размера.
///
/// Обход идёт ПО ВОЗРАСТАНИЮ глубины (срез 14): host-элемент фрейма глубины
/// `d` живёт в документе фрейма глубины `d-1`, а `NodeId` уникален только
/// внутри своего документа — искать его в layout страницы значило бы найти либо
/// ничего, либо чужой бокс с совпавшим индексом. Поэтому вложенному фрейму
/// нужен уже пересчитанный layout его собственного родителя, а он готов ровно
/// после прохода предыдущей глубины.
///
/// Display list ребёнка собирается ПОСЛЕ всех layout-ов и в обратном порядке
/// глубин: в него вклеивается содержимое его собственных вложенных фреймов,
/// значит те должны быть нарисованы раньше.
pub(crate) fn sync_frame_viewports(
    frames: &mut [FrameHandle],
    page_layout: &lumen_layout::LayoutBox,
    interactive: FrameInteractive,
) {
    if frames.is_empty() {
        return;
    }
    // «Layout пересчитан на этом проходе» — гейт для пересборки display list:
    // перерисовывать нужно и сам фрейм, и каждого его предка (его содержимое
    // вклеено в их списки).
    let mut relaid = vec![false; frames.len()];
    for depth in 0..=MAX_FRAME_DEPTH {
        // Фаза 1 — только чтение: где стоит host-бокс каждого фрейма этой
        // глубины. Отдельно от записи, потому что для глубины ≥ 1 читается
        // ЧУЖОЙ элемент того же среза (`layout` фрейма-родителя).
        let mut plan: Vec<(usize, Rect)> = Vec::new();
        for (i, h) in frames.iter().enumerate() {
            if h.depth != depth {
                continue;
            }
            let host = match &h.parent_doc {
                None => crate::forms::find_layout_box(page_layout, h.host),
                Some(pd) => frames
                    .iter()
                    .find(|o| Arc::ptr_eq(&o.doc, pd))
                    .and_then(|p| p.layout.as_ref())
                    .and_then(|pl| crate::forms::find_layout_box(pl, h.host)),
            };
            if let Some(b) = host {
                plan.push((i, host_content_rect(b)));
            }
        }
        // Фаза 2 — запись.
        for (i, rect) in plan {
            // Положение хоста пишется ВСЕГДА, а не только при смене размера:
            // фрейм может уехать вниз, не изменив габаритов (что-то над ним
            // выросло), и тогда вклеивать его содержимое надо по новому адресу.
            frames[i].host_rect = Some(rect);
            // Схлопнутый бокс (`display:none`, нулевые атрибуты) вьюпортом быть
            // не может — ребёнок остаётся на прежнем размере, а не считается в 0.
            if rect.width <= 0.0 || rect.height <= 0.0 {
                continue;
            }
            let size = lumen_core::geom::Size::new(rect.width, rect.height);
            // Гейт «ничего не изменилось — не пересчитывать» сравнивает ДВА
            // входа прохода, а не один (BUG-480 срез 23): размер host-бокса и
            // интерактивное состояние ребёнка. Каскад `:hover`/`:focus`/
            // `:active` — такой же вход layout, как вьюпорт, и без второй
            // половины сравнения клик внутрь фрейма менял бы состояние, а
            // пересчёта не вызывал: размер-то остался прежним.
            let state = interactive.for_frame(i);
            // FRAME-1: тот же вход, что и гейт ниже, нужен ОТДЕЛЬНО от него —
            // гейт пропускает пересчёт и при смене одного интерактива без
            // смены размера, а `resize` должен сработать только на РЕАЛЬНОЕ
            // изменение вьюпорта (включая самый первый проход этой функции —
            // переход с UA-дефолта [`FRAME_UA_DEFAULT_SIZE`] на настоящий
            // host-бокс, тот самый переход, из-за которого страница, кэширующая
            // размер в обработчике `load`, застревала на 300x150: к этому
            // моменту `load` ребёнка уже отработал на старом размере).
            let viewport_changed = (size.width - frames[i].viewport.width).abs() >= 0.01
                || (size.height - frames[i].viewport.height).abs() >= 0.01;
            if !viewport_changed && frames[i].interactive == state && frames[i].layout.is_some() {
                continue;
            }
            // FRAME-5: no longer memoized across frames — each frame can carry
            // its own `@font-face` set (`frames[i].sheet.font_faces`), so a
            // measurer built for frame A would silently miss frame B's fonts.
            let Some(measurer) = frame_measurer(
                &frames[i].sheet.font_faces,
                &frames[i].font_registry,
                &frames[i].web_fonts,
            ) else {
                continue;
            };
            let layout = layout_frame_document(
                &frames[i].doc,
                &frames[i].sheet,
                size,
                frames[i].js.as_ref(),
                &measurer,
                state,
            );
            frames[i].scroll_containers = lumen_layout::collect_scroll_containers(&layout);
            frames[i].layout = Some(layout);
            frames[i].viewport = size;
            frames[i].interactive = state;
            relaid[i] = true;
            // FRAME-5 срез 2: fresh rects/viewport are in `js` now — proximity
            // check against them for lazy `<img>` of this frame.
            let pending = harvest_frame_lazy_requests(frames[i].js.as_ref(), &frames[i].lazy_requests);
            frames[i].pending_lazy.extend(pending);
            // FRAME-1: `resize` — событие ребёнку по HTML LS §7.4.4, счётчик
            // страницы ([`crate::app::window_event`]'s `WindowEvent::Resized`)
            // для его собственного вьюпорта. `update_viewport_size` внутри
            // [`layout_frame_document`] уже обновила прочитанные значения
            // (`window.innerWidth`/`innerHeight`) — событие лишь сообщает
            // скрипту, что их стоит перечитать.
            if viewport_changed && let Some(js) = frames[i].js.as_ref() {
                js.fire_window_resize();
            }
        }
    }
    rebuild_frame_display_lists(frames, &relaid);
    clamp_frame_scroll(frames);
}

/// Зажать прокрутку под-документов, оказавшуюся за новым пределом (срез 17):
/// содержимое стало ниже или вьюпорт выше.
///
/// Вызывается сразу после пересборки display list'ов и только после неё:
/// предел ([`frame_max_scroll`]) считается по ГОТОВОМУ списку ребёнка.
fn clamp_frame_scroll(frames: &mut [FrameHandle]) {
    for h in frames.iter_mut() {
        let max = frame_max_scroll(h);
        if h.scroll_y > max {
            h.scroll_y = max;
            if let Some(js) = h.js.as_ref()
                && js.set_page_scroll_y(max)
            {
                js.fire_window_scroll();
            }
        }
        // Горизонталь (FRAME-3 срез 1): нет JS-моста (`scroll_x` doc-comment),
        // так что только зажим числа — событие слать некому и нечего.
        let max_x = frame_max_scroll_x(h);
        if h.scroll_x > max_x {
            h.scroll_x = max_x;
        }
    }
}

/// Пересчитать под-документ фрейма `idx` после мутации ЕГО DOM — нативное
/// переключение элемента управления формы (BUG-480 срез 18).
///
/// Отличается от [`sync_frame_viewports`] тем, ЧТО изменилось: там менялся
/// размер host-бокса, здесь — само дерево ребёнка при неизменном вьюпорте,
/// то есть гейт «размер не менялся — не пересчитывать» пропустил бы правку
/// молча. Поэтому layout считается здесь напрямую, а `content_dl`
/// ОЧИЩАЕТСЯ: пустой список — единственный признак «перерисовать», который
/// понимает [`rebuild_frame_display_lists`], и через него правка сама доходит
/// до списков всех предков этого фрейма.
///
/// Дальше работу доделывает [`sync_frame_viewports`] — не ради экономии кода,
/// а потому что мутация могла подвинуть host-бокс ВЛОЖЕННОГО фрейма (раскрытый
/// `<details>` над ним), и порядок обхода по глубине живёт только там.
///
/// GAP-CSPENF срез 39: единственный вызывающий этой функции по мутации
/// скрипта — `about_to_wait.rs`'s `frame_dirty` (`own_dirty || bridge_dirty`,
/// тот же сигнал, что уже гейтит любую другую пост-скриптовую работу ребёнка)
/// — тот же тип триггера, что срез 37 дал странице через `dom_touched`.
/// `style_attr_csp_blocked` ребёнка считался только один раз, при спавне
/// (`fetch_frame_subresources`, до его собственных скриптов): точечная
/// мутация `style=""` (`setAttribute`/`style.cssText`) без затрагивания
/// `<style>`/`<link>` доезжала до layout незаблокированной. Полотно ребёнка
/// (`FrameHandle::sheet`) само не пересчитывается после спавна (CSSOM-1 ещё
/// не даёт живой per-node registry), так что здесь достаточно пере-собрать
/// только это множество узлов — не whole-каскад.
#[allow(clippy::unwrap_used)] // короткий лок дерева, docs/lint-policy.md §10
pub(crate) fn relayout_frame_content(
    frames: &mut [FrameHandle],
    idx: usize,
    page_layout: &lumen_layout::LayoutBox,
    interactive: FrameInteractive,
) {
    {
        let mut doc = frames[idx].doc.lock().unwrap();
        let root = doc.root();
        let csp_policy = crate::csp_enforce::document_csp_policy(&doc, root);
        let (blocked_style_attr_nodes, _) =
            collect_style_attr_csp_blocked(&doc, csp_policy.as_ref().map(|(p, _)| p.as_slice()));
        doc.set_style_attr_csp_blocked(blocked_style_attr_nodes);
    }
    let Some(measurer) = frame_measurer(
        &frames[idx].sheet.font_faces,
        &frames[idx].font_registry,
        &frames[idx].web_fonts,
    ) else {
        return;
    };
    let size = frames[idx].viewport;
    let state = interactive.for_frame(idx);
    let layout = layout_frame_document(
        &frames[idx].doc,
        &frames[idx].sheet,
        size,
        frames[idx].js.as_ref(),
        &measurer,
        state,
    );
    frames[idx].scroll_containers = lumen_layout::collect_scroll_containers(&layout);
    frames[idx].layout = Some(layout);
    frames[idx].interactive = state;
    frames[idx].content_dl.clear();
    // FRAME-5 срез 2: this frame's viewport/interactive state are unchanged
    // (only its tree mutated), so the `sync_frame_viewports` call below will
    // skip it on the "nothing changed" gate — harvest here or not at all.
    let pending = harvest_frame_lazy_requests(frames[idx].js.as_ref(), &frames[idx].lazy_requests);
    frames[idx].pending_lazy.extend(pending);
    sync_frame_viewports(frames, page_layout, interactive);
}

/// Пересобрать display list под-документов, чьё содержимое изменилось
/// (BUG-480 срез 14).
///
/// От глубокого к мелкому: в список фрейма вклеено содержимое его собственных
/// вложенных фреймов, поэтому те должны быть готовы раньше. Перерисовывается
/// фрейм, чей layout пересчитан на этом проходе, чей список ещё пуст (первый
/// проход после загрузки) — и любой, у кого перерисовался потомок.
/// `pub(crate)` (не только для [`sync_frame_viewports`]) с FRAME-3 среза 3:
/// прокрутка overflow-контейнера ВНУТРИ под-документа меняет его
/// `content_dl`, не вьюпорт, поэтому зовёт эту функцию напрямую с `relaid`,
/// взведённым только для своего индекса — та же пропагация «потомок
/// перерисовался» наверх по цепочке хостов уже здесь, второй такой не нужно.
pub(crate) fn rebuild_frame_display_lists(frames: &mut [FrameHandle], relaid: &[bool]) {
    let mut dirty: Vec<bool> = (0..frames.len())
        .map(|i| relaid[i] || frames[i].content_dl.is_empty())
        .collect();
    for depth in (0..=MAX_FRAME_DEPTH).rev() {
        for i in 0..frames.len() {
            if frames[i].depth != depth {
                continue;
            }
            let child_dirty = frames.iter().enumerate().any(|(j, c)| {
                dirty[j]
                    && c.parent_doc
                        .as_ref()
                        .is_some_and(|pd| Arc::ptr_eq(pd, &frames[i].doc))
            });
            if !dirty[i] && !child_dirty {
                continue;
            }
            let dl = {
                let Some(layout) = frames[i].layout.as_ref() else {
                    continue;
                };
                let mut dl = crate::display_list_metrics::paint_ordered(layout);
                // Срез 21: подложка под-документа на весь его вьюпорт — как
                // [`redraw_requested.rs`] чистит ВСЁ окно в canvas-цвет
                // страницы (CSS Backgrounds §3.11.1), а не только рамку
                // корневого бокса, здесь нужен тот же приём для фрейма:
                // `paint_ordered` кладёт фон `<html>`-бокса только в его
                // СОБСТВЕННОМ прямоугольнике, который короче вьюпорта, когда
                // содержимое ниже него — тогда без подложки сквозь фрейм
                // видно фон СТРАНИЦЫ (residual среза 14, найден пробой среза
                // 19). Белый по умолчанию — тот же UA-дефолт, которым
                // `canvas_background_color` документирует своё `None`.
                let vp = frames[i].viewport;
                let bg = lumen_layout::canvas_background_color(layout)
                    .unwrap_or(lumen_layout::style::Color::WHITE);
                dl.insert(
                    0,
                    lumen_paint::DisplayCommand::FillRect {
                        rect: lumen_core::geom::Rect { x: 0.0, y: 0.0, width: vp.width, height: vp.height },
                        color: bg,
                    },
                );
                // Срез 15: ключи картинок ребёнка — ДО вклейки содержимого его
                // вложенных фреймов. Их команды уже переписаны своими ключами
                // (список собирается от глубокого к мелкому), а заглушки
                // вложенных фреймов должны остаться со своим `src` — иначе
                // [`splice_one_frame`] их не найдёт.
                rekey_frame_images(&mut dl, frames, i);
                splice_children_of(&mut dl, frames, i);
                dl
            };
            frames[i].content_dl = dl;
            dirty[i] = true;
        }
    }
}

/// Переписать ключи картинок под-документа в его display list (BUG-480 срез 15).
///
/// `paint_ordered` кладёт в `DrawImage.src` сырое значение атрибута — ключ,
/// уникальный лишь внутри своего документа. Регистрируются картинки фрейма под
/// разрешённым адресом ([`frame_image_key`]), поэтому список надо привести к
/// тем же ключам, иначе рендерер не найдёт текстуру и нарисует серую заглушку.
///
/// Заглушки ВЛОЖЕННЫХ фреймов пропускаются по их `src`: [`splice_one_frame`]
/// ищет их именно по нему, и переписанный ключ означал бы серый прямоугольник
/// вместо содержимого внука. Совпасть `src` картинки и `src` фрейма могут
/// только в патологической разметке (`<img>` и `<iframe>` на один адрес), где
/// правильнее сохранить фрейм.
///
/// FRAME-5: `DrawBackgroundImage.src` и `DrawCrossFade.{src_a,src_b}` идут той
/// же переадресацией — `image_keys` теперь несёт и записи из
/// [`fetch_frame_background_images`], не только `<img>` — а заглушечная
/// проверка выше их не касается: `splice_one_frame` ищет заглушку вложенного
/// фрейма только среди `DrawImage`.
pub(crate) fn rekey_frame_images(dl: &mut DisplayList, frames: &[FrameHandle], idx: usize) {
    if frames[idx].image_keys.is_empty() {
        return;
    }
    let find_key = |src: &str| -> Option<String> {
        frames[idx].image_keys.iter().find(|(raw, _)| raw == src).map(|(_, key)| key.clone())
    };
    for cmd in dl.iter_mut() {
        match cmd {
            DisplayCommand::DrawImage { src, .. } => {
                if frames.iter().any(|h| {
                    h.parent_doc
                        .as_ref()
                        .is_some_and(|pd| Arc::ptr_eq(pd, &frames[idx].doc))
                        && &h.host_src == src
                }) {
                    continue;
                }
                if let Some(key) = find_key(src) {
                    *src = key;
                }
            }
            DisplayCommand::DrawBackgroundImage { src, .. } => {
                if let Some(key) = find_key(src) {
                    *src = key;
                }
            }
            DisplayCommand::DrawCrossFade { src_a, src_b, .. } => {
                if let Some(key) = find_key(src_a) {
                    *src_a = key;
                }
                if let Some(key) = find_key(src_b) {
                    *src_b = key;
                }
            }
            _ => {}
        }
    }
}

/// Что находится под точкой страницы (BUG-480 срез 16).
///
/// Один результат на оба вопроса, потому что задавать их порознь значит дважды
/// пройти hit-тестом по layout страницы, а спрашивают на каждом движении мыши.
pub(crate) struct PointerTarget {
    /// Hit-тест в layout СТРАНИЦЫ. Если точка во фрейме, это его host-элемент
    /// (для вложенного — самый внешний `<iframe>`): именно его фокусирует и
    /// подсвечивает родитель.
    pub(crate) page: Option<lumen_paint::HitTestResult>,
    /// Непусто, если точка попала в содержимое фрейма.
    pub(crate) frame: Option<FramePointerHit>,
}

/// Куда на самом деле указывает точка страницы, если она попала в СОДЕРЖИМОЕ
/// фрейма (BUG-480 срез 16).
pub(crate) struct FramePointerHit {
    /// Индекс хэндла в `Lumen::frames` — самого глубокого фрейма, накрывшего
    /// точку.
    pub(crate) frame: usize,
    /// Та же точка в координатах ВЬЮПОРТА под-документа — `clientX`/`clientY`
    /// события для скриптов ребёнка (CSSOM-View §10: отсчёт от левого верхнего
    /// угла окна просмотра, а не документа).
    ///
    /// Со срезом 17 это уже НЕ та система, в которой ищется [`Self::hit`]:
    /// hit-тест идёт по layout, который о прокрутке не знает (её применяет
    /// вклейка), поэтому там к точке прибавляется `scroll_y`, а наружу отдаётся
    /// вьюпортная. Пока фрейм не прокручен, оба ответа совпадают — потому срез
    /// 16 и обходился одним полем.
    pub(crate) client: Point,
    /// Hit-тест точки в layout под-документа. `None` — под точкой нет ни
    /// одного бокса ребёнка: событие всё равно принадлежит фрейму (родитель
    /// его не увидит), но адресовать его в под-документе некому.
    pub(crate) hit: Option<lumen_paint::HitTestResult>,
}

/// Одинаковый ли документ-хозяин у хэндла и у текущего шага спуска
/// (`None` — страница).
fn same_host_doc(handle: &Option<Arc<Mutex<Document>>>, cur: Option<&Arc<Mutex<Document>>>) -> bool {
    match (handle, cur) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

/// Перевести точку страницы в под-документ фрейма, если она туда попала
/// (BUG-480 срез 16).
///
/// Спуск идёт ПО ПОПАДАНИЮ В HOST-ЭЛЕМЕНТ, а не по перебору прямоугольников:
/// `hit_test` уже умеет z-index, `transform`, `pointer-events` и клипы, а
/// «содержит ли прямоугольник точку» не умеет ничего из этого — фрейм,
/// накрытый чужим позиционированным блоком, забирал бы клик себе.
///
/// Попадание в САМ host-бокс мимо его контентной части (рамка, padding) фреймом
/// не считается: там точка адресует `<iframe>` как элемент родителя.
///
/// `NodeId` уникален лишь внутри своего документа, поэтому кандидат ищется по
/// паре «host-узел + документ-хозяин»: у вложенного фрейма (глубина ≥ 1) хозяин
/// — документ его собственного фрейма-родителя, и совпадение одного лишь
/// индекса узла нашло бы чужой элемент.
pub(crate) fn pointer_target(
    frames: &[FrameHandle],
    page_layout: &lumen_layout::LayoutBox,
    page: Point,
) -> PointerTarget {
    let mut cur_layout = page_layout;
    let mut cur_doc: Option<&Arc<Mutex<Document>>> = None;
    let mut cur_pt = page;
    let mut page_hit: Option<lumen_paint::HitTestResult> = None;
    let mut best: Option<FramePointerHit> = None;
    // Шагов на один больше предельной глубины: последний завершает спуск и
    // проставляет `hit` даже фрейму самой глубокой вложенности.
    for step in 0..=MAX_FRAME_DEPTH + 1 {
        let hit = hit_test(cur_pt, cur_layout);
        if step == 0 {
            page_hit = hit.clone();
        }
        let descend = hit
            .as_ref()
            .and_then(|h| {
                frames
                    .iter()
                    .position(|f| f.host == h.node && same_host_doc(&f.parent_doc, cur_doc))
            })
            .and_then(|i| {
                let rect = frames[i].host_rect?;
                let layout = frames[i].layout.as_ref()?;
                let inside = cur_pt.x >= rect.x
                    && cur_pt.x < rect.right()
                    && cur_pt.y >= rect.y
                    && cur_pt.y < rect.bottom();
                inside.then_some((i, rect, layout))
            });
        let Some((i, rect, layout)) = descend else {
            // Спуск кончился: точка адресует обычный узел текущего документа.
            if let Some(b) = best.as_mut() {
                b.hit = hit;
            }
            return PointerTarget { page: page_hit, frame: best };
        };
        // Срез 17: та же прокрутка, что сдвигает содержимое при вклейке —
        // иначе клик по видимому блоку попадал бы в тот, что был на этом
        // месте до прокрутки.
        let client = Point::new(cur_pt.x - rect.x, cur_pt.y - rect.y);
        cur_pt = Point::new(client.x + frames[i].scroll_x, client.y + frames[i].scroll_y);
        cur_layout = layout;
        cur_doc = Some(&frames[i].doc);
        best = Some(FramePointerHit { frame: i, client, hit: None });
    }
    PointerTarget { page: page_hit, frame: best }
}

/// Вклеить содержимое всех под-документов ГЛУБИНЫ 0 в display list страницы
/// (BUG-480 срез 14) — вместо серой заглушки, которую `display_list.rs` рисует
/// для `BoxKind::Iframe`.
///
/// Вызывается на каждой записи `Lumen::display_list`, а не один раз на загрузку:
/// список страницы пересобирается из layout при каждом relayout и о фреймах
/// ничего не знает.
///
/// Идемпотентна: заглушка ищется по своей команде, а после вклейки её там
/// больше нет — повторный проход по уже склеенному списку ничего не делает.
pub(crate) fn splice_frame_content(dl: &mut DisplayList, frames: &[FrameHandle]) {
    for h in frames.iter().filter(|h| h.parent_doc.is_none()) {
        splice_one_frame(dl, h);
    }
}

/// То же для вложенных фреймов: вклеить в список фрейма `parent` содержимое
/// тех фреймов, чей host-элемент лежит в ЕГО документе.
fn splice_children_of(dl: &mut DisplayList, frames: &[FrameHandle], parent: usize) {
    for h in frames.iter().filter(|h| {
        h.parent_doc
            .as_ref()
            .is_some_and(|pd| Arc::ptr_eq(pd, &frames[parent].doc))
    }) {
        splice_one_frame(dl, h);
    }
}

/// Заменить команду-заглушку одного `<iframe>`/`<frame>` на содержимое его
/// под-документа.
///
/// Заглушка — `DrawImage` с ключом-`src` элемента по его контентному боксу
/// (`display_list.rs`, ветка `BoxKind::Iframe`): нерегистрированный ключ
/// рисуется серым. Ищется по ПАРЕ «тот же `src` + тот же прямоугольник» —
/// одного `src` мало (два `<iframe src="">` на странице — обычное дело), одного
/// прямоугольника мало для гарантии, что это именно заглушка, а не совпавшая по
/// геометрии картинка.
///
/// Координаты ребёнка начинаются от его собственного (0, 0), поэтому вокруг
/// содержимого встают `PushClipRect` (в системе координат родителя — клип
/// применяется ДО трансформы) и `PushTransform` на смещение к боксу.
///
/// Прокрутка под-документа (срез 17) входит в ЭТО смещение, а не в клип:
/// клип — это окно фрейма на странице, оно на месте, а уезжает содержимое.
fn splice_one_frame(dl: &mut DisplayList, h: &FrameHandle) {
    let Some(rect) = h.host_rect else { return };
    if h.content_dl.is_empty() || rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let Some(at) = dl.iter().position(|c| match c {
        DisplayCommand::DrawImage { rect: r, src, .. } => {
            src == &h.host_src
                && (r.x - rect.x).abs() < 0.01
                && (r.y - rect.y).abs() < 0.01
                && (r.width - rect.width).abs() < 0.01
                && (r.height - rect.height).abs() < 0.01
        }
        _ => false,
    }) else {
        return;
    };
    let mut wrapped: DisplayList = Vec::with_capacity(h.content_dl.len() + 4);
    wrapped.push(DisplayCommand::PushClipRect { rect });
    wrapped.push(DisplayCommand::PushTransform {
        matrix: lumen_layout::Mat4::translation_2d(rect.x - h.scroll_x, rect.y - h.scroll_y),
    });
    wrapped.extend(h.content_dl.iter().cloned());
    wrapped.push(DisplayCommand::PopTransform);
    wrapped.push(DisplayCommand::PopClip);
    dl.splice(at..at + 1, wrapped);
}

/// Начало координат под-документа фрейма `idx` в системе координат документа
/// СТРАНИЦЫ (BUG-480 срез 20).
///
/// Тот же путь, которым [`splice_one_frame`] везёт пиксели, только сложенный
/// по цепочке хозяев и без клипов: точка `p` под-документа лежит на странице в
/// `p + frame_page_origin(idx)`. Нужен всему, что рисуется НЕ внутри фрейма, а
/// поверх страницы, но привязано к узлу ребёнка — сейчас это подсказка о
/// непройденной валидации формы.
///
/// Прокрутка вычитается на КАЖДОМ шаге и своя у каждого уровня: содержимое
/// фрейма сдвигает его собственная `scroll_y`, а сам фрейм внутри хозяина —
/// уже хозяйская. `None` — у какого-то звена цепочки ещё нет host-бокса
/// (layout не посчитан) либо хозяин не найден: тогда переводить нечего.
pub(crate) fn frame_page_origin(frames: &[FrameHandle], idx: usize) -> Option<(f32, f32)> {
    let (mut x, mut y) = (0.0_f32, 0.0_f32);
    let mut cur = idx;
    // Ограничение шагов — та же защита от петли, что у `MAX_FRAME_DEPTH` при
    // загрузке: цепочка `parent_doc` строится кодом, но идти по ней вечно
    // нельзя даже теоретически.
    for _ in 0..=MAX_FRAME_DEPTH {
        let h = frames.get(cur)?;
        let rect = h.host_rect?;
        x += rect.x - h.scroll_x;
        y += rect.y - h.scroll_y;
        let Some(pd) = h.parent_doc.as_ref() else { return Some((x, y)) };
        cur = frames.iter().position(|o| Arc::ptr_eq(&o.doc, pd))?;
    }
    None
}

/// Origin ХОСТ-БОКСА фрейма `idx` на странице (FRAME-3 remainder:
/// собственный scrollbar фрейма).
///
/// Не путать с [`frame_page_origin`]: та переводит точку СОДЕРЖИМОГО фрейма
/// (вычитает и его СОБСТВЕННЫЙ scroll), а этой нужен сам бокс — он не
/// двигается от прокрутки СВОЕГО содержимого, только от прокрутки ПРЕДКОВ.
/// Для фрейма страницы (`parent_doc: None`) это просто его `host_rect`
/// (координаты документа страницы); для вложенного — тот же бокс, сложенный
/// с origin-ом родителя (`frame_page_origin` родителя уже вычитает его
/// собственный scroll — то самое смещение, которому подчинён ЭТОТ
/// host-бокс, лежащий в документе родителя).
pub(crate) fn frame_box_page_origin(frames: &[FrameHandle], idx: usize) -> Option<(f32, f32)> {
    let h = frames.get(idx)?;
    let rect = h.host_rect?;
    match h.parent_doc.as_ref() {
        None => Some((rect.x, rect.y)),
        Some(pd) => {
            let parent = frames.iter().position(|o| Arc::ptr_eq(&o.doc, pd))?;
            let (px, py) = frame_page_origin(frames, parent)?;
            Some((px + rect.x, py + rect.y))
        }
    }
}

/// Высота содержимого под-документа фрейма `h` — та же величина, что
/// [`frame_max_scroll`] уже вычисляет для клампа, но без вычитания
/// viewport-а: `scrollbar::build_scrollbar_overlay` ожидает именно
/// content-height, а не max-scroll.
pub(crate) fn frame_content_height(h: &FrameHandle) -> f32 {
    frame_max_scroll(h) + h.viewport.height
}

/// Оверлей СОБСТВЕННОГО scrollbar-а каждого видимого фрейма (FRAME-3
/// remainder: "собственный скроллбар фрейма" — визуал).
///
/// Зеркало страничного `scrollbar::build_scrollbar_overlay`, вызванное на
/// геометрию КАЖДОГО `FrameHandle` вместо `Lumen`: те же pure-fn формулы,
/// свой viewport и свой content-height ([`frame_content_height`]), обёрнутые
/// в клип и трансляцию к боксу фрейма НА СТРАНИЦЕ — иначе полоса рисовалась
/// бы в (0,0) для каждого фрейма разом.
///
/// Origin — [`frame_box_page_origin`] (бокс ХОСТА, не его содержимого: свой
/// скролл фрейма не должен двигать полосу, только скролл ПРЕДКОВ), минус
/// `page_scroll_{x,y}` — та же "raw overlay" конвенция, что уже использует
/// страничный scrollbar и `build_validation_tooltip`
/// (`frame_form_submit.rs::show_frame_validation_tooltip` — тот же приём для
/// фреймовой tooltip-валидации): overlay viewport-locked, страничный
/// page-offset (панель вкладок) его не подхватывает — ни у страничного
/// scrollbar-а, ни у этого.
pub(crate) fn frame_scrollbar_overlay(
    frames: &[FrameHandle],
    page_scroll_x: f32,
    page_scroll_y: f32,
) -> DisplayList {
    let mut out: DisplayList = Vec::new();
    for (idx, h) in frames.iter().enumerate() {
        let Some(rect) = h.host_rect else { continue };
        if h.content_dl.is_empty() || rect.width <= 0.0 || rect.height <= 0.0 {
            continue;
        }
        let Some((ox, oy)) = frame_box_page_origin(frames, idx) else { continue };
        let bar = crate::scrollbar::build_scrollbar_overlay(
            h.scroll_y,
            frame_content_height(h),
            h.viewport.width,
            h.viewport.height,
        );
        if bar.is_empty() {
            continue;
        }
        let sx = ox - page_scroll_x;
        let sy = oy - page_scroll_y;
        out.push(DisplayCommand::PushClipRect {
            rect: Rect::new(sx, sy, h.viewport.width, h.viewport.height),
        });
        out.push(DisplayCommand::PushTransform {
            matrix: lumen_layout::Mat4::translation_2d(sx, sy),
        });
        out.extend(bar);
        out.push(DisplayCommand::PopTransform);
        out.push(DisplayCommand::PopClip);
    }
    out
}

/// Окружение загрузки под-документа: всё, что фрейм берёт у страницы, одним
/// `Clone`-значением (BUG-480 срез 19).
///
/// До среза этот десяток провайдеров передавался в [`load_frame_sub_documents`]
/// по отдельности и существовал ТОЛЬКО внутри `parse_and_layout` — то есть
/// повторить загрузку под-документа позже, из живого окна, было нечем. Ровно
/// этим и занимается навигация фрейма ([`navigate_frame`]), поэтому набор
/// собран в одно значение, которое переезжает в `LoadedPage` и дальше в
/// `Lumen`.
#[derive(Clone)]
pub(crate) struct FrameLoadEnv {
    /// Приёмник событий загрузки — тот же, что у страницы.
    pub(crate) sink: Arc<dyn EventSink>,
    /// Банка cookie сессии.
    pub(crate) cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    /// Провайдер `window.fetch()` под-документа.
    pub(crate) fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    /// Провайдер `new WebSocket()`.
    pub(crate) ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
    /// Провайдер `new EventSource()`.
    pub(crate) sse_provider: Option<Arc<dyn lumen_core::ext::JsSseProvider>>,
    /// `localStorage` origin-а страницы.
    pub(crate) ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    /// `sessionStorage` вкладки (BUG-836).
    pub(crate) ss_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    /// Бэкенд IndexedDB.
    pub(crate) idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    /// Бэкенд Service Worker.
    pub(crate) sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    /// Реестр живых SW-потоков.
    pub(crate) sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
    /// Бэкенд Cache Storage.
    pub(crate) cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
    /// Бэкенд Push API (срез 1 of P3-pushapi).
    pub(crate) push_backend: Option<Arc<dyn lumen_core::ext::PushBackend>>,
    /// Экранный media-контекст: гейт `<link media>` и `@media` каскада ребёнка.
    pub(crate) media_ctx: lumen_css_parser::MediaContext,
    /// Вьюпорт СТРАНИЦЫ — им picker выбирает `srcset`-кандидата картинок
    /// ребёнка (то же значение, с которым страница грузит свои).
    pub(crate) viewport: lumen_core::geom::Size,
    /// Гасить cookie-баннеры.
    pub(crate) cookie_banner_dismiss: bool,
    /// Детерминированный режим (`--deterministic`).
    pub(crate) deterministic: deterministic::DetConfig,
    /// `crossOriginIsolated` страницы.
    pub(crate) cross_origin_isolated: bool,
    /// BUG-480 срез 15: целевое цветовое пространство декодера картинок — то
    /// же, с которым страница декодирует свои (`parse_and_layout`).
    pub(crate) target: lumen_core::ColorSpace,
    /// База ВЕРХНЕГО окна: `window.top.location` фреймов глубины ≥ 1 и вторая
    /// сторона same-origin-проверки к нему.
    ///
    /// Отдельным полем, а не параметром рекурсии: в отличие от `base`, которая
    /// на каждом уровне своя, эта величина одна на страницу — и навигации
    /// фрейма (срез 19) взять её больше неоткуда, потому что `PageSource`
    /// вкладки не знает про редиректные хопы, через которые страница пришла.
    pub(crate) page_base: ResourceBase,
}

#[allow(clippy::unwrap_used)] // короткий лок дерева; poisoned mutex = паника потока загрузки, docs/lint-policy.md §10
pub(crate) fn load_frame_sub_documents(
    parent: &Arc<Mutex<Document>>,
    depth: usize,
    base: &ResourceBase,
    top_doc: &Arc<Mutex<Document>>,
    env: &FrameLoadEnv,
    parent_js: Option<&Arc<dyn PersistentJs>>,
) -> Vec<FrameHandle> {
    // Короткий лок: собираем описания фреймов и отпускаем дерево — дальше
    // сеть/скрипты/события, которые вправе читать документ.
    let infos = {
        let d = parent.lock().unwrap();
        collect_iframes(&d)
    };
    if infos.is_empty() {
        return Vec::new();
    }
    let mut handles = Vec::new();
    for info in infos {
        if info.loading_lazy {
            continue;
        }
        handles.extend(spawn_frame(&info, None, parent, depth, base, top_doc, env, parent_js, None));
    }
    handles
}

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
    // Источник HTML + база ребёнка для его относительных URL.
    let fetched = match &js_url_result {
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
                            fetch_iframe_source(
                                &href,
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
        lumen_html_parser::parse(&html)
    };
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
        true,
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
        // BUG-1118: iframe scripts not wired to the immediate-`<img src>`
        // hook yet — see `ImageLoadHook`'s doc comment for scope; sub-
        // documents keep relying on the post-relayout sweep only.
        None,
        // BUG-1119: the frame's `document.cookie` uses the jar its requests
        // go through; an opaque origin has no cookies (HTML LS §3.1.3).
        env.cookie_jar.clone().filter(|_| !opaque),
    );
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
    // Срез 3 BUG-480: ссылки на предков в контексте ребёнка — до его
    // DOMContentLoaded/load, чтобы обработчики (в т.ч. встроенный
    // testharness на window load) читали window.parent/top/frameElement
    // сразу. Инлайн-скрипты ребёнка к этому моменту уже исполнены и при
    // чтении видели прежний fallback (parent === window) — известное
    // ограничение среза.
    if let Some(js) = &child_js {
        let accessible_parent = frame_access_allowed(base, &child_url, opaque);
        // BUG-921: снимок атрибута `name` хоста на момент создания контекста —
        // `window.name` ребёнка запоминает его один раз (HTML LS §7.2.3), а не
        // перечитывает атрибут при каждом обращении.
        // BUG-979: peer — родитель, only когда сам доступен same-origin
        // (`accessible_parent`) — глобалы читаются исключительно same-origin,
        // натив ещё раз гейтит это явно, но не полагаться на второй слой
        // защиты, когда первый доступен бесплатно.
        let parent_peer = accessible_parent.then(|| parent_js.and_then(|js| js.frame_peer_bridge())).flatten();
        js.register_parent_document(
            info.node.index() as u32,
            Arc::clone(parent),
            &parent_url,
            info.name.as_deref(),
            accessible_parent,
            parent_peer,
        );
        // Ребёнок глубины ≥ 2 получает отдельный слот top: его верх —
        // корень страницы, а не непосредственный родитель.
        if depth >= 1 {
            let accessible_top = frame_access_allowed(&env.page_base, &child_url, opaque);
            // BUG-979: top's own runtime is not reachable here (only its doc
            // Arc is threaded down through `top_doc`) — `window.top`'s facade
            // keeps the IDL-only whitelist for now; scope stays contentWindow/
            // parent, the shapes this bug's WPT repro actually exercises.
            js.register_top_document(Arc::clone(top_doc), &top_url, accessible_top, None);
        }
    }
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
    let frame_sheet = lumen_css_parser::parse(&subresources.css);
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
    // FRAME-5: синхронно (см. doc-comment `load_frame_fonts`) — тем же
    // приёмом, что срез 11 уже применяет к картинкам и таблицам стилей
    // ребёнка выше в этой функции.
    let (font_registry, web_fonts, blocked_by_font_src) = load_frame_fonts(
        &frame_sheet.font_faces,
        &child_base,
        sink,
        cookie_jar.clone(),
        child_csp_gate.as_ref(),
        child_self_origin.as_ref(),
        child_referrer_policy,
    );
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
    // Хэндлы уплощаются в общий список страницы: время жизни всех
    // под-документов привязано к странице целиком (замена/удаление
    // отдельного фрейма — будущий срез).
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
    if let Some(js) = parent_js {
        let accessible = frame_access_allowed(base, &child_url, opaque);
        // BUG-979: peer only when same-origin — see the symmetric comment on
        // the `register_parent_document` call site above.
        let child_peer = accessible.then(|| child_js.as_ref().and_then(|js| js.frame_peer_bridge())).flatten();
        js.register_iframe_document(
            info.node.index() as u32,
            Arc::clone(&child_doc_arc),
            &child_url,
            info.name.as_deref(),
            accessible,
            child_peer,
        );
    }
    fire_iframe_load_event(parent_js, info.node);
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

/// Всё, что [`run_frame_navigation`] нужно от хозяина фрейма, снятое ДО того,
/// как загрузка ребёнка уйдёт на фоновый поток (FRAME-4 срез 3).
///
/// `spawn_frame` не трогает `Lumen::frames`, поэтому его вызов сам по себе
/// потокобезопасен (`Document` — `Send + Sync` по конструкции, см. assert в
/// `lumen_dom::lib.rs`; `PersistentJs: Send + Sync`) — единственное, что
/// нельзя нести через границу потока, это ЖИВОЙ индекс/заём `Lumen::frames`,
/// который на момент завершения загрузки может уже не существовать
/// (навигация предка, полная перезагрузка страницы). [`Self::old_doc`] — это
/// и есть якорь идентичности вместо индекса: [`apply_frame_navigation`]
/// проверяет его присутствие в `frames`, а не читает `idx`.
pub(crate) struct FrameNavPrep {
    pub(crate) host: NodeId,
    depth: usize,
    pub(crate) host_doc: Arc<Mutex<Document>>,
    host_base: ResourceBase,
    parent_js: Option<Arc<dyn PersistentJs>>,
    info: lumen_dom::IframeInfo,
    /// Документ, который эта навигация заменяет — якорь идентичности вместо
    /// индекса (см. doc на [`Self`]).
    pub(crate) old_doc: Arc<Mutex<Document>>,
}

/// Один слот "чья очередь" для generation-guard навигации фрейма
/// (FRAME-4 срез 3): без него ответ МЕДЛЕННОГО запроса, пришедший ПОСЛЕ
/// БЫСТРОГО (пользователь кликнул по второй ссылке, не дождавшись первой),
/// откатил бы фрейм назад — контроля по одному лишь [`FrameNavPrep::old_doc`]
/// для этого недостаточно: оба запроса читают ОДИН и тот же старый документ,
/// раз ни один из них ещё не применился.
///
/// Ключ — пара (документ-хозяин, host-узел), а не `Arc`-адрес сам по себе:
/// адрес аллокации может быть переиспользован после `Drop` совершенно другим
/// документом (ABA), поэтому сравнение всегда идёт через `Arc::ptr_eq` по
/// живому значению, а не по хэшу указателя.
pub(crate) struct FrameNavRequest {
    host_doc: Arc<Mutex<Document>>,
    host: NodeId,
    generation: u64,
}

/// Снять описание хозяина и старый документ фрейма `idx` — синхронная,
/// быстрая (короткий лок дерева) часть навигации; сеть и парсинг остаются в
/// [`run_frame_navigation`], который вызывающая сторона уносит на фоновый
/// поток.
///
/// `host_src` нового хэндла остаётся прежним намеренно: это половина ключа, по
/// которому [`splice_one_frame`] узнаёт команду-заглушку в display list
/// родителя, а заглушку рисует layout родителя по атрибуту `src` элемента —
/// навигация фрейма атрибут не трогает (HTML LS §7.4.2 меняет документ, а не
/// разметку хозяина).
#[allow(clippy::unwrap_used)] // короткий лок дерева, docs/lint-policy.md §10
pub(crate) fn prepare_frame_navigation(
    frames: &[FrameHandle],
    idx: usize,
    page_doc: &Arc<Mutex<Document>>,
    env: &FrameLoadEnv,
    page_js: Option<&Arc<dyn PersistentJs>>,
) -> Option<FrameNavPrep> {
    let h = frames.get(idx)?;
    let (host, depth) = (h.host, h.depth);
    let parent_doc = h.parent_doc.clone();
    // Всё, что нужно от хозяина, вынимается СЕЙЧАС: к моменту завершения
    // фоновой загрузки `frames` мог перестроиться (другая навигация того же
    // или соседнего фрейма), а хозяин адресуется `Arc`, а не индексом.
    let (host_doc, host_base, parent_js) = match &parent_doc {
        None => (Arc::clone(page_doc), env.page_base.clone(), page_js.cloned()),
        Some(pd) => {
            let p = frames.iter().find(|o| Arc::ptr_eq(&o.doc, pd))?;
            (Arc::clone(pd), p.base.clone(), p.js.clone())
        }
    };
    // Описание host-элемента перечитывается из дерева хозяина: sandbox и `name`
    // принадлежат элементу, а не документу, и переживают навигацию.
    let info = {
        let d = host_doc.lock().unwrap();
        collect_iframes(&d).into_iter().find(|i| i.node == host)
    }?;
    let old_doc = Arc::clone(&h.doc);
    Some(FrameNavPrep { host, depth, host_doc, host_base, parent_js, info, old_doc })
}

/// Найти (или завести) generation-слот хозяина `(host_doc, host)` и увеличить
/// его — вызывается на UI-потоке СИНХРОННО, до того как загрузка уйдёт на
/// фоновый поток, чтобы каждый клик получил свой уникальный, монотонно
/// растущий номер. [`apply_frame_navigation`]'s caller сверяет его при
/// получении ответа: несовпадение значит «этот хозяин навигировал ещё раз,
/// пока мы ждали сеть» — ответ отбрасывается, что бы он ни принёс.
pub(crate) fn bump_frame_nav_generation(
    requests: &mut Vec<FrameNavRequest>,
    host_doc: &Arc<Mutex<Document>>,
    host: NodeId,
) -> u64 {
    if let Some(r) = requests.iter_mut().find(|r| r.host == host && Arc::ptr_eq(&r.host_doc, host_doc)) {
        r.generation += 1;
        r.generation
    } else {
        requests.push(FrameNavRequest { host_doc: Arc::clone(host_doc), host, generation: 1 });
        1
    }
}

/// Действительно ли `generation` — ещё текущий номер слота `(host_doc, host)`
/// (FRAME-4 срез 3). Слот НЕ удаляется здесь — следующая навигация того же
/// хозяина продолжит расти от текущего числа, а не начнёт с 1 и не столкнётся
/// со значением какого-то более раннего в полёте ответа.
pub(crate) fn frame_nav_generation_current(
    requests: &[FrameNavRequest],
    host_doc: &Arc<Mutex<Document>>,
    host: NodeId,
    generation: u64,
) -> bool {
    requests
        .iter()
        .any(|r| r.host == host && Arc::ptr_eq(&r.host_doc, host_doc) && r.generation == generation)
}

/// Забыть про фреймы страницы, которой больше нет (полная перезагрузка) —
/// `host_doc`, по которому раньше сверялись слоты, вот-вот перестанет
/// существовать вместе со старым `Lumen::frames`.
pub(crate) fn clear_frame_nav_requests(requests: &mut Vec<FrameNavRequest>) {
    requests.clear();
}

/// Навигация под-документа фрейма по адресу `href` (BUG-480 срез 19) — сеть,
/// парсинг, скрипты и layout ребёнка (FRAME-4 срез 3: вызывается на фоновом
/// потоке, результат применяет [`apply_frame_navigation`] на UI-потоке).
///
/// `href` — сырое значение ссылки, `nav_base` — база документа, В КОТОРОМ по
/// ней кликнули ([`FrameHandle::base`] этого документа). Это не всегда база
/// целевого фрейма: `target=_parent` меняет чужой под-документ, а адрес всё
/// равно написан кликнувшим.
///
/// `uir_override` — GAP-CSPENF срез 55, прямиком в [`spawn_frame`]'s
/// одноимённый параметр: см. его doc-comment.
pub(crate) fn run_frame_navigation(
    prep: &FrameNavPrep,
    href: &str,
    nav_base: &ResourceBase,
    page_doc: &Arc<Mutex<Document>>,
    env: &FrameLoadEnv,
    uir_override: Option<bool>,
) -> Vec<FrameHandle> {
    spawn_frame(
        &prep.info,
        Some((href, nav_base)),
        &prep.host_doc,
        prep.depth,
        &prep.host_base,
        page_doc,
        env,
        prep.parent_js.as_ref(),
        uir_override,
    )
}

/// Вклеить результат [`run_frame_navigation`] в `frames` на UI-потоке.
///
/// Старый хэндл заменяется новым, а не правится на месте: под-документ — это
/// другой `Document`, другой JS-контекст и другой каскад, то есть от прежнего
/// не остаётся ничего, кроме места на странице. Вместе с ним уходят и хэндлы
/// его ВЛОЖЕННЫХ фреймов — их документы-хозяева только что перестали
/// существовать, и оставить их значило бы держать живые рантаймы, до которых
/// уже никто не доберётся.
///
/// Возвращает `false` (ничего не меняя), если `old_doc` уже отсутствует в
/// `frames` — предок навигировал сам, или страница успела перезагрузиться
/// целиком, пока этот ответ летел с фонового потока; вызывающая сторона в
/// этом случае просто роняет `handles` вместе с их JS-рантаймами. Пустой
/// `handles` тоже трактуется как отказ — `spawn_frame` пуст лишь в случае,
/// которого сегодняшняя логика (FRAME-4 срез 2, документ ошибки) не создаёт,
/// но контракт остаётся тем же, что был у синхронной `navigate_frame`.
pub(crate) fn apply_frame_navigation(
    frames: &mut Vec<FrameHandle>,
    old_doc: &Arc<Mutex<Document>>,
    handles: Vec<FrameHandle>,
) -> bool {
    if handles.is_empty() || !frames.iter().any(|o| Arc::ptr_eq(&o.doc, old_doc)) {
        return false;
    }
    // FRAME-4 срез 2: `handles`'s own handle is always LAST (`spawn_frame`
    // pushes it after extending with its nested children) — the frame-level
    // outcome, distinct from the source-fetch-level reason already printed
    // inside `fetch_iframe_source`. Navigation still "succeeds" here (a
    // document replaced the old one), but the visible result is an error
    // page, worth a diagnostic of its own for anyone correlating logs with
    // the screen.
    if handles.last().is_some_and(|h| h.load_failed) {
        eprintln!("iframe: навигация фрейма показывает документ ошибки");
    }
    drop_frame_subtree(frames, old_doc);
    frames.retain(|o| !Arc::ptr_eq(&o.doc, old_doc));
    frames.extend(handles);
    true
}

/// Выбросить хэндлы всех фреймов, чей host-элемент лежал в `doc` — прямо или
/// через цепочку вложенности (BUG-480 срез 19).
///
/// Цикл до неподвижной точки, а не один проход: список плоский, и внук
/// удаляемого фрейма ссылается на документ своего родителя, который сам
/// удаляется на этом же шаге.
pub(crate) fn drop_frame_subtree(frames: &mut Vec<FrameHandle>, doc: &Arc<Mutex<Document>>) {
    let mut doomed: Vec<Arc<Mutex<Document>>> = vec![Arc::clone(doc)];
    let mut i = 0;
    while i < doomed.len() {
        let cur = Arc::clone(&doomed[i]);
        for h in frames.iter() {
            if h.parent_doc.as_ref().is_some_and(|pd| Arc::ptr_eq(pd, &cur))
                && !doomed.iter().any(|d| Arc::ptr_eq(d, &h.doc))
            {
                doomed.push(Arc::clone(&h.doc));
            }
        }
        i += 1;
    }
    frames.retain(|h| {
        !h.parent_doc
            .as_ref()
            .is_some_and(|pd| doomed.iter().any(|d| Arc::ptr_eq(d, pd)))
    });
}

/// Живой sub-документ одного `<iframe>` (BUG-480, срез 1).
///
/// Держит порождённый `Document` и его JS-контекст живыми на время жизни
/// страницы: пока хэндл жив, тикают таймеры ребёнка и работают его
/// обработчики. Падает вместе со страницей — замена страницы в
/// [`Lumen::apply_loaded_page`] уносит все фреймы разом, отдельного
/// lifecycle-менеджмента не нужно.
///
/// Срез 2 дал JS родителя фасады под-документа через реестр биндингов
/// `frame_bridge.rs` — регистрация идёт из локальных переменных этой функции,
/// поэтому поля хэндла по-прежнему не читаются; читаться начнут со срезом
/// навигации/замены фрейма.
pub(crate) struct FrameHandle {
    /// `NodeId` `<iframe>`-элемента в документе-родителе.
    pub(crate) host: NodeId,
    /// Адрес под-документа: разрешённый URL, путь файла или `about:blank` /
    /// `about:srcdoc`. Диагностика и будущая навигация фрейма.
    pub(crate) url: String,
    /// `true` — [`Self::doc`] не то, что просили: `src`/навигация не
    /// получили ответ (сеть, битый файл, неподдержанная схема), и вместо
    /// содержимого показан синтетический документ ошибки (FRAME-4 срез 2,
    /// [`frame_error_document`]). `Self::url` в этом случае — адрес, который
    /// не открылся, а не адрес того, что реально показано.
    pub(crate) load_failed: bool,
    /// База, относительно которой под-документ разрешает СВОИ адреса
    /// (BUG-480 срез 19).
    ///
    /// Хранится, а не выводится из [`Self::url`]: `about:blank`/`about:srcdoc`
    /// наследуют базу хозяина, и восстановить её из строки адреса нечем. Читает
    /// её навигация фрейма — ссылку резолвит тот документ, в котором по ней
    /// кликнули, а не документ-хозяин.
    pub(crate) base: ResourceBase,
    /// Под-документ. Отдельный `Arc` — JS-замыкания ребёнка держат его же.
    pub(crate) doc: Arc<Mutex<Document>>,
    /// JS-контекст ребёнка (`None` — у фрейма не было скриптов или v8 выключен).
    pub(crate) js: Option<Arc<dyn PersistentJs>>,
    /// Глубина вложенности: 0 — фрейм страницы, 1 — фрейм внутри фрейма.
    ///
    /// Задаёт ПОРЯДОК обоих проходов [`sync_frame_viewports`]: host-бокс фрейма
    /// глубины `d` ищется в layout фрейма глубины `d-1` (`NodeId` уникален лишь
    /// внутри своего документа), поэтому layout считается по возрастанию
    /// глубины, а display list — по убыванию.
    pub(crate) depth: usize,
    /// Разобранный каскад под-документа (BUG-480 срез 12 собирает его текст,
    /// срез 13 пересчитывает по нему layout при каждой смене размера хоста).
    pub(crate) sheet: lumen_css_parser::Stylesheet,
    /// Вьюпорт последнего посчитанного layout ребёнка: сначала
    /// [`FRAME_UA_DEFAULT_SIZE`], затем контентный бокс хоста. Служит гейтом
    /// «размер не менялся — не пересчитывать» в [`sync_frame_viewports`].
    pub(crate) viewport: lumen_core::geom::Size,
    /// Документ, в дереве которого лежит host-элемент: `None` — страница,
    /// `Some` — под-документ фрейма-родителя (BUG-480 срез 14).
    ///
    /// Родитель адресуется именно `Arc`-ом, а не индексом в списке: список
    /// плоский, вложенные хэндлы попадают в него раньше своего родителя, а
    /// `NodeId` хоста уникален лишь внутри своего документа — сравнение
    /// `Arc::ptr_eq` единственное, что здесь ничего не путает.
    pub(crate) parent_doc: Option<Arc<Mutex<Document>>>,
    /// Layout под-документа на текущем [`Self::viewport`] (BUG-480 срез 14).
    ///
    /// Хранится по двум причинам: по нему рисуется [`Self::content_dl`], и в
    /// нём ищется host-бокс ВЛОЖЕННОГО фрейма — в layout страницы его нет.
    pub(crate) layout: Option<lumen_layout::LayoutBox>,
    /// Display list под-документа в его собственных координатах, с уже
    /// вклеенным содержимым его вложенных фреймов (BUG-480 срез 14).
    ///
    /// Пуст, пока layout не посчитан: тогда на экране остаётся серая заглушка.
    pub(crate) content_dl: DisplayList,
    /// Интерактивное состояние ПОСЛЕДНЕГО посчитанного прохода ребёнка
    /// (BUG-480 срез 23) — вторая половина гейта «ничего не изменилось — не
    /// пересчитывать» в [`sync_frame_viewports`], рядом с [`Self::viewport`].
    ///
    /// Хранится здесь, а не выводится вызывающим: так любой, кто передаст
    /// новое [`FrameInteractive`], автоматически получит пересчёт ровно тех
    /// фреймов, чьё состояние сдвинулось, и не может забыть назвать их сам.
    pub(crate) interactive: FrameNodeState,
    /// Контентный бокс host-элемента в координатах ЕГО документа — куда
    /// вклеивается [`Self::content_dl`] (BUG-480 срез 14).
    pub(crate) host_rect: Option<Rect>,
    /// Значение атрибута `src` host-элемента — половина ключа, по которому
    /// [`splice_one_frame`] узнаёт команду-заглушку в display list родителя.
    pub(crate) host_src: String,
    /// Декодированные картинки под-документа (BUG-480 срез 15) — `<img>` плюс,
    /// с FRAME-5, `background-image`/`cross-fade()` ([`fetch_frame_background_images`]).
    ///
    /// Едут в `LoadedPage::images` страницы: регистрация в рендерере (и в
    /// CPU-кэше снимков) идёт единым списком, поэтому ни одной новой точки
    /// регистрации срез не заводит — все существующие подхватывают их сами.
    pub(crate) images: Vec<(String, Arc<lumen_image::Image>)>,
    /// `(сырой src, ключ регистрации)` картинок под-документа — карта для
    /// [`rekey_frame_images`] (BUG-480 срез 15, FRAME-5 добавил в неё
    /// background-картинки).
    pub(crate) image_keys: Vec<(String, String)>,
    /// Прокрутка под-документа по вертикали, CSS px (BUG-480 срез 17).
    ///
    /// Читают четыре разных места, и все обязаны читать ОДНО поле, иначе
    /// пиксели, hit-тест и `window.scrollY` ребёнка разойдутся:
    /// [`splice_one_frame`] сдвигает содержимое, [`pointer_target`] — точку
    /// спуска, [`frame_page_origin`] — координаты оверлеев поверх фрейма, а
    /// шелл — позицию в JS-контексте ребёнка.
    pub(crate) scroll_y: f32,
    /// Прокрутка под-документа по горизонтали, CSS px (FRAME-3 срез 1).
    ///
    /// Сестра [`Self::scroll_y`] и её же три ПЕРВЫХ читателя (сплайс,
    /// hit-тест, [`frame_page_origin`]) — тот же инвариант «одно поле».
    /// Четвёртого читателя, JS-контекста, у неё НЕТ: `window.scrollX`
    /// ребёнка, как и у страницы (`scrolling.rs`), остаётся захардкожен в
    /// 0 — колесо вбок двигает содержимое визуально, но `scroll`/`scrollend`
    /// по этой оси ребёнку не шлётся, симметрично тому, что `scroll_x_by`
    /// самой странице тоже их не шлёт.
    pub(crate) scroll_x: f32,
    /// Overflow-контейнеры (`overflow: scroll|auto`) СОБСТВЕННОГО дерева
    /// под-документа (FRAME-3 срез 3) — зеркало [`Lumen::scroll_containers`]
    /// на уровне фрейма, а не отдельная per-node карта: у фрейма и так один
    /// хэндл на под-документ, а `NodeId` внутри него не пересекается с
    /// `NodeId`-ами родителя, так что дополнительный ключ избыточен.
    ///
    /// Пересобирается КАЖДЫЙ раз, когда пересчитан [`Self::layout`] (обе точки
    /// присвоения этого поля обязаны обновлять и это тоже, иначе хит-тест
    /// колеса читал бы геометрию от предыдущего прохода) — `collect_scroll_containers`
    /// того же движка, что уже строит `Lumen::scroll_containers` для страницы.
    pub(crate) scroll_containers: Vec<lumen_layout::ScrollContainer>,
    /// FRAME-5: реестр `local()` `@font-face`-семей ребёнка ([`load_frame_fonts`]) —
    /// повторно читается на каждом пересчёте [`frame_measurer`] в
    /// [`sync_frame_viewports`]/[`relayout_frame_content`], без сети.
    pub(crate) font_registry: lumen_font::FontRegistry,
    /// FRAME-5: `url()` `@font-face`-семьи ребёнка, уже скачанные и
    /// декодированные [`load_frame_fonts`] в `spawn_frame` — сестра
    /// [`Self::font_registry`], тот же повторный, но безсетевой читатель.
    pub(crate) web_fonts: Vec<LoadedWebFont>,
    /// FRAME-5: многокадровые GIF-анимации под-документа — форма
    /// [`FrameSubresourceOutcomes::animated_gifs`]. Едут в `Lumen::animated_gifs`
    /// (карту СТРАНИЦЫ) вместе со [`Self::images`], тем же слиянием в
    /// `page_pipeline.rs` — тиканье в `RedrawRequested` не заводит отдельного
    /// пути для фреймов, потому что ключи уже уникальны на всю страницу
    /// ([`frame_image_key`]).
    pub(crate) animated_gifs: Vec<(String, lumen_image::AnimatedGif)>,
    /// FRAME-5 срез 2: `<img loading="lazy">` requests of this frame's own
    /// document ([`FrameSubresourceOutcomes::lazy_requests`], fixed at
    /// creation — the child's markup does not change). Re-registered with the
    /// child's `IntersectionObserver` shim on every real layout pass
    /// ([`harvest_frame_lazy_requests`]).
    pub(crate) lazy_requests: Vec<lumen_layout::ImageRequest>,
    /// `(node id, url)` pairs the child's own `IntersectionObserver` has
    /// fired since [`crate::frame_lazy::fetch_frame_lazy_images`] last
    /// drained it — harvested by [`harvest_frame_lazy_requests`] inside
    /// [`sync_frame_viewports`]/[`relayout_frame_content`].
    pub(crate) pending_lazy: Vec<(u32, String)>,
}

// ── скролл под-документа (BUG-480 срез 17) ──────────────────────────────────

/// Предел прокрутки под-документа: насколько его содержимое выше вьюпорта.
///
/// Высота берётся из ГОТОВОГО display list ребёнка — тем же правилом, что и у
/// страницы ([`content_height_of`]), а не из layout-дерева: у страницы
/// «прокручивается ровно то, что нарисовано» (пустой распорка без фона не даёт
/// прокрутки — известная ловушка, см. CLAUDE.md), и разойтись этим двум
/// ответам внутри одного движка нельзя.
pub(crate) fn frame_max_scroll(h: &FrameHandle) -> f32 {
    if h.content_dl.is_empty() {
        return 0.0;
    }
    (crate::display_list_metrics::content_height_of(&h.content_dl) - h.viewport.height).max(0.0)
}

/// Предел горизонтальной прокрутки под-документа (FRAME-3 срез 1) — то же
/// правило, что [`frame_max_scroll`], по ширине.
pub(crate) fn frame_max_scroll_x(h: &FrameHandle) -> f32 {
    if h.content_dl.is_empty() {
        return 0.0;
    }
    (crate::display_list_metrics::content_width_of(&h.content_dl) - h.viewport.width).max(0.0)
}

/// Прокрутить под-документ фрейма `idx` в АБСОЛЮТНУЮ позицию `y` (с зажимом).
///
/// Возвращает новую позицию, если она действительно изменилась, и `None`
/// иначе — вызывающая сторона по этому ответу решает две разные вещи: слать ли
/// ребёнку `scroll`/`scrollend` (CSSOM-View §14 — событие принадлежит движению,
/// а не колесу) и продолжать ли цепочку прокрутки выше по CSS Overscroll
/// Behavior L1 §3, как это уже делают overflow-контейнеры страницы.
pub(crate) fn scroll_frame_to(frames: &mut [FrameHandle], idx: usize, y: f32) -> Option<f32> {
    let max = frame_max_scroll(&frames[idx]);
    let clamped = y.clamp(0.0, max);
    if (clamped - frames[idx].scroll_y).abs() <= f32::EPSILON {
        return None;
    }
    frames[idx].scroll_y = clamped;
    Some(clamped)
}

/// Прокрутить под-документ фрейма `idx` в АБСОЛЮТНУЮ горизонтальную позицию
/// `x` (с зажимом) — FRAME-3 срез 1, зеркало [`scroll_frame_to`].
pub(crate) fn scroll_frame_to_x(frames: &mut [FrameHandle], idx: usize, x: f32) -> Option<f32> {
    let max = frame_max_scroll_x(&frames[idx]);
    let clamped = x.clamp(0.0, max);
    if (clamped - frames[idx].scroll_x).abs() <= f32::EPSILON {
        return None;
    }
    frames[idx].scroll_x = clamped;
    Some(clamped)
}

/// Максимальная глубина вложенности фреймов: страница (0) → iframe (1) →
/// iframe в iframe (2) → глубже не загружаем. Защита от рекурсивных
/// самовложений в недоверенном HTML; спека глубину не ограничивает.
pub(crate) const MAX_FRAME_DEPTH: usize = 2;

/// UA-дефолт intrinsic-размера `<iframe>` (HTML LS §4.8.5): 300×150 CSS px —
/// см. `iframe_ua_default_size_300_by_150` в `lumen-layout`. BUG-480 срез 12
/// использует его как вьюпорт для ПЕРВОГО layout ребёнка: реальный размер
/// host-бокса ещё не известен в момент вызова (`load_frame_sub_documents` идёт
/// ДО layout страницы-родителя), а собственные скрипты ребёнка и его
/// DOMContentLoaded/load исполняются уже здесь и обязаны видеть какую-то
/// geometry. Срез 13 уточняет её до контентного бокса хоста сразу после layout
/// родителя ([`sync_frame_viewports`]).
const FRAME_UA_DEFAULT_SIZE: lumen_core::geom::Size = lumen_core::geom::Size::new(300.0, 150.0);
