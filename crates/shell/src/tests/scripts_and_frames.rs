//! Сбор скриптов документа, журнал парсерных вставок, песочница `<iframe>`,
//! доступ между фреймами, режим Tor и потоковая выдача картинок.

use super::*;

// ── BUG-164: external <script src> collection ─────────────────────────────

/// External `<script src>` is recorded as `External` in document order,
/// interleaved with inline classic scripts (HTML LS §8.1.3.1).
#[test]
fn collect_scripts_ordered_records_external_in_order() {
    let doc = lumen_html_parser::parse(
        r#"<html><body>
              <script>a=1;</script>
              <script src="/bundle.js"></script>
              <script>b=2;</script>
            </body></html>"#,
    );
    let mut classic = Vec::new();
    let mut modules = Vec::new();
    collect_scripts_ordered(&doc, doc.root(), &mut classic, &mut modules);
    assert!(modules.is_empty());
    assert_eq!(classic.len(), 3, "two inline + one external");
    assert!(matches!(&classic[0], ScriptSource::Inline(_, s) if s.contains("a=1")));
    assert!(matches!(&classic[1], ScriptSource::External(_, s) if s == "/bundle.js"));
    assert!(matches!(&classic[2], ScriptSource::Inline(_, s) if s.contains("b=2")));
}

/// BUG-804: скрипт, чей файл не пришёл, обязан остаться в списке — иначе
/// его элементу негде выстрелить `error`. Раньше `resolve_script_sources`
/// такой скрипт молча выбрасывал, и страница не узнавала об отказе ничего.
#[test]
fn resolve_script_sources_keeps_a_failed_external_for_its_error_event() {
    struct NullSink;
    impl EventSink for NullSink {
        fn emit(&self, _event: &Event) {}
    }
    let doc = lumen_html_parser::parse(
        r#"<html><body>
              <script src="b804-does-not-exist.js"></script>
              <script>ok=1;</script>
            </body></html>"#,
    );
    let mut classic = Vec::new();
    let mut modules = Vec::new();
    collect_scripts_ordered(&doc, doc.root(), &mut classic, &mut modules);
    let base = ResourceBase::File(PathBuf::from("samples/page.html"));
    let sink: Arc<dyn EventSink> = Arc::new(NullSink);
    let resolved = resolve_script_sources(&classic, &base, &sink, None, &doc);
    assert_eq!(resolved.len(), 2, "the failed script keeps its slot");
    assert_eq!(resolved[0].external_ok, Some(false));
    assert!(resolved[0].source.is_empty(), "no body to execute");
    assert_eq!(resolved[1].external_ok, None, "inline owes no load event");
    assert!(resolved[1].source.contains("ok=1"));
}

/// BUG-1124: `script-src 'nonce-…'` (с `'strict-dynamic'` и без) пропускает
/// внешний `<script nonce src>` с совпавшим nonce, а не судит его по URL —
/// раньше гейт видел только адрес, не находил подходящего источника и не
/// запрашивал скрипт вовсе. Скрипт без nonce по-прежнему заблокирован.
#[test]
fn resolve_script_sources_lets_a_nonced_external_script_through_csp() {
    struct NullSink;
    impl EventSink for NullSink {
        fn emit(&self, _event: &Event) {}
    }
    for policy in ["script-src 'nonce-abc'", "script-src 'nonce-abc' 'strict-dynamic'"] {
        let doc = lumen_html_parser::parse(&format!(
            r#"<html><head><meta http-equiv="Content-Security-Policy" content="{policy}"></head><body>
                  <script nonce="abc" src="b1124-missing.js"></script>
                  <script nonce="abd" src="b1124-missing.js"></script>
                  <script src="b1124-missing.js"></script>
                </body></html>"#
        ));
        let mut classic = Vec::new();
        let mut modules = Vec::new();
        collect_scripts_ordered(&doc, doc.root(), &mut classic, &mut modules);
        // URL-база: файловый путь не разбирается как URL, и гейт его не
        // судит вовсе. Порт 9 (discard) на loopback отказывает в соединении
        // сразу — пропущенному скрипту незачем ходить в настоящую сеть.
        let base = ResourceBase::Url("http://127.0.0.1:9/page.html".to_owned());
        let sink: Arc<dyn EventSink> = Arc::new(NullSink);
        let resolved = resolve_script_sources(&classic, &base, &sink, None, &doc);
        assert_eq!(resolved.len(), 3);
        assert!(resolved[0].csp_blocked.is_empty(), "{policy}: matching nonce is fetched");
        assert_eq!(resolved[1].csp_blocked, vec![policy.to_owned()], "{policy}: wrong nonce is blocked");
        assert_eq!(resolved[2].csp_blocked, vec![policy.to_owned()], "{policy}: no nonce is blocked");
    }
}

