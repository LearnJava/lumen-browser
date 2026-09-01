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
enum FrameSource {
    /// Готовый HTML (атрибут `srcdoc` / пустой `about:blank`).
    Inline(String),
    /// Прочитанный файл.
    File { html: String, path: std::path::PathBuf },
    /// Тело ответа по сети.
    Url { html: String, url: String },
}

/// Получить исходник под-документа для `src`-фрейма: разрешить относительно
/// `base`, файл прочитать с диска, URL скачать через subresource-клиент с
/// `RequestDestination::Document` (тот же mixed-content/SW-интерсептор, что у
/// остальных подресурсов). `None` — источник получить нельзя (лог в stderr).
fn fetch_iframe_source(
    src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Option<FrameSource> {
    if src.trim().is_empty() {
        return Some(FrameSource::Inline(String::new()));
    }
    let lowered = src.trim_start().to_ascii_lowercase();
    if lowered.starts_with("javascript:") {
        eprintln!("iframe: javascript:-URL не поддерживаются (BUG-480 срез 1), пропуск '{src}'");
        return None;
    }
    if lowered.starts_with("data:") {
        eprintln!("iframe: data:-URL не поддерживаются (BUG-480 срез 1), пропуск '{src}'");
        return None;
    }
    match base.resolve(src) {
        ResolvedResource::File(path) => {
            let html = std::fs::read_to_string(&path)
                .map_err(|e| eprintln!("iframe: файл {} не читается: {e}", path.display()))
                .ok()?;
            Some(FrameSource::File { html, path })
        }
        ResolvedResource::Url(url) => {
            use lumen_core::url::Url as _Url;
            use lumen_network::RequestDestination;
            let sub_url = _Url::parse(&url)
                .map_err(|e| eprintln!("iframe: битый URL '{url}': {e}"))
                .ok()?;
            let client = base.http_client_for_subresource(Arc::clone(sink), cookie_jar);
            let bytes = client
                .fetch_subresource(&sub_url, RequestDestination::Document)
                .map_err(|e| eprintln!("iframe: загрузка '{url}' не удалась: {e}"))
                .ok()?;
            Some(FrameSource::Url {
                html: String::from_utf8_lossy(&bytes).into_owned(),
                url,
            })
        }
    }
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
        ResourceBase::File(p) => format!("file://{}", p.display()),
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
/// экран (срез 14). `loading="lazy"` не запрашивается вовсе: прокси вьюпорта у
/// фреймов нет, так же как срез 1 пропускает сами `loading=lazy`-iframe.
pub(crate) fn fetch_frame_subresources(
    doc: &mut Document,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    media_ctx: &lumen_css_parser::MediaContext,
    viewport: lumen_core::geom::Size,
    target: lumen_core::ColorSpace,
) -> FrameSubresourceOutcomes {
    let inline = extract_style_blocks(doc);
    let mut css = inline_css_imports(
        &inline,
        base,
        sink,
        cookie_jar.clone(),
        media_ctx,
        &mut std::collections::HashSet::new(),
        0,
    );
    let (linked, links) = load_linked_stylesheets(doc, base, sink, cookie_jar.clone(), media_ctx);
    css.push_str(&linked);

    let requests: Vec<lumen_layout::ImageRequest> =
        lumen_layout::collect_image_requests(doc, viewport)
            .into_iter()
            .filter(|req| !req.is_lazy)
            .collect();
    // Фаза 1 (параллельно): сеть + декодирование, `doc` не трогаем — форма
    // `fetch_and_decode_images` страницы.
    let decoded = parallel_map(&requests, |_, req| {
        let sink: &Arc<dyn EventSink> = &sink.clone();
        let key = frame_image_key(base, &req.url);
        let img = crate::image_cache::IMAGE_CACHE.get_or_decode_current(&key, || {
            decode_image(&req.url, base, sink, cookie_jar.clone(), target)
        });
        (key, img)
    });
    // Фаза 2 (последовательно): intrinsic-размеры в дерево ребёнка и сборка
    // выходных векторов в порядке DOM.
    let mut images = Vec::with_capacity(requests.len());
    let mut decoded_images = Vec::new();
    let mut image_keys = Vec::with_capacity(requests.len());
    for (req, (key, img)) in requests.iter().zip(decoded) {
        image_keys.push((req.url.clone(), key.clone()));
        // BUG-269, как у страницы: intrinsic нужен, если автор не задал ХОТЯ БЫ
        // одно измерение — второе достраивается по соотношению сторон.
        let wants_intrinsic = !(req.has_explicit_width && req.has_explicit_height);
        let first = match &img {
            None => None,
            Some(crate::image_cache::DecodedImage::Static(i)) => Some(Arc::clone(i)),
            // Многокадровый GIF: во фрейм идёт первый кадр. Тиканья анимации у
            // под-документов нет (`Lumen::animated_gifs` — карта страницы),
            // поэтому сама анимация наружу не отдаётся.
            Some(crate::image_cache::DecodedImage::Animated { first, .. }) => Some(Arc::clone(first)),
        };
        images.push((req.node_id, first.is_some()));
        if let Some(image) = first {
            if wants_intrinsic {
                lumen_layout::apply_intrinsic_size(doc, req.node_id, image.width, image.height);
            }
            decoded_images.push((key, image));
        }
    }

    FrameSubresourceOutcomes { links, images, css, decoded_images, image_keys }
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
fn frame_image_key(base: &ResourceBase, raw_src: &str) -> String {
    base.resolve_str(raw_src)
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
/// face-ы, как у страницы ([`page_measurer`]), но без `@font-face`-шрифтов
/// ребёнка — собственного прохода `url()`-загрузки у фрейма пока нет.
///
/// `None` — шрифт не разобрался; вызывающая сторона тогда просто не считает
/// geometry (лог в stderr), а не валит загрузку страницы.
fn frame_measurer() -> Option<lumen_paint::MultiFontMeasurer> {
    match lumen_font::Font::parse(INTER_FONT) {
        Ok(font) => Some(page_measurer(&font, &[])),
        Err(e) => {
            eprintln!("iframe: сбой измерителя шрифта, geometry ребёнка не посчитана: {e}");
            None
        }
    }
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
    let (frame_layout, rects, styles) = {
        let d = doc.lock().unwrap();
        lumen_layout::set_interactive_state(state.hovered, state.focused, state.active);
        let frame_layout = lumen_layout::layout_measured(&d, sheet, viewport, measurer);
        lumen_layout::clear_interactive_state();
        let rects = lumen_layout::collect_layout_rects(&frame_layout);
        let styles = lumen_layout::collect_computed_styles(&frame_layout);
        (frame_layout, rects, styles)
    };
    if let Some(js) = js {
        js.update_layout_rects(rects);
        js.update_hit_test_tree(Arc::new(frame_layout.clone()));
        js.update_computed_styles(styles);
        js.update_viewport_size(viewport.width, viewport.height);
    }
    frame_layout
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
    let mut measurer: Option<lumen_paint::MultiFontMeasurer> = None;
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
            if measurer.is_none() {
                measurer = frame_measurer();
            }
            let Some(m) = measurer.as_ref() else { return };
            let layout = layout_frame_document(
                &frames[i].doc,
                &frames[i].sheet,
                size,
                frames[i].js.as_ref(),
                m,
                state,
            );
            frames[i].scroll_containers = lumen_layout::collect_scroll_containers(&layout);
            frames[i].layout = Some(layout);
            frames[i].viewport = size;
            frames[i].interactive = state;
            relaid[i] = true;
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
pub(crate) fn relayout_frame_content(
    frames: &mut [FrameHandle],
    idx: usize,
    page_layout: &lumen_layout::LayoutBox,
    interactive: FrameInteractive,
) {
    let Some(measurer) = frame_measurer() else { return };
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
pub(crate) fn rekey_frame_images(dl: &mut DisplayList, frames: &[FrameHandle], idx: usize) {
    if frames[idx].image_keys.is_empty() {
        return;
    }
    for cmd in dl.iter_mut() {
        let DisplayCommand::DrawImage { src, .. } = cmd else { continue };
        if frames.iter().any(|h| {
            h.parent_doc
                .as_ref()
                .is_some_and(|pd| Arc::ptr_eq(pd, &frames[idx].doc))
                && &h.host_src == src
        }) {
            continue;
        }
        if let Some((_, key)) = frames[idx].image_keys.iter().find(|(raw, _)| raw == src) {
            *src = key.clone();
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
        handles.extend(spawn_frame(&info, None, parent, depth, base, top_doc, env, parent_js));
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
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
#[allow(clippy::unwrap_used)] // короткий лок дерева; poisoned mutex = паника потока загрузки, docs/lint-policy.md §10
fn spawn_frame(
    info: &lumen_dom::IframeInfo,
    dest: Option<(&str, &ResourceBase)>,
    parent: &Arc<Mutex<Document>>,
    depth: usize,
    base: &ResourceBase,
    top_doc: &Arc<Mutex<Document>>,
    env: &FrameLoadEnv,
    parent_js: Option<&Arc<dyn PersistentJs>>,
) -> Vec<FrameHandle> {
    // URL родителя и верха для фасадов location/URL у предков (срез 3).
    let parent_url = base_url_string(base);
    let top_url = base_url_string(&env.page_base);
    let sink = &env.sink;
    let cookie_jar = env.cookie_jar.clone();
    let mut handles = Vec::new();
    // Источник HTML + база ребёнка для его относительных URL.
    let fetched = match dest {
        Some((href, nav_base)) => Some(fetch_iframe_source(href, nav_base, sink, cookie_jar.clone())),
        None if info.srcdoc.is_some() => None,
        None => info
            .src
            .as_deref()
            .map(|src| fetch_iframe_source(src, base, sink, cookie_jar.clone())),
    };
    let (html, child_base, child_url): (String, ResourceBase, String) = match fetched {
        Some(Some(FrameSource::Inline(html))) => (html, base.clone(), "about:blank".to_owned()),
        Some(Some(FrameSource::File { html, path })) => {
            let url = format!("file://{}", path.display());
            (html, ResourceBase::File(path), url)
        }
        Some(Some(FrameSource::Url { html, url })) => (html, ResourceBase::Url(url.clone()), url),
        // Источник получить нельзя — лог уже напечатан внутри.
        Some(None) => return handles,
        None => match &info.srcdoc {
            Some(srcdoc) => (srcdoc.clone(), base.clone(), "about:srcdoc".to_owned()),
            // Ни src, ни srcdoc — спека грузит about:blank немедленно.
            None => (String::new(), base.clone(), "about:blank".to_owned()),
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
        )
    };
    // Скрипты ребёнка собираются и (внешние) скачиваются ДО передачи
    // документа в рантайм: run_scripts_with_dom принимает doc по значению.
    let (classic_scripts, module_scripts) = {
        let mut classic_items = Vec::new();
        let mut module_items = Vec::new();
        collect_scripts_ordered(&child_doc, child_doc.root(), &mut classic_items, &mut module_items);
        (
            resolve_script_sources(&classic_items, &child_base, sink, cookie_jar.clone()),
            resolve_script_sources(&module_items, &child_base, sink, cookie_jar.clone()),
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
        env.cookie_banner_dismiss,
        env.deterministic,
        env.cross_origin_isolated,
        &[],
        classic_scripts,
        module_scripts,
        // BUG-480 срез 8: фрейму рантайм нужен даже без единого парсерного
        // скрипта — иначе ему нечем принимать кросс-фреймовые postMessage/
        // события/RunScript (срезы 4–8), а статические iframe — самый
        // частый встраиваемый случай. Странице (второй вызов) хватает
        // старого поведения: без скриптов ей нечем отвечать.
        true,
        // BUG-443: a sub-document is laid out only after this call returns
        // (`layout_frame_document`), so there is no parse-time layout to offer.
        None,
    );
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
        js.register_parent_document(
            info.node.index() as u32,
            Arc::clone(parent),
            &parent_url,
            accessible_parent,
        );
        // Ребёнок глубины ≥ 2 получает отдельный слот top: его верх —
        // корень страницы, а не непосредственный родитель.
        if depth >= 1 {
            let accessible_top = frame_access_allowed(&env.page_base, &child_url, opaque);
            js.register_top_document(Arc::clone(top_doc), &top_url, accessible_top);
        }
    }
    // BUG-480 срез 12: cascade + layout ребёнка — контентная геометрия
    // внутри фрейма (getBoundingClientRect/offsetWidth/offsetHeight)
    // вместо честных нулей (см. frame_bridge.rs: «layout содержимого
    // фрейма — отдельный срез»). Вьюпорт — [`FRAME_UA_DEFAULT_SIZE`]
    // (реальный размер host-бокса ещё не известен на этом шаге).
    // Измеритель собран как у страницы ([`page_measurer`]), но без
    // @font-face ребёнка (`web_fonts: &[]` — задел следующего среза).
    // Каскад ребёнка разбирается один раз и переезжает в хэндл: срез 13
    // пересчитывает layout под реальный host-бокс, и повторный разбор
    // того же текста на каждом relayout был бы чистой тратой. Сам layout
    // тоже едет в хэндл (срез 14): по нему рисуется содержимое фрейма и в
    // нём ищется host-бокс вложенного фрейма.
    let frame_sheet = lumen_css_parser::parse(&subresources.css);
    let frame_layout = frame_measurer().map(|measurer| {
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
    // Lifecycle ребёнка: DOMContentLoaded сразу после parse+inline-скриптов
    // (тот же порядок, что у top-level в parse_and_layout); window load —
    // следом, НО после исходов подресурсов (срез 11): «load» документа
    // следует за его подресурсами, и тест, где внутри window load читают
    // загруженный `<img>`/`link.onload`, работает.
    if let Some(js) = &child_js {
        js.notify_dom_content_loaded();
        deliver_frame_subresource_events(js, &subresources);
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
        js.register_iframe_document(
            info.node.index() as u32,
            Arc::clone(&child_doc_arc),
            &child_url,
            info.name.as_deref(),
            accessible,
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
        images: subresources.decoded_images,
        image_keys: subresources.image_keys,
        scroll_y: 0.0,
        scroll_x: 0.0,
        scroll_containers: frame_scroll_containers,
    });
    handles
}

/// Навигация под-документа фрейма `idx` по адресу `href` (BUG-480 срез 19).
///
/// `href` — сырое значение ссылки, `nav_base` — база документа, В КОТОРОМ по
/// ней кликнули ([`FrameHandle::base`] этого документа). Это не всегда база
/// целевого фрейма: `target=_parent` меняет чужой под-документ, а адрес всё
/// равно написан кликнувшим.
///
/// Старый хэндл заменяется новым, а не правится на месте: под-документ — это
/// другой `Document`, другой JS-контекст и другой каскад, то есть от прежнего
/// не остаётся ничего, кроме места на странице. Вместе с ним уходят и хэндлы
/// его ВЛОЖЕННЫХ фреймов — их документы-хозяева только что перестали
/// существовать, и оставить их значило бы держать живые рантаймы, до которых
/// уже никто не доберётся.
///
/// `host_src` нового хэндла остаётся прежним намеренно: это половина ключа, по
/// которому [`splice_one_frame`] узнаёт команду-заглушку в display list
/// родителя, а заглушку рисует layout родителя по атрибуту `src` элемента —
/// навигация фрейма атрибут не трогает (HTML LS §7.4.2 меняет документ, а не
/// разметку хозяина).
///
/// Возвращает `true`, если под-документ заменён; `false` — фрейма нет, хозяин
/// не найден или источник не получен (лог напечатан ниже по стеку).
#[allow(clippy::unwrap_used)] // короткий лок дерева, docs/lint-policy.md §10
#[allow(clippy::too_many_arguments)]
pub(crate) fn navigate_frame(
    frames: &mut Vec<FrameHandle>,
    idx: usize,
    href: &str,
    nav_base: &ResourceBase,
    page_doc: &Arc<Mutex<Document>>,
    env: &FrameLoadEnv,
    page_js: Option<&Arc<dyn PersistentJs>>,
) -> bool {
    let Some(h) = frames.get(idx) else { return false };
    let (host, depth) = (h.host, h.depth);
    let parent_doc = h.parent_doc.clone();
    // Всё, что нужно от хозяина, вынимается ДО удаления: и документ, и его
    // база, и его JS-контекст живут внутри того же `frames`, который сейчас
    // будет перестроен.
    let (host_doc, host_base, parent_js) = match &parent_doc {
        None => (Arc::clone(page_doc), env.page_base.clone(), page_js.cloned()),
        Some(pd) => {
            let Some(p) = frames.iter().find(|o| Arc::ptr_eq(&o.doc, pd)) else { return false };
            (Arc::clone(pd), p.base.clone(), p.js.clone())
        }
    };
    // Описание host-элемента перечитывается из дерева хозяина: sandbox и `name`
    // принадлежат элементу, а не документу, и переживают навигацию.
    let Some(info) = ({
        let d = host_doc.lock().unwrap();
        collect_iframes(&d).into_iter().find(|i| i.node == host)
    }) else {
        return false;
    };

    let old_doc = Arc::clone(&frames[idx].doc);
    let spawned = spawn_frame(
        &info,
        Some((href, nav_base)),
        &host_doc,
        depth,
        &host_base,
        page_doc,
        env,
        parent_js.as_ref(),
    );
    if spawned.is_empty() {
        return false;
    }
    drop_frame_subtree(frames, &old_doc);
    frames.retain(|o| o.host != host || !same_host_doc(&o.parent_doc, parent_doc.as_ref()));
    frames.extend(spawned);
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
    /// Декодированные картинки под-документа (BUG-480 срез 15).
    ///
    /// Едут в `LoadedPage::images` страницы: регистрация в рендерере (и в
    /// CPU-кэше снимков) идёт единым списком, поэтому ни одной новой точки
    /// регистрации срез не заводит — все существующие подхватывают их сами.
    pub(crate) images: Vec<(String, Arc<lumen_image::Image>)>,
    /// `(сырой src, ключ регистрации)` картинок под-документа — карта для
    /// [`rekey_frame_images`] (BUG-480 срез 15).
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