// ── BUG-827: порядок парсерных вставок для MutationObserver ───────────────

/// Собрать `ResolvedScript` из результата [`collect_scripts_ordered`] —
/// тела внешних скриптов тесту не нужны, важен только узел.
fn resolved_for_test(items: &[ScriptSource]) -> Vec<ResolvedScript> {
    items
        .iter()
        .map(|s| {
            let (node, source) = match s {
                ScriptSource::Inline(n, src) | ScriptSource::External(n, src) => (*n, src),
            };
            ResolvedScript { node, source: source.clone(), url: None, external_ok: None, csp_blocked: Vec::new() }
        })
        .collect()
}

fn count_nodes(doc: &Document, id: NodeId) -> usize {
    1 + doc.get(id).children.iter().map(|&c| count_nodes(doc, c)).sum::<usize>()
}

/// Журнал перечисляет каждый узел документа ровно один раз (кроме корня,
/// который ниоткуда не вставляется), в порядке дерева.
#[test]
fn parser_insert_log_lists_every_node_once() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><div><span></span></div><script>a=1;</script></body></html>"#,
    );
    let mut classic = Vec::new();
    let mut modules = Vec::new();
    collect_scripts_ordered(&doc, doc.root(), &mut classic, &mut modules);
    let scripts = resolved_for_test(&classic);
    let log = ParserInsertLog::build(&doc, &scripts);

    assert_eq!(log.pairs.len(), count_nodes(&doc, doc.root()) - 1);
    let mut seen: Vec<usize> = log.pairs.iter().map(|(_, c)| *c).collect();
    let before = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), before, "ни один узел не вставлен дважды");
}

/// Отрезок скрипта кончается на нём самом (вместе с его текстом), а то, что
/// стоит в документе ниже, попадает уже в следующий отрезок: настоящий
/// парсер вставил бы это после того, как скрипт отработал.
#[test]
fn parser_insert_log_cuts_segment_at_the_script() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><div></div><script>a=1;</script><p></p></body></html>"#,
    );
    let mut classic = Vec::new();
    let mut modules = Vec::new();
    collect_scripts_ordered(&doc, doc.root(), &mut classic, &mut modules);
    let scripts = resolved_for_test(&classic);
    assert_eq!(scripts.len(), 1);
    let log = ParserInsertLog::build(&doc, &scripts);

    let at_script = log.segment_end(Some(scripts[0].node));
    let all = log.segment_end(None);
    assert!(at_script < all, "ниже скрипта в документе ещё есть узлы");

    // Последняя пара отрезка — текст самого скрипта.
    let (parent, _) = log.pairs[at_script - 1];
    assert_eq!(parent, scripts[0].node.index());

    // Первая пара следующего отрезка — <p>.
    let (_, child) = log.pairs[at_script];
    let node = doc.get(NodeId::from_index(child));
    assert!(matches!(&node.data, NodeData::Element { name, .. } if name.local == "p"));
}

/// Без классических скриптов журнал пуст: наблюдателя ставить некому, а
/// модули исполняются, когда парсер уже всё вставил (HTML LS §8.1.3.1).
#[test]
fn parser_insert_log_is_empty_without_classic_scripts() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><div></div><script type="module">export const y = 2;</script></body></html>"#,
    );
    let log = ParserInsertLog::build(&doc, &[]);
    assert!(log.pairs.is_empty());
    assert_eq!(log.segment_end(None), 0);
}

/// BUG-1120: внешний классический `defer` без `async` уходит в отложенный
/// список вместе с модулями, в порядке документа (HTML LS §4.12.1.1 шаг 31).
/// `defer` на инлайновом скрипте игнорируется, `async` его отменяет.
#[test]
fn collect_scripts_ordered_puts_external_defer_into_deferred_list() {
    let doc = lumen_html_parser::parse(
        r#"<html><head>
              <script>head=1;</script>
              <script defer src="/d1.js"></script>
              <script type="module">mod=1;</script>
              <script defer>inlineDefer=1;</script>
              <script defer async src="/da.js"></script>
              <script src="/sync.js"></script>
              <script defer src="/d2.js"></script>
            </head><body><script>tail=1;</script></body></html>"#,
    );
    let mut classic = Vec::new();
    let mut deferred = Vec::new();
    collect_scripts_ordered(&doc, doc.root(), &mut classic, &mut deferred);
    let describe = |s: &ScriptSource| match s {
        ScriptSource::Inline(_, b) => b.trim().to_owned(),
        ScriptSource::External(_, u) => u.clone(),
    };
    let classic: Vec<String> = classic.iter().map(describe).collect();
    let deferred: Vec<String> = deferred.iter().map(describe).collect();
    assert_eq!(classic, ["head=1;", "inlineDefer=1;", "/da.js", "/sync.js", "tail=1;"]);
    assert_eq!(deferred, ["/d1.js", "mod=1;", "/d2.js"]);
}

/// `<script type=module src>` lands in the deferred list as `External`.
#[test]
fn collect_scripts_ordered_external_module() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><script type="module" src="/app.mjs"></script></body></html>"#,
    );
    let mut classic = Vec::new();
    let mut modules = Vec::new();
    collect_scripts_ordered(&doc, doc.root(), &mut classic, &mut modules);
    assert!(classic.is_empty());
    assert_eq!(modules.len(), 1);
    assert!(matches!(&modules[0], ScriptSource::External(_, s) if s == "/app.mjs"));
}

/// Non-JS script blocks (`application/ld+json`, `importmap`) are data, not
/// code — they must not be collected for execution, with or without `src`.
#[test]
fn collect_scripts_ordered_skips_non_js_types() {
    let doc = lumen_html_parser::parse(
        r#"<html><body>
              <script type="application/ld+json">{"@type":"Article"}</script>
              <script type="importmap">{"imports":{}}</script>
              <script type="application/json" src="/data.json"></script>
              <script>real=1;</script>
            </body></html>"#,
    );
    let mut classic = Vec::new();
    let mut modules = Vec::new();
    collect_scripts_ordered(&doc, doc.root(), &mut classic, &mut modules);
    assert!(modules.is_empty());
    assert_eq!(classic.len(), 1, "only the executable classic script");
    assert!(matches!(&classic[0], ScriptSource::Inline(_, s) if s.contains("real=1")));
}

/// `nomodule` — запасная сборка для движка без ES-модулей. Движок с
/// модулями обязан её пропустить, иначе сайт получает обе сборки разом
/// (живой пример — форма входа id.tbank.ru: legacy и современный бандл
/// монтировались в один корень и гасили друг друга).
#[test]
fn collect_scripts_ordered_skips_nomodule() {
    let doc = lumen_html_parser::parse(
        r#"<html><body>
              <script type="module" src="/modern.js"></script>
              <script nomodule src="/legacy.js"></script>
              <script nomodule>legacyInline=1;</script>
              <script>plain=1;</script>
            </body></html>"#,
    );
    let mut classic = Vec::new();
    let mut modules = Vec::new();
    collect_scripts_ordered(&doc, doc.root(), &mut classic, &mut modules);
    assert_eq!(modules.len(), 1);
    assert!(matches!(&modules[0], ScriptSource::External(_, s) if s == "/modern.js"));
    assert_eq!(classic.len(), 1, "обе nomodule-сборки должны быть пропущены");
    assert!(matches!(&classic[0], ScriptSource::Inline(_, s) if s.contains("plain=1")));
}

/// When both `src` and an inline body are present, `src` wins and the inline
/// body is ignored (HTML LS §4.12.1).
#[test]
fn collect_scripts_ordered_src_wins_over_inline_body() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><script src="/x.js">ignored=1;</script></body></html>"#,
    );
    let mut classic = Vec::new();
    let mut modules = Vec::new();
    collect_scripts_ordered(&doc, doc.root(), &mut classic, &mut modules);
    assert_eq!(classic.len(), 1);
    assert!(matches!(&classic[0], ScriptSource::External(_, s) if s == "/x.js"));
}

/// Inline items resolve to their body verbatim without any fetch (the
/// no-network path of `resolve_script_sources`).
#[test]
fn resolve_script_sources_passes_inline_through() {
    let doc = lumen_html_parser::parse(
        "<script>var a = 1;</script><script>var b = 2;</script>",
    );
    let mut items: Vec<ScriptSource> = Vec::new();
    let mut modules: Vec<ScriptSource> = Vec::new();
    collect_scripts_ordered(&doc, doc.root(), &mut items, &mut modules);
    let base = ResourceBase::Url("https://example.com/".to_owned());
    let sink: Arc<dyn EventSink> = Arc::new(StdoutEventSink);
    let out = resolve_script_sources(&items, &base, &sink, None, &doc);
    let bodies: Vec<&str> = out.iter().map(|r| r.source.as_str()).collect();
    assert_eq!(bodies, vec!["var a = 1;", "var b = 2;"]);
    // BUG-486: each body keeps the id of its own `<script>` element, so the
    // executor can point `document.currentScript` at it.
    assert_ne!(out[0].node, out[1].node);
    // Inline-скрипт своего адреса не имеет — база импортов остаётся страницей.
    assert!(out[0].url.is_none());
}

#[test]
fn run_scripts_blocked_by_sandbox() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><script>x=1;</script></body></html>"#,
    );
    let count = run_scripts(&doc, lumen_core::SandboxFlags::SCRIPTS, &lumen_core::NullJsRuntime);
    assert_eq!(count, 0);
}

#[test]
fn run_scripts_allowed_calls_runtime() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><script>x=1;</script></body></html>"#,
    );
    // empty() — без ограничений, скрипты разрешены; NullJsRuntime → NotImplemented
    let count = run_scripts(&doc, lumen_core::SandboxFlags::empty(), &lumen_core::NullJsRuntime);
    assert_eq!(count, 1);
}

#[test]
fn run_scripts_no_scripts_returns_zero() {
    let doc = lumen_html_parser::parse(
        r#"<html><head></head><body><p>no scripts</p></body></html>"#,
    );
    let count = run_scripts(&doc, lumen_core::SandboxFlags::empty(), &lumen_core::NullJsRuntime);
    assert_eq!(count, 0);
}

// ── navigation gate ──────────────────────────────────────────────────────

#[test]
fn navigation_gate_blocked_by_sandbox_returns_count() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><a href="/page1">link</a><a href="/page2">link2</a></body></html>"#,
    );
    assert_eq!(check_navigation_gate(&doc, lumen_core::SandboxFlags::NAVIGATION), 2);
}

#[test]
fn navigation_gate_allowed_returns_zero() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><a href="/page1">link</a></body></html>"#,
    );
    assert_eq!(check_navigation_gate(&doc, lumen_core::SandboxFlags::empty()), 0);
}

#[test]
fn navigation_gate_no_anchors_returns_zero() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><p>no links</p></body></html>"#,
    );
    assert_eq!(check_navigation_gate(&doc, lumen_core::SandboxFlags::NAVIGATION), 0);
}

// ── apply_iframe_sandbox_gates ───────────────────────────────────────────

#[test]
fn iframe_sandbox_no_iframes_returns_zero() {
    let doc = lumen_html_parser::parse(r#"<html><body><p>hello</p></body></html>"#);
    assert_eq!(apply_iframe_sandbox_gates(&doc), 0);
}

#[test]
fn iframe_sandbox_url_based_no_blocking() {
    // URL-based iframes are Phase 0 (not loaded); gate returns 0 blocked.
    let doc = lumen_html_parser::parse(
        r#"<html><body><iframe src="http://example.com" sandbox></iframe></body></html>"#,
    );
    assert_eq!(apply_iframe_sandbox_gates(&doc), 0);
}

#[test]
fn iframe_sandbox_srcdoc_scripts_blocked() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><iframe sandbox srcdoc="<script>x=1;</script><script>y=2;</script>"></iframe></body></html>"#,
    );
    // 2 scripts + 1 popup capability (AUXILIARY_NAVIGATION set in full sandbox).
    assert_eq!(apply_iframe_sandbox_gates(&doc), 3);
}

#[test]
fn iframe_sandbox_srcdoc_scripts_allowed() {
    // allow-scripts lifts SCRIPTS; AUXILIARY_NAVIGATION still set → popup blocked (+1).
    let doc = lumen_html_parser::parse(
        r#"<html><body><iframe sandbox="allow-scripts" srcdoc="<script>x=1;</script>"></iframe></body></html>"#,
    );
    assert_eq!(apply_iframe_sandbox_gates(&doc), 1);
}

#[test]
fn iframe_sandbox_srcdoc_forms_blocked() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><iframe sandbox srcdoc="<form action='/submit'><input type='submit'></form>"></iframe></body></html>"#,
    );
    // 1 form + 1 popup capability (full sandbox).
    assert_eq!(apply_iframe_sandbox_gates(&doc), 2);
}

#[test]
fn iframe_sandbox_srcdoc_navigation_blocked() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><iframe sandbox srcdoc="<a href='/page1'>link1</a><a href='/page2'>link2</a>"></iframe></body></html>"#,
    );
    // 2 navigation links + 1 popup capability (full sandbox).
    assert_eq!(apply_iframe_sandbox_gates(&doc), 3);
}

#[test]
fn iframe_sandbox_srcdoc_no_sandbox_attr_no_blocking() {
    // iframe without sandbox attribute: is_sandboxed = false, no blocking.
    let doc = lumen_html_parser::parse(
        r#"<html><body><iframe srcdoc="<script>x=1;</script>"></iframe></body></html>"#,
    );
    assert_eq!(apply_iframe_sandbox_gates(&doc), 0);
}

// ── frame_access_allowed (BUG-480 срез 2) ────────────────────────────────

#[test]
fn frame_access_about_urls_inherit_parent_origin() {
    let base = ResourceBase::Url("https://a.example/x/y.html".to_owned());
    assert!(frame_access_allowed(&base, "about:srcdoc", false));
    assert!(frame_access_allowed(&base, "about:blank", false));
}

#[test]
fn frame_access_opaque_sandbox_denies_everything() {
    let base = ResourceBase::Url("https://a.example/".to_owned());
    assert!(!frame_access_allowed(&base, "about:srcdoc", true));
    assert!(!frame_access_allowed(&base, "https://a.example/child.html", true));
}

#[test]
fn frame_access_same_origin_allowed_cross_origin_denied() {
    let a = ResourceBase::Url("https://a.example/x.html".to_owned());
    assert!(frame_access_allowed(
        &a,
        "https://a.example/child.html",
        false
    ));
    // Порт по умолчанию и регистр хоста не влияют на совпадение origin.
    assert!(frame_access_allowed(
        &a,
        "HTTPS://A.EXAMPLE:443/other.html",
        false
    ));
    assert!(!frame_access_allowed(
        &a,
        "https://b.example/child.html",
        false
    ));
    assert!(!frame_access_allowed(
        &a,
        "http://a.example/child.html",
        false
    ));
}

#[test]
fn frame_access_file_parent_talks_only_to_files() {
    let f = ResourceBase::File(std::path::PathBuf::from("D:/pages/index.html"));
    assert!(frame_access_allowed(&f, "file://D:/pages/child.html", false));
    assert!(!frame_access_allowed(&f, "https://a.example/", false));
}

#[test]
fn frame_access_url_parent_to_file_child_denied() {
    // У file://-ребёнка opaque origin — сетевой родитель его не читает.
    let u = ResourceBase::Url("https://a.example/".to_owned());
    assert!(!frame_access_allowed(&u, "file://D:/x.html", false));
}

// ── host_content_rect (BUG-480 срезы 13/14) ─────────────────────────────────

/// Вьюпорт под-документа — КОНТЕНТНЫЙ бокс хоста, а не его `rect`
/// (border-бокс): `<iframe width=400 height=200>` с рамкой и padding должен
/// дать ребёнку ровно 400×200, а не 400+10+6.
///
/// Второй ассерт на `rect` не декоративный: без него тест прошёл бы и в том
/// случае, если бы вычитание рамок вовсе не выполнялось, а атрибуты
/// резолвились как border-бокс.
///
/// Третий — про НАЧАЛО прямоугольника, а не про размер (срез 14): по нему
/// вклеивается display list ребёнка, и сдвиг на рамку+padding здесь означал бы
/// содержимое фрейма, нарисованное поверх его собственной рамки.
#[test]
fn frame_host_content_size_is_content_box_not_border_box() {
    let doc = lumen_html_parser::parse(
        r#"<html><body style="margin:0"><iframe width="400" height="200"
             style="border:5px solid black; padding:3px"></iframe></body></html>"#,
    );
    let infos = collect_iframes(&doc);
    assert_eq!(infos.len(), 1);

    let font = lumen_font::Font::parse(INTER_FONT).unwrap();
    let measurer = crate::relayout::page_measurer(&font, &[]);
    let sheet = lumen_css_parser::parse("");
    let layout =
        lumen_layout::layout_measured(&doc, &sheet, Size::new(1024.0, 720.0), &measurer);
    let host = crate::forms::find_layout_box(&layout, infos[0].node).expect("бокс <iframe>");

    let content = crate::frames::host_content_rect(host);
    assert!(
        (content.width - 400.0).abs() < 0.5 && (content.height - 200.0).abs() < 0.5,
        "контентный бокс = атрибуты width/height, получено {content:?}"
    );
    assert!(
        host.rect.width > 410.0 && host.rect.height > 210.0,
        "border-бокс шире контентного на padding 3+3 и рамку 5+5: {:?}",
        host.rect
    );
    assert!(
        (content.x - (host.rect.x + 8.0)).abs() < 0.5
            && (content.y - (host.rect.y + 8.0)).abs() < 0.5,
        "начало контентного бокса сдвинуто на рамку 5 + padding 3: {content:?} против {:?}",
        host.rect
    );
}

// ── splice_frame_content (BUG-480 срез 14) ──────────────────────────────────

/// Хэндл фрейма, у которого заполнено ровно то, что читает вклейка: адрес
/// заглушки (`host_src` + `host_rect`) и содержимое (`content_dl`).
///
/// Остальные поля — минимально живые заглушки: под-документ вклейке не нужен,
/// она работает уже по готовому display list ребёнка.
pub(super) fn splice_handle(src: &str, host_rect: Rect, content_dl: DisplayList) -> crate::frames::FrameHandle {
    crate::frames::FrameHandle {
        host: NodeId::from_index(0),
        url: "about:blank".to_owned(),
        load_failed: false,
        base: crate::ResourceBase::Url("about:blank".to_owned()),
        doc: Arc::new(Mutex::new(lumen_html_parser::parse("<html></html>"))),
        js: None,
        depth: 0,
        sheet: lumen_css_parser::Stylesheet::default(),
        viewport: Size::new(host_rect.width, host_rect.height),
        parent_doc: None,
        layout: None,
        content_dl,
        interactive: crate::frames::FrameNodeState::default(),
        host_rect: Some(host_rect),
        host_src: src.to_owned(),
        images: Vec::new(),
        image_keys: Vec::new(),
        scroll_y: 0.0,
        scroll_x: 0.0,
        scroll_containers: Vec::new(),
        font_registry: lumen_font::FontRegistry::new(),
        web_fonts: Vec::new(),
        animated_gifs: Vec::new(),
        lazy_requests: Vec::new(),
        pending_lazy: Vec::new(),
    }
}

/// Страница с одним `<iframe src>`: её display list (с серой заглушкой внутри)
/// и контентный бокс хоста, посчитанный тем же [`host_content_rect`], которым
/// пользуется [`sync_frame_viewports`].
///
/// Заглушку рисует НАСТОЯЩИЙ эмиттер (`paint_ordered` → ветка `BoxKind::Iframe`),
/// а не рукописная команда: вклейка ищет её по паре «src + прямоугольник», и
/// расхождение эмиттера с `host_content_rect` хоть на рамку означало бы, что
/// поиск не находит ничего и содержимое фрейма молча не рисуется.
pub(super) fn page_with_iframe_placeholder(src: &str) -> (DisplayList, Rect) {
    let html = format!(
        r#"<html><body style="margin:0"><iframe src="{src}" width="400" height="200"
             style="border:5px solid black; padding:3px"></iframe></body></html>"#
    );
    let doc = lumen_html_parser::parse(&html);
    let infos = collect_iframes(&doc);
    assert_eq!(infos.len(), 1);
    let font = lumen_font::Font::parse(INTER_FONT).unwrap();
    let measurer = crate::relayout::page_measurer(&font, &[]);
    let sheet = lumen_css_parser::parse("");
    let layout = lumen_layout::layout_measured(&doc, &sheet, Size::new(1024.0, 720.0), &measurer);
    let host = crate::forms::find_layout_box(&layout, infos[0].node).expect("бокс <iframe>");
    let rect = crate::frames::host_content_rect(host);
    (paint_ordered(&layout), rect)
}

/// Позиция команды-заглушки `<iframe>` в списке — по её `src`.
pub(super) fn placeholder_at(dl: &DisplayList, src: &str) -> Option<usize> {
    dl.iter().position(|c| {
        matches!(c, lumen_paint::DisplayCommand::DrawImage { src: s, .. } if s == src)
    })
}

/// Вклейка заменяет серую заглушку содержимым под-документа, обёрнутым в клип
/// по контентному боксу хоста и сдвиг к его началу.
///
/// Сдвиг проверяется по КООРДИНАТАМ, а не по факту наличия `PushTransform`:
/// список ребёнка начинается от его собственного (0, 0), поэтому ошибка в
/// смещении рисует содержимое фрейма в углу страницы, а не внутри фрейма.
#[test]
fn splice_frame_content_replaces_placeholder_with_child_list() {
    let (mut dl, host_rect) = page_with_iframe_placeholder("child.html");
    let at = placeholder_at(&dl, "child.html").expect("эмиттер обязан нарисовать заглушку");

    let marker = Rect::new(0.0, 0.0, 40.0, 20.0);
    let content = vec![lumen_paint::DisplayCommand::FillRect {
        rect: marker,
        color: lumen_layout::Color { r: 1, g: 2, b: 3, a: 255 },
    }];
    let frames = vec![splice_handle("child.html", host_rect, content)];
    crate::frames::splice_frame_content(&mut dl, &frames);

    assert!(
        placeholder_at(&dl, "child.html").is_none(),
        "заглушка обязана исчезнуть — иначе поверх содержимого остаётся серый прямоугольник"
    );
    match &dl[at] {
        lumen_paint::DisplayCommand::PushClipRect { rect } => assert!(
            (rect.x - host_rect.x).abs() < 0.01 && (rect.width - host_rect.width).abs() < 0.01,
            "клип = контентный бокс хоста: {rect:?} против {host_rect:?}"
        ),
        other => panic!("на месте заглушки ожидался PushClipRect, получено {other:?}"),
    }
    match &dl[at + 1] {
        lumen_paint::DisplayCommand::PushTransform { matrix } => {
            let expected = lumen_layout::Mat4::translation_2d(host_rect.x, host_rect.y);
            assert_eq!(
                matrix.0, expected.0,
                "сдвиг = начало контентного бокса, иначе содержимое рисуется мимо фрейма"
            );
        }
        other => panic!("после клипа ожидался PushTransform, получено {other:?}"),
    }
    assert!(
        matches!(&dl[at + 2], lumen_paint::DisplayCommand::FillRect { rect, .. } if *rect == marker),
        "содержимое ребёнка идёт в его СОБСТВЕННЫХ координатах, без предварительного сдвига"
    );
    assert!(matches!(&dl[at + 3], lumen_paint::DisplayCommand::PopTransform));
    assert!(matches!(&dl[at + 4], lumen_paint::DisplayCommand::PopClip));
}

/// Повторный проход по уже склеенному списку ничего не делает.
///
/// Не декоративно: [`Lumen::set_display_list`] вызывается и на путях, где
/// список уже прошёл через вклейку (кэш, повторная запись того же списка), а
/// вторая вклейка означала бы содержимое фрейма, вложенное само в себя.
#[test]
fn splice_frame_content_is_idempotent() {
    let (mut dl, host_rect) = page_with_iframe_placeholder("child.html");
    let content = vec![lumen_paint::DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, 40.0, 20.0),
        color: lumen_layout::Color { r: 1, g: 2, b: 3, a: 255 },
    }];
    let frames = vec![splice_handle("child.html", host_rect, content)];
    crate::frames::splice_frame_content(&mut dl, &frames);
    let after_first = dl.len();
    crate::frames::splice_frame_content(&mut dl, &frames);
    assert_eq!(after_first, dl.len(), "вторая вклейка обязана быть no-op");
}

/// Заглушка ищется по ПАРЕ «src + прямоугольник»: совпадения одного `src` мало.
///
/// Два `<iframe src="">` на странице — обычное дело, и склеить содержимое
/// первого в бокс второго хуже, чем не склеить вовсе.
#[test]
fn splice_frame_content_needs_matching_rect_not_just_src() {
    let (mut dl, host_rect) = page_with_iframe_placeholder("child.html");
    let before = dl.clone();
    let moved = Rect::new(host_rect.x + 50.0, host_rect.y, host_rect.width, host_rect.height);
    let content = vec![lumen_paint::DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, 40.0, 20.0),
        color: lumen_layout::Color { r: 1, g: 2, b: 3, a: 255 },
    }];
    let frames = vec![splice_handle("child.html", moved, content)];
    crate::frames::splice_frame_content(&mut dl, &frames);
    assert_eq!(before.len(), dl.len(), "чужой бокс — не наша заглушка, список не трогаем");
    assert!(placeholder_at(&dl, "child.html").is_some());
}

// ── rekey_frame_images (BUG-480 срез 15) ────────────────────────────────────

/// Ключи картинок под-документа переписываются на разрешённые адреса, а `src`
/// заглушки ВЛОЖЕННОГО фрейма остаётся нетронутым.
///
/// Второе не мелочь и не гипотетика: заглушка ищется вклейкой именно по `src`,
/// поэтому переписанный ключ означал бы серый прямоугольник вместо содержимого
/// внука — при том что сама картинка нарисовалась бы правильно, то есть дефект
/// выглядел бы как «вложенные фреймы перестали работать».
#[test]
fn rekey_frame_images_rewrites_own_images_and_spares_nested_placeholder() {
    let img = |src: &str| lumen_paint::DisplayCommand::DrawImage {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        src: src.to_owned(),
        alt: String::new(),
        object_fit: lumen_layout::ObjectFit::default(),
        object_position: lumen_layout::ObjectPosition::default(),
        image_rendering: lumen_layout::ImageRendering::default(),
    };
    let mut dl = vec![img("pic.png"), img("nested.html"), img("other.png")];

    let mut parent = splice_handle("child.html", Rect::new(0.0, 0.0, 100.0, 100.0), Vec::new());
    parent.image_keys = vec![
        ("pic.png".to_owned(), "http://h/a/pic.png".to_owned()),
        // Патологическая разметка: `<img>` на тот же адрес, что вложенный
        // фрейм. Побеждает фрейм — картинку он всё равно не показал бы.
        ("nested.html".to_owned(), "http://h/a/nested.html".to_owned()),
    ];
    let mut nested = splice_handle("nested.html", Rect::new(0.0, 0.0, 50.0, 50.0), Vec::new());
    nested.depth = 1;
    nested.parent_doc = Some(Arc::clone(&parent.doc));
    let frames = vec![parent, nested];

    crate::frames::rekey_frame_images(&mut dl, &frames, 0);

    let srcs: Vec<String> = dl
        .iter()
        .map(|c| match c {
            lumen_paint::DisplayCommand::DrawImage { src, .. } => src.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(srcs[0], "http://h/a/pic.png", "своя картинка получает разрешённый ключ");
    assert_eq!(srcs[1], "nested.html", "заглушка вложенного фрейма остаётся адресуемой");
    assert_eq!(srcs[2], "other.png", "чего нет в карте — не трогаем");
}

/// FRAME-5: `DrawBackgroundImage.src` и `DrawCrossFade.{src_a,src_b}`
/// переписываются тем же ключом, что и `DrawImage` — без заглушечного
/// исключения (только `<img>`-заглушка вложенного фрейма его требует).
#[test]
fn rekey_frame_images_rewrites_background_image_and_cross_fade() {
    let mut dl = vec![
        lumen_paint::DisplayCommand::DrawBackgroundImage {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            origin_rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            src: "bg.png".to_owned(),
            size: lumen_layout::BackgroundSize::default(),
            position: lumen_layout::ObjectPosition::default(),
            repeat: lumen_layout::BackgroundRepeat::default(),
            image_rendering: lumen_layout::ImageRendering::default(),
        },
        lumen_paint::DisplayCommand::DrawCrossFade {
            dest: Rect::new(0.0, 0.0, 10.0, 10.0),
            src_a: "a.png".to_owned(),
            src_b: "untouched.png".to_owned(),
            progress: 0.5,
        },
    ];
    let mut parent = splice_handle("child.html", Rect::new(0.0, 0.0, 100.0, 100.0), Vec::new());
    parent.image_keys = vec![
        ("bg.png".to_owned(), "http://h/a/bg.png".to_owned()),
        ("a.png".to_owned(), "http://h/a/a.png".to_owned()),
    ];
    let frames = vec![parent];

    crate::frames::rekey_frame_images(&mut dl, &frames, 0);

    match &dl[0] {
        lumen_paint::DisplayCommand::DrawBackgroundImage { src, .. } => {
            assert_eq!(src, "http://h/a/bg.png");
        }
        other => panic!("ожидался DrawBackgroundImage, получено {other:?}"),
    }
    match &dl[1] {
        lumen_paint::DisplayCommand::DrawCrossFade { src_a, src_b, .. } => {
            assert_eq!(src_a, "http://h/a/a.png", "своя сторона переписывается");
            assert_eq!(src_b, "untouched.png", "чего нет в карте — не трогаем");
        }
        other => panic!("ожидался DrawCrossFade, получено {other:?}"),
    }
}
