//! Сбор скриптов документа, журнал парсерных вставок, песочница `<iframe>`,
//! доступ между фреймами, режим Tor и потоковая выдача картинок.

use super::*;

// в”Ђв”Ђ BUG-164: external <script src> collection в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

/// External `<script src>` is recorded as `External` in document order,
/// interleaved with inline classic scripts (HTML LS В§8.1.3.1).
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

/// BUG-804: СЃРєСЂРёРїС‚, С‡РµР№ С„Р°Р№Р» РЅРµ РїСЂРёС€С‘Р», РѕР±СЏР·Р°РЅ РѕСЃС‚Р°С‚СЊСЃСЏ РІ СЃРїРёСЃРєРµ вЂ” РёРЅР°С‡Рµ
/// РµРіРѕ СЌР»РµРјРµРЅС‚Сѓ РЅРµРіРґРµ РІС‹СЃС‚СЂРµР»РёС‚СЊ `error`. Р Р°РЅСЊС€Рµ `resolve_script_sources`
/// С‚Р°РєРѕР№ СЃРєСЂРёРїС‚ РјРѕР»С‡Р° РІС‹Р±СЂР°СЃС‹РІР°Р», Рё СЃС‚СЂР°РЅРёС†Р° РЅРµ СѓР·РЅР°РІР°Р»Р° РѕР± РѕС‚РєР°Р·Рµ РЅРёС‡РµРіРѕ.
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
    let resolved = resolve_script_sources(&classic, &base, &sink, None);
    assert_eq!(resolved.len(), 2, "the failed script keeps its slot");
    assert_eq!(resolved[0].external_ok, Some(false));
    assert!(resolved[0].source.is_empty(), "no body to execute");
    assert_eq!(resolved[1].external_ok, None, "inline owes no load event");
    assert!(resolved[1].source.contains("ok=1"));
}

// в”Ђв”Ђ BUG-827: РїРѕСЂСЏРґРѕРє РїР°СЂСЃРµСЂРЅС‹С… РІСЃС‚Р°РІРѕРє РґР»СЏ MutationObserver в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

/// РЎРѕР±СЂР°С‚СЊ `ResolvedScript` РёР· СЂРµР·СѓР»СЊС‚Р°С‚Р° [`collect_scripts_ordered`] вЂ”
/// С‚РµР»Р° РІРЅРµС€РЅРёС… СЃРєСЂРёРїС‚РѕРІ С‚РµСЃС‚Сѓ РЅРµ РЅСѓР¶РЅС‹, РІР°Р¶РµРЅ С‚РѕР»СЊРєРѕ СѓР·РµР».
fn resolved_for_test(items: &[ScriptSource]) -> Vec<ResolvedScript> {
    items
        .iter()
        .map(|s| {
            let (node, source) = match s {
                ScriptSource::Inline(n, src) | ScriptSource::External(n, src) => (*n, src),
            };
            ResolvedScript { node, source: source.clone(), url: None, external_ok: None }
        })
        .collect()
}

fn count_nodes(doc: &Document, id: NodeId) -> usize {
    1 + doc.get(id).children.iter().map(|&c| count_nodes(doc, c)).sum::<usize>()
}

/// Р–СѓСЂРЅР°Р» РїРµСЂРµС‡РёСЃР»СЏРµС‚ РєР°Р¶РґС‹Р№ СѓР·РµР» РґРѕРєСѓРјРµРЅС‚Р° СЂРѕРІРЅРѕ РѕРґРёРЅ СЂР°Р· (РєСЂРѕРјРµ РєРѕСЂРЅСЏ,
/// РєРѕС‚РѕСЂС‹Р№ РЅРёРѕС‚РєСѓРґР° РЅРµ РІСЃС‚Р°РІР»СЏРµС‚СЃСЏ), РІ РїРѕСЂСЏРґРєРµ РґРµСЂРµРІР°.
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
    assert_eq!(seen.len(), before, "РЅРё РѕРґРёРЅ СѓР·РµР» РЅРµ РІСЃС‚Р°РІР»РµРЅ РґРІР°Р¶РґС‹");
}

/// РћС‚СЂРµР·РѕРє СЃРєСЂРёРїС‚Р° РєРѕРЅС‡Р°РµС‚СЃСЏ РЅР° РЅС‘Рј СЃР°РјРѕРј (РІРјРµСЃС‚Рµ СЃ РµРіРѕ С‚РµРєСЃС‚РѕРј), Р° С‚Рѕ, С‡С‚Рѕ
/// СЃС‚РѕРёС‚ РІ РґРѕРєСѓРјРµРЅС‚Рµ РЅРёР¶Рµ, РїРѕРїР°РґР°РµС‚ СѓР¶Рµ РІ СЃР»РµРґСѓСЋС‰РёР№ РѕС‚СЂРµР·РѕРє: РЅР°СЃС‚РѕСЏС‰РёР№
/// РїР°СЂСЃРµСЂ РІСЃС‚Р°РІРёР» Р±С‹ СЌС‚Рѕ РїРѕСЃР»Рµ С‚РѕРіРѕ, РєР°Рє СЃРєСЂРёРїС‚ РѕС‚СЂР°Р±РѕС‚Р°Р».
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
    assert!(at_script < all, "РЅРёР¶Рµ СЃРєСЂРёРїС‚Р° РІ РґРѕРєСѓРјРµРЅС‚Рµ РµС‰С‘ РµСЃС‚СЊ СѓР·Р»С‹");

    // РџРѕСЃР»РµРґРЅСЏСЏ РїР°СЂР° РѕС‚СЂРµР·РєР° вЂ” С‚РµРєСЃС‚ СЃР°РјРѕРіРѕ СЃРєСЂРёРїС‚Р°.
    let (parent, _) = log.pairs[at_script - 1];
    assert_eq!(parent, scripts[0].node.index());

    // РџРµСЂРІР°СЏ РїР°СЂР° СЃР»РµРґСѓСЋС‰РµРіРѕ РѕС‚СЂРµР·РєР° вЂ” <p>.
    let (_, child) = log.pairs[at_script];
    let node = doc.get(NodeId::from_index(child));
    assert!(matches!(&node.data, NodeData::Element { name, .. } if name.local == "p"));
}

/// Р‘РµР· РєР»Р°СЃСЃРёС‡РµСЃРєРёС… СЃРєСЂРёРїС‚РѕРІ Р¶СѓСЂРЅР°Р» РїСѓСЃС‚: РЅР°Р±Р»СЋРґР°С‚РµР»СЏ СЃС‚Р°РІРёС‚СЊ РЅРµРєРѕРјСѓ, Р°
/// РјРѕРґСѓР»Рё РёСЃРїРѕР»РЅСЏСЋС‚СЃСЏ, РєРѕРіРґР° РїР°СЂСЃРµСЂ СѓР¶Рµ РІСЃС‘ РІСЃС‚Р°РІРёР» (HTML LS В§8.1.3.1).
#[test]
fn parser_insert_log_is_empty_without_classic_scripts() {
    let doc = lumen_html_parser::parse(
        r#"<html><body><div></div><script type="module">export const y = 2;</script></body></html>"#,
    );
    let log = ParserInsertLog::build(&doc, &[]);
    assert!(log.pairs.is_empty());
    assert_eq!(log.segment_end(None), 0);
}

/// `<script type=module src>` lands in the module list as `External`.
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
/// code вЂ” they must not be collected for execution, with or without `src`.
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

/// `nomodule` вЂ” Р·Р°РїР°СЃРЅР°СЏ СЃР±РѕСЂРєР° РґР»СЏ РґРІРёР¶РєР° Р±РµР· ES-РјРѕРґСѓР»РµР№. Р”РІРёР¶РѕРє СЃ
/// РјРѕРґСѓР»СЏРјРё РѕР±СЏР·Р°РЅ РµС‘ РїСЂРѕРїСѓСЃС‚РёС‚СЊ, РёРЅР°С‡Рµ СЃР°Р№С‚ РїРѕР»СѓС‡Р°РµС‚ РѕР±Рµ СЃР±РѕСЂРєРё СЂР°Р·РѕРј
/// (Р¶РёРІРѕР№ РїСЂРёРјРµСЂ вЂ” С„РѕСЂРјР° РІС…РѕРґР° id.tbank.ru: legacy Рё СЃРѕРІСЂРµРјРµРЅРЅС‹Р№ Р±Р°РЅРґР»
/// РјРѕРЅС‚РёСЂРѕРІР°Р»РёСЃСЊ РІ РѕРґРёРЅ РєРѕСЂРµРЅСЊ Рё РіР°СЃРёР»Рё РґСЂСѓРі РґСЂСѓРіР°).
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
    assert_eq!(classic.len(), 1, "РѕР±Рµ nomodule-СЃР±РѕСЂРєРё РґРѕР»Р¶РЅС‹ Р±С‹С‚СЊ РїСЂРѕРїСѓС‰РµРЅС‹");
    assert!(matches!(&classic[0], ScriptSource::Inline(_, s) if s.contains("plain=1")));
}

/// When both `src` and an inline body are present, `src` wins and the inline
/// body is ignored (HTML LS В§4.12.1).
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
    let out = resolve_script_sources(&items, &base, &sink, None);
    let bodies: Vec<&str> = out.iter().map(|r| r.source.as_str()).collect();
    assert_eq!(bodies, vec!["var a = 1;", "var b = 2;"]);
    // BUG-486: each body keeps the id of its own `<script>` element, so the
    // executor can point `document.currentScript` at it.
    assert_ne!(out[0].node, out[1].node);
    // Inline-СЃРєСЂРёРїС‚ СЃРІРѕРµРіРѕ Р°РґСЂРµСЃР° РЅРµ РёРјРµРµС‚ вЂ” Р±Р°Р·Р° РёРјРїРѕСЂС‚РѕРІ РѕСЃС‚Р°С‘С‚СЃСЏ СЃС‚СЂР°РЅРёС†РµР№.
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
    // empty() вЂ” Р±РµР· РѕРіСЂР°РЅРёС‡РµРЅРёР№, СЃРєСЂРёРїС‚С‹ СЂР°Р·СЂРµС€РµРЅС‹; NullJsRuntime в†’ NotImplemented
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

// в”Ђв”Ђ navigation gate в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

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

// в”Ђв”Ђ apply_iframe_sandbox_gates в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

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
    // allow-scripts lifts SCRIPTS; AUXILIARY_NAVIGATION still set в†’ popup blocked (+1).
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

// в”Ђв”Ђ frame_access_allowed (BUG-480 СЃСЂРµР· 2) в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

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
    // РџРѕСЂС‚ РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ Рё СЂРµРіРёСЃС‚СЂ С…РѕСЃС‚Р° РЅРµ РІР»РёСЏСЋС‚ РЅР° СЃРѕРІРїР°РґРµРЅРёРµ origin.
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
    // РЈ file://-СЂРµР±С‘РЅРєР° opaque origin вЂ” СЃРµС‚РµРІРѕР№ СЂРѕРґРёС‚РµР»СЊ РµРіРѕ РЅРµ С‡РёС‚Р°РµС‚.
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
fn splice_handle(src: &str, host_rect: Rect, content_dl: DisplayList) -> crate::frames::FrameHandle {
    crate::frames::FrameHandle {
        host: NodeId::from_index(0),
        url: "about:blank".to_owned(),
        doc: Arc::new(Mutex::new(lumen_html_parser::parse("<html></html>"))),
        js: None,
        depth: 0,
        sheet: lumen_css_parser::Stylesheet::default(),
        viewport: Size::new(host_rect.width, host_rect.height),
        parent_doc: None,
        layout: None,
        content_dl,
        host_rect: Some(host_rect),
        host_src: src.to_owned(),
        images: Vec::new(),
        image_keys: Vec::new(),
        scroll_y: 0.0,
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
fn page_with_iframe_placeholder(src: &str) -> (DisplayList, Rect) {
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
fn placeholder_at(dl: &DisplayList, src: &str) -> Option<usize> {
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

// ── pointer_target (BUG-480 срез 16) ───────────────────────────────────────

/// Разметка → (документ, его layout на `size`).
fn laid_out(html: &str, size: Size) -> (Document, lumen_layout::LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let font = lumen_font::Font::parse(INTER_FONT).unwrap();
    let measurer = crate::relayout::page_measurer(&font, &[]);
    let sheet = lumen_css_parser::parse("");
    let layout = lumen_layout::layout_measured(&doc, &sheet, size, &measurer);
    (doc, layout)
}

/// Страница с одним `<iframe>` в известном месте и ребёнок с одной цветной
/// плашкой в своём левом верхнем углу: layout страницы, готовый хэндл фрейма и
/// `NodeId` плашки ребёнка.
///
/// Хост-бокс считается тем же [`host_content_rect`], которым его считает
/// [`sync_frame_viewports`]: тест о переводе координат, и своя арифметика
/// прямоугольника проверяла бы сама себя.
fn page_with_live_frame() -> (lumen_layout::LayoutBox, crate::frames::FrameHandle, NodeId) {
    let (page_doc, page_layout) = laid_out(
        r#"<html><body style="margin:0">
             <iframe src="c.html" width="300" height="200"
                     style="border:0;position:absolute;left:40px;top:120px"></iframe>
           </body></html>"#,
        Size::new(1024.0, 720.0),
    );
    let infos = collect_iframes(&page_doc);
    assert_eq!(infos.len(), 1);
    let host_box =
        crate::forms::find_layout_box(&page_layout, infos[0].node).expect("бокс <iframe>");
    let host_rect = crate::frames::host_content_rect(host_box);

    let (child_doc, child_layout) = laid_out(
        r#"<html><body style="margin:0"><div id="t"
             style="position:absolute;left:0;top:0;width:200px;height:100px;background:#f00">
           </div></body></html>"#,
        Size::new(host_rect.width, host_rect.height),
    );
    let target = child_doc.find_by_id("t").expect("плашка ребёнка");

    let mut handle = splice_handle("c.html", host_rect, Vec::new());
    handle.host = infos[0].node;
    handle.doc = Arc::new(Mutex::new(child_doc));
    handle.layout = Some(child_layout);
    (page_layout, handle, target)
}

/// Точка внутри содержимого фрейма адресует УЗЕЛ РЕБЁНКА, а координаты
/// пересчитываются в его систему.
///
/// Проверяются оба: узел без координат означал бы событие, пришедшее верному
/// слушателю с `clientX`/`clientY` от чужого вьюпорта, а координаты без узла —
/// событие, ушедшее не туда. `page` при этом остаётся host-элементом: именно
/// его фокусирует родитель.
#[test]
fn pointer_target_inside_frame_maps_to_child_node_and_local_point() {
    let (page_layout, handle, child_node) = page_with_live_frame();
    let host = handle.host;
    let frames = vec![handle];

    let t = crate::frames::pointer_target(&frames, &page_layout, Point::new(100.0, 150.0));
    let hit = t.frame.expect("точка внутри фрейма");
    assert_eq!(hit.frame, 0);
    assert_eq!(hit.hit.map(|h| h.node), Some(child_node), "узел РЕБЁНКА");
    assert!(
        (hit.client.x - 60.0).abs() < 0.5 && (hit.client.y - 30.0).abs() < 0.5,
        "координаты в системе ребёнка: {:?}",
        hit.client
    );
    assert_eq!(t.page.map(|h| h.node), Some(host), "у страницы под точкой — сам <iframe>");
}

/// Точка рядом с фреймом — обычный путь страницы, `frame` пуст.
#[test]
fn pointer_target_outside_frame_stays_on_page() {
    let (page_layout, handle, _) = page_with_live_frame();
    let frames = vec![handle];
    let t = crate::frames::pointer_target(&frames, &page_layout, Point::new(10.0, 10.0));
    assert!(t.frame.is_none(), "выше и левее фрейма — страница");
    assert!(t.page.is_some());
}

/// Хэндл без посчитанного layout (первый кадр до `sync_frame_viewports`) не
/// забирает событие себе: адресовать в под-документе всё равно нечего, а
/// проглоченный клик выглядел бы как мёртвая страница.
#[test]
fn pointer_target_frame_without_layout_stays_on_page() {
    let (page_layout, mut handle, _) = page_with_live_frame();
    handle.layout = None;
    let frames = vec![handle];
    let t = crate::frames::pointer_target(&frames, &page_layout, Point::new(100.0, 150.0));
    assert!(t.frame.is_none());
}

/// Попадание в САМ host-бокс мимо его контентной части (рамка `<iframe>`) —
/// это элемент родителя, а не под-документ: `host_rect` вычитает рамку и
/// padding, и точка на рамке в него не попадает.
#[test]
fn pointer_target_on_host_border_stays_on_page() {
    let (page_doc, page_layout) = laid_out(
        r#"<html><body style="margin:0">
             <iframe src="c.html" width="300" height="200"
                     style="border:10px solid black;position:absolute;left:40px;top:120px"></iframe>
           </body></html>"#,
        Size::new(1024.0, 720.0),
    );
    let infos = collect_iframes(&page_doc);
    let host_box =
        crate::forms::find_layout_box(&page_layout, infos[0].node).expect("бокс <iframe>");
    let host_rect = crate::frames::host_content_rect(host_box);
    let (child_doc, child_layout) = laid_out(
        r#"<html><body style="margin:0"><div style="width:200px;height:100px;background:#f00">
           </div></body></html>"#,
        Size::new(host_rect.width, host_rect.height),
    );
    let mut handle = splice_handle("c.html", host_rect, Vec::new());
    handle.host = infos[0].node;
    handle.doc = Arc::new(Mutex::new(child_doc));
    handle.layout = Some(child_layout);
    let frames = vec![handle];

    // (45, 125) — внутри border-бокса, но на рамке шириной 10.
    let t = crate::frames::pointer_target(&frames, &page_layout, Point::new(45.0, 125.0));
    assert!(t.frame.is_none(), "рамка хоста принадлежит родителю");
    // На 10 пикселей глубже — уже содержимое.
    let t = crate::frames::pointer_target(&frames, &page_layout, Point::new(60.0, 140.0));
    assert!(t.frame.is_some(), "контентная часть — под-документ");
}

/// Спуск идёт до САМОГО ГЛУБОКОГО фрейма, и координаты складываются по всей
/// цепочке. Кандидат ищется по паре «host-узел + документ-хозяин»: `NodeId`
/// уникален лишь внутри своего документа, и здесь индексы узлов страницы и
/// под-документа заведомо пересекаются.
#[test]
fn pointer_target_descends_into_nested_frame() {
    let (page_doc, page_layout) = laid_out(
        r#"<html><body style="margin:0">
             <iframe src="c.html" width="400" height="300"
                     style="border:0;position:absolute;left:40px;top:120px"></iframe>
           </body></html>"#,
        Size::new(1024.0, 720.0),
    );
    let outer_host = collect_iframes(&page_doc)[0].node;
    let outer_rect = crate::frames::host_content_rect(
        crate::forms::find_layout_box(&page_layout, outer_host).expect("бокс <iframe>"),
    );

    // Ребёнок сам держит `<iframe>` со сдвигом (20, 30) в своей системе.
    let (mid_doc, mid_layout) = laid_out(
        r#"<html><body style="margin:0">
             <iframe src="g.html" width="200" height="150"
                     style="border:0;position:absolute;left:20px;top:30px"></iframe>
           </body></html>"#,
        Size::new(outer_rect.width, outer_rect.height),
    );
    let inner_host = collect_iframes(&mid_doc)[0].node;
    let inner_rect = crate::frames::host_content_rect(
        crate::forms::find_layout_box(&mid_layout, inner_host).expect("бокс вложенного <iframe>"),
    );

    let (leaf_doc, leaf_layout) = laid_out(
        r#"<html><body style="margin:0"><div id="t"
             style="position:absolute;left:0;top:0;width:200px;height:150px;background:#00f">
           </div></body></html>"#,
        Size::new(inner_rect.width, inner_rect.height),
    );
    let leaf_node = leaf_doc.find_by_id("t").expect("плашка внука");

    let mut mid = splice_handle("c.html", outer_rect, Vec::new());
    mid.host = outer_host;
    mid.doc = Arc::new(Mutex::new(mid_doc));
    mid.layout = Some(mid_layout);

    let mut leaf = splice_handle("g.html", inner_rect, Vec::new());
    leaf.host = inner_host;
    leaf.depth = 1;
    leaf.parent_doc = Some(Arc::clone(&mid.doc));
    leaf.doc = Arc::new(Mutex::new(leaf_doc));
    leaf.layout = Some(leaf_layout);
    let frames = vec![mid, leaf];

    // (40+20+5, 120+30+7) — пять и семь пикселей вглубь содержимого внука.
    let t = crate::frames::pointer_target(&frames, &page_layout, Point::new(65.0, 157.0));
    let hit = t.frame.expect("точка внутри внука");
    assert_eq!(hit.frame, 1, "самый глубокий фрейм, а не первый попавшийся");
    assert_eq!(hit.hit.map(|h| h.node), Some(leaf_node));
    assert!(
        (hit.client.x - 5.0).abs() < 0.5 && (hit.client.y - 7.0).abs() < 0.5,
        "координаты сложены по всей цепочке: {:?}",
        hit.client
    );
}

// ── скролл под-документа (BUG-480 срез 17) ──────────────────────────────────

/// Хэндл фрейма 300×200 с содержимым высотой `content_h`.
fn scrollable_handle(content_h: f32) -> crate::frames::FrameHandle {
    let host_rect = Rect::new(40.0, 120.0, 300.0, 200.0);
    let content = vec![lumen_paint::DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, 300.0, content_h),
        color: lumen_layout::Color { r: 1, g: 2, b: 3, a: 255 },
    }];
    splice_handle("child.html", host_rect, content)
}

/// Предел прокрутки под-документа = «нарисованное минус вьюпорт», и он не
/// уходит в минус на содержимом короче вьюпорта.
///
/// Высота берётся из display list, а не из layout, ровно как у страницы: два
/// ответа на один вопрос внутри одного движка разойтись не должны.
#[test]
fn frame_max_scroll_is_painted_height_minus_viewport() {
    assert!((crate::frames::frame_max_scroll(&scrollable_handle(600.0)) - 400.0).abs() < 0.5);
    assert_eq!(crate::frames::frame_max_scroll(&scrollable_handle(50.0)), 0.0);
    // Пустой список — фрейм ещё не нарисован: прокручивать нечего.
    let mut empty = scrollable_handle(600.0);
    empty.content_dl = Vec::new();
    assert_eq!(crate::frames::frame_max_scroll(&empty), 0.0);
}

/// Прокрутка зажимается пределом, а «не сдвинулось» отличается от
/// «сдвинулось»: на этом ответе держится и отправка `scroll` ребёнку, и
/// передача остатка колеса странице.
#[test]
fn scroll_frame_to_clamps_and_reports_movement() {
    let mut frames = vec![scrollable_handle(600.0)];
    assert_eq!(crate::frames::scroll_frame_to(&mut frames, 0, 150.0), Some(150.0));
    assert_eq!(
        crate::frames::scroll_frame_to(&mut frames, 0, 150.0),
        None,
        "та же позиция — движения нет"
    );
    assert_eq!(
        crate::frames::scroll_frame_to(&mut frames, 0, 9999.0),
        Some(400.0),
        "зажим по нижнему краю содержимого"
    );
    assert_eq!(
        crate::frames::scroll_frame_to(&mut frames, 0, 9999.0),
        None,
        "у нижнего края колесо фрейм не двигает — остаток уходит странице"
    );
    assert_eq!(crate::frames::scroll_frame_to(&mut frames, 0, -50.0), Some(0.0));
}

/// Прокрутка уезжает в СДВИГ вклейки, а клип остаётся окном фрейма на
/// странице: клип — это дырка, в которую видно ребёнка, и двигать её нельзя.
#[test]
fn splice_frame_content_offsets_child_by_its_scroll() {
    let (mut dl, host_rect) = page_with_iframe_placeholder("child.html");
    let at = placeholder_at(&dl, "child.html").expect("заглушка");
    let mut handle = splice_handle("child.html", host_rect, vec![
        lumen_paint::DisplayCommand::FillRect {
            rect: Rect::new(0.0, 0.0, 300.0, 600.0),
            color: lumen_layout::Color { r: 1, g: 2, b: 3, a: 255 },
        },
    ]);
    handle.scroll_y = 120.0;
    crate::frames::splice_frame_content(&mut dl, &[handle]);

    match &dl[at] {
        lumen_paint::DisplayCommand::PushClipRect { rect } => assert!(
            (rect.y - host_rect.y).abs() < 0.01,
            "клип не двигается прокруткой ребёнка: {rect:?}"
        ),
        other => panic!("ожидался PushClipRect, получено {other:?}"),
    }
    match &dl[at + 1] {
        lumen_paint::DisplayCommand::PushTransform { matrix } => {
            let expected =
                lumen_layout::Mat4::translation_2d(host_rect.x, host_rect.y - 120.0);
            assert_eq!(matrix.0, expected.0, "содержимое уезжает вверх на прокрутку");
        }
        other => panic!("ожидался PushTransform, получено {other:?}"),
    }
}

/// В прокрученном фрейме hit-тест и координаты события — РАЗНЫЕ системы.
///
/// Найдено пробой `verify_frame_scroll.py`, а не постановкой: срез 16 отдавал
/// одну точку на оба вопроса, и это было верно ровно до появления прокрутки.
/// Узел ищется в layout, который о прокрутке не знает (её применяет вклейка),
/// значит точке поиска нужен + `scroll_y`; `clientX`/`clientY` спека
/// отсчитывает от ВЬЮПОРТА (CSSOM-View §10), значит наружу идёт точка БЕЗ
/// него. Перепутать их — это либо клик в блок, уехавший с экрана, либо верный
/// блок с координатами чужой системы; проверяются поэтому оба ответа сразу.
#[test]
fn pointer_target_in_scrolled_frame_hits_visible_block_with_viewport_point() {
    let (page_doc, page_layout) = laid_out(
        r#"<html><body style="margin:0">
             <iframe src="c.html" width="300" height="200"
                     style="border:0;position:absolute;left:40px;top:120px"></iframe>
           </body></html>"#,
        Size::new(1024.0, 720.0),
    );
    let host = collect_iframes(&page_doc)[0].node;
    let host_rect = crate::frames::host_content_rect(
        crate::forms::find_layout_box(&page_layout, host).expect("бокс <iframe>"),
    );
    // Ребёнок — стопка из трёх полос по 100: клик в одну и ту же точку окна
    // обязан менять адресата вместе с прокруткой.
    let (child_doc, child_layout) = laid_out(
        r#"<html><body style="margin:0">
             <div id="a" style="height:100px;background:#f00"></div>
             <div id="b" style="height:100px;background:#0f0"></div>
             <div id="c" style="height:100px;background:#00f"></div>
           </body></html>"#,
        Size::new(host_rect.width, host_rect.height),
    );
    let (a, c) = (
        child_doc.find_by_id("a").expect("полоса A"),
        child_doc.find_by_id("c").expect("полоса C"),
    );
    let mut handle = splice_handle("c.html", host_rect, Vec::new());
    handle.host = host;
    handle.doc = Arc::new(Mutex::new(child_doc));
    handle.layout = Some(child_layout);

    // Точка (40+60, 120+30) — 30 пикселей вглубь содержимого фрейма.
    let point = Point::new(100.0, 150.0);
    let mut frames = vec![handle];
    let t = crate::frames::pointer_target(&frames, &page_layout, point);
    let hit = t.frame.expect("точка внутри фрейма");
    assert_eq!(hit.hit.map(|h| h.node), Some(a), "без прокрутки — первая полоса");

    frames[0].scroll_y = 200.0;
    let t = crate::frames::pointer_target(&frames, &page_layout, point);
    let hit = t.frame.expect("точка внутри фрейма");
    assert_eq!(
        hit.hit.map(|h| h.node),
        Some(c),
        "после прокрутки на 200 под той же точкой окна — третья полоса"
    );
    assert!(
        (hit.client.y - 30.0).abs() < 0.5,
        "координаты события остаются вьюпортными: {:?}",
        hit.client
    );
}

/// Пересчёт вьюпортов зажимает прокрутку, оказавшуюся за новым пределом:
/// содержимое ребёнка могло стать ниже, а хост — выше.
#[test]
fn sync_frame_viewports_clamps_stale_scroll() {
    // Содержимое 600 при вьюпорте 200 — предел 400. Список непустой, поэтому
    // проход его не пересобирает, и предел в тесте настоящий, а не нулевой
    // (с нулём зажим «до 0» проходил бы и при сломанной арифметике).
    let tall = vec![lumen_paint::DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, 300.0, 600.0),
        color: lumen_layout::Color { r: 1, g: 2, b: 3, a: 255 },
    }];

    let (page_layout, mut handle, _) = page_with_live_frame();
    handle.content_dl = tall.clone();
    handle.scroll_y = 500.0;
    let mut frames = vec![handle];
    crate::frames::sync_frame_viewports(&mut frames, &page_layout);
    assert!(
        (frames[0].scroll_y - 400.0).abs() < 0.5,
        "прокрутка за пределом возвращается К КРАЮ содержимого, а не к нулю: {}",
        frames[0].scroll_y
    );

    let (page_layout, mut handle, _) = page_with_live_frame();
    handle.content_dl = tall;
    handle.scroll_y = 100.0;
    let mut frames = vec![handle];
    crate::frames::sync_frame_viewports(&mut frames, &page_layout);
    assert_eq!(frames[0].scroll_y, 100.0, "прокрутка в пределах — не трогаем");
}

// ── формы под-документа (BUG-480 срез 18) ───────────────────────────────────

/// Есть ли в списке синяя заливка — панель, которую прячет закрытый
/// `<details>` в фикстурах ниже.
fn has_blue(dl: &DisplayList) -> bool {
    dl.iter().any(|c| {
        matches!(c, lumen_paint::DisplayCommand::FillRect { color, .. }
                 if color.r == 0 && color.g == 0 && color.b == 255)
    })
}

/// Страница с `<iframe>` и живой хэндл ребёнка на произвольной разметке —
/// [`page_with_live_frame`] с параметром вместо фиксированной плашки.
fn live_frame_with_child(child_html: &str) -> (lumen_layout::LayoutBox, crate::frames::FrameHandle) {
    let (page_layout, mut handle, _) = page_with_live_frame();
    let host_rect = handle.host_rect.expect("хост-бокс задан хелпером");
    let (child_doc, child_layout) =
        laid_out(child_html, Size::new(host_rect.width, host_rect.height));
    handle.doc = Arc::new(Mutex::new(child_doc));
    handle.layout = Some(child_layout);
    (page_layout, handle)
}

/// Разметка ребёнка с `<details>`: закрытый прячет синюю панель, открытый — нет.
const CHILD_WITH_DETAILS: &str = r#"<html><body style="margin:0">
     <details id="d"><summary>s</summary>
       <div style="height:60px;background:rgb(0,0,255)"></div>
     </details>
   </body></html>"#;

/// Пересчёт после мутации DOM ребёнка идёт при НЕИЗМЕННОМ вьюпорте.
///
/// Это и есть отличие среза 18 от 13: гейт «размер хоста не менялся — не
/// пересчитывать» в [`sync_frame_viewports`] здесь срабатывает всегда, и
/// правка дошла бы до атрибута, но не до экрана. Проверяется по СОДЕРЖИМОМУ
/// готового display list, а не по факту вызова: панель за закрытым
/// `<details>` не рисуется, за открытым — рисуется.
#[test]
fn relayout_frame_content_repaints_child_at_unchanged_viewport() {
    let (page_layout, handle) = live_frame_with_child(CHILD_WITH_DETAILS);
    let viewport_before = handle.viewport;
    let mut frames = vec![handle];
    crate::frames::sync_frame_viewports(&mut frames, &page_layout);
    assert!(
        !has_blue(&frames[0].content_dl),
        "закрытый <details> не рисует панель — иначе тест ниже ничего не докажет"
    );

    let d = {
        let doc = frames[0].doc.lock().expect("лок ребёнка");
        doc.find_by_id("d").expect("<details>")
    };
    {
        let mut doc = frames[0].doc.lock().expect("лок ребёнка");
        crate::forms::toggle_details_open(&mut doc, d);
    }
    // Повторный проход по вьюпортам правку НЕ увидит: размер хоста тот же.
    crate::frames::sync_frame_viewports(&mut frames, &page_layout);
    assert!(
        !has_blue(&frames[0].content_dl),
        "мутация дерева ребёнка мимо гейта размеров — ровно тот дефект, который чинит срез"
    );

    crate::frames::relayout_frame_content(&mut frames, 0, &page_layout);
    assert!(has_blue(&frames[0].content_dl), "раскрытая панель обязана попасть в список");
    assert_eq!(
        frames[0].viewport, viewport_before,
        "вьюпорт ребёнка мутация его DOM не меняет"
    );
}

/// Правка внутри ВЛОЖЕННОГО фрейма доходит до списка его предка.
///
/// Содержимое внука вклеено в список среднего фрейма, а тот — в список
/// страницы: перерисовать только внука значит не показать ничего. Порядок
/// обхода по глубине живёт в [`sync_frame_viewports`], поэтому срез им и
/// пользуется вместо собственного прохода.
#[test]
fn relayout_frame_content_reaches_the_ancestor_list() {
    // Средний фрейм сам держит `<iframe src="g.html">` — по его заглушке
    // вклейка и найдёт содержимое внука.
    let (page_layout, mid) = live_frame_with_child(
        r#"<html><body style="margin:0">
             <iframe src="g.html" width="200" height="100" style="border:0"></iframe>
           </body></html>"#,
    );
    let mid_doc = Arc::clone(&mid.doc);
    let grand_host = {
        let doc = mid_doc.lock().expect("лок среднего");
        collect_iframes(&doc)[0].node
    };
    let grand_rect = crate::frames::host_content_rect(
        crate::forms::find_layout_box(mid.layout.as_ref().expect("layout среднего"), grand_host)
            .expect("бокс <iframe> внука"),
    );

    let (_, grand_doc_layout) = live_frame_with_child(CHILD_WITH_DETAILS);
    let mut grand = grand_doc_layout;
    grand.host = grand_host;
    grand.host_src = "g.html".to_owned();
    grand.host_rect = Some(grand_rect);
    grand.viewport = Size::new(grand_rect.width, grand_rect.height);
    grand.depth = 1;
    grand.parent_doc = Some(Arc::clone(&mid_doc));
    // Layout внука — на СВОЁМ вьюпорте, а не на вьюпорте среднего фрейма.
    let (gd, gl) = laid_out(CHILD_WITH_DETAILS, grand.viewport);
    grand.doc = Arc::new(Mutex::new(gd));
    grand.layout = Some(gl);

    let mut frames = vec![mid, grand];
    crate::frames::sync_frame_viewports(&mut frames, &page_layout);
    assert!(!has_blue(&frames[0].content_dl), "панель внука пока спрятана");

    let d = {
        let doc = frames[1].doc.lock().expect("лок внука");
        doc.find_by_id("d").expect("<details>")
    };
    {
        let mut doc = frames[1].doc.lock().expect("лок внука");
        crate::forms::toggle_details_open(&mut doc, d);
    }
    crate::frames::relayout_frame_content(&mut frames, 1, &page_layout);
    assert!(has_blue(&frames[1].content_dl), "внук перерисован");
    assert!(
        has_blue(&frames[0].content_dl),
        "и его содержимое доехало до списка СРЕДНЕГО фрейма — иначе на экране пусто"
    );
}

/// Мутация, укоротившая содержимое, зажимает прокрутку ребёнка.
///
/// Тот же долг, что у среза 17 при смене размера хоста, но приходит с другой
/// стороны: закрыть `<details>` можно на прокрученном до низа фрейме, и без
/// зажима под ним осталась бы пустота.
#[test]
fn relayout_frame_content_clamps_scroll_when_child_shrinks() {
    let tall = r#"<html><body style="margin:0">
         <details id="d" open><summary>s</summary>
           <div style="height:600px;background:rgb(0,0,255)"></div>
         </details>
       </body></html>"#;
    let (page_layout, handle) = live_frame_with_child(tall);
    let mut frames = vec![handle];
    crate::frames::sync_frame_viewports(&mut frames, &page_layout);
    let max = crate::frames::frame_max_scroll(&frames[0]);
    assert!(max > 100.0, "раскрытый <details> обязан давать прокрутку: {max}");
    assert_eq!(crate::frames::scroll_frame_to(&mut frames, 0, max), Some(max));

    let d = {
        let doc = frames[0].doc.lock().expect("лок ребёнка");
        doc.find_by_id("d").expect("<details>")
    };
    {
        let mut doc = frames[0].doc.lock().expect("лок ребёнка");
        crate::forms::toggle_details_open(&mut doc, d);
    }
    crate::frames::relayout_frame_content(&mut frames, 0, &page_layout);
    assert_eq!(
        frames[0].scroll_y,
        crate::frames::frame_max_scroll(&frames[0]),
        "прокрутка вернулась к новому краю содержимого"
    );
    assert!(frames[0].scroll_y < max, "новый край выше прежнего: {}", frames[0].scroll_y);
}

// в”Ђв”Ђ PH1-2: Progressive streaming pipeline в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

// Compile-time: streaming throttle must be в‰¤16 ms (~60 Hz).
// Prevents accidental reversion to the old 150 ms value.
const _: () = assert!(STREAM_PAINT_INTERVAL_MS <= 16);
const _: () = assert!(STREAM_PAINT_INTERVAL_MS >= 14);

// в”Ђв”Ђ extract_tor_mode в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn tor_mode_not_present() {
    let (port, rest) = extract_tor_mode(&args(&["page.html"]));
    assert!(port.is_none());
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn tor_mode_basic() {
    let (port, rest) = extract_tor_mode(&args(&["--tor", "page.html"]));
    assert_eq!(port, Some(9050));
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn tor_mode_custom_port() {
    let (port, rest) = extract_tor_mode(&args(&["--tor", "--tor-port", "9150", "page.html"]));
    assert_eq!(port, Some(9150));
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn tor_mode_port_before_tor_flag() {
    // --tor-port before --tor is consumed regardless of order.
    let (port, rest) = extract_tor_mode(&args(&["--tor-port", "9150", "--tor"]));
    assert_eq!(port, Some(9150));
    assert!(rest.is_empty());
}

#[test]
fn tor_mode_no_flag_no_extra_port() {
    // --tor-port without --tor в†’ tor_found=false в†’ return None (port consumed but no tor).
    let (port, rest) = extract_tor_mode(&args(&["--tor-port", "9150", "page.html"]));
    assert!(port.is_none());
    assert_eq!(rest, args(&["page.html"]));
}

#[test]
fn tor_mode_empty_args() {
    let (port, rest) = extract_tor_mode(&[]);
    assert!(port.is_none());
    assert!(rest.is_empty());
}

// в”Ђв”Ђ PH1-2c: РїСЂРѕРіСЂРµСЃСЃРёРІРЅР°СЏ РїРѕРґРіСЂСѓР·РєР° РєР°СЂС‚РёРЅРѕРє РІРѕ РІСЂРµРјСЏ streaming в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn resource_base_maps_url_and_file_sources() {
    assert!(matches!(
        PageSource::Url("https://example.com/".to_owned()).resource_base(),
        Some(ResourceBase::Url(_))
    ));
    assert!(matches!(
        PageSource::File(PathBuf::from("/tmp/page.html")).resource_base(),
        Some(ResourceBase::File(_))
    ));
    assert!(matches!(
        PageSource::Snapshot { html: String::new(), base_url: "https://x/".to_owned() }
            .resource_base(),
        Some(ResourceBase::Url(_))
    ));
}

#[test]
fn resource_base_none_for_baseless_sources() {
    // РСЃС‚РѕС‡РЅРёРєРё Р±РµР· Р±Р°Р·С‹ РЅРµ РґРѕР»Р¶РЅС‹ СЃРїР°РІРЅРёС‚СЊ streaming-Р·Р°РіСЂСѓР·РєСѓ РєР°СЂС‚РёРЅРѕРє.
    assert!(PageSource::Empty.resource_base().is_none());
    assert!(PageSource::AboutBlank.resource_base().is_none());
    assert!(
        PageSource::Static { html: String::new(), url: "x".to_owned() }
            .resource_base()
            .is_none()
    );
}

#[test]
fn stream_image_discovery_dedups_and_skips_lazy() {
    // Р’РѕСЃРїСЂРѕРёР·РІРѕРґРёС‚ С„РёР»СЊС‚СЂР°С†РёСЋ РёР· spawn_stream_image_loads: lazy РїСЂРѕРїСѓСЃРєР°РµРј,
    // РїРѕРІС‚РѕСЂРЅРѕ РІСЃС‚СЂРµС‡РµРЅРЅС‹Р№ src РЅРµ Р·Р°РїСЂР°С€РёРІР°РµРј РґРІР°Р¶РґС‹ РјРµР¶РґСѓ РєР°РґСЂР°РјРё.
    let html = r#"
            <img src="a.png">
            <img src="b.png" loading="lazy">
            <img src="a.png">
        "#;
    let doc = lumen_html_parser::parse(html);
    let requests = lumen_layout::collect_image_requests(&doc, Size::new(300.0, 300.0));

    let mut requested: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut dispatched: Vec<String> = Vec::new();
    for req in requests {
        if req.is_lazy {
            continue;
        }
        if requested.insert(req.url.clone()) {
            dispatched.push(req.url);
        }
    }
    assert_eq!(dispatched, vec!["a.png".to_owned()], "lazy РїСЂРѕРїСѓС‰РµРЅ, РґСѓР±Р»СЊ a.png СЃС…Р»РѕРїРЅСѓС‚");
}

#[test]
fn post_load_image_discovery_dispatches_only_the_new_src() {
    // BUG-730: РІС‚РѕСЂРѕР№ РїСЂРѕС…РѕРґ (`spawn_dynamic_image_loads` РїРѕСЃР»Рµ СЂРµР»РµР№Р°СѓС‚Р°
    // JS-РјСѓС‚Р°С†РёРё) РІРёРґРёС‚ РІРµСЃСЊ РґРѕРєСѓРјРµРЅС‚ С†РµР»РёРєРѕРј, Р° РЅРµ С‚РѕР»СЊРєРѕ РґРѕРµС…Р°РІС€СѓСЋ РїРѕ СЃРµС‚Рё
    // СЂР°Р·РјРµС‚РєСѓ. Р”РµРґСѓРї С‡РµСЂРµР· С‚РѕС‚ Р¶Рµ `stream_images_requested` РѕР±СЏР·Р°РЅ РїСЂРѕРїСѓСЃС‚РёС‚СЊ
    // СѓР¶Рµ Р·Р°РіСЂСѓР¶РµРЅРЅРѕРµ Рё РІС‹РґР°С‚СЊ СЂРѕРІРЅРѕ РїРѕСЏРІРёРІС€РёР№СЃСЏ `<img>` вЂ” РёРЅР°С‡Рµ РєР°Р¶РґС‹Р№
    // СЂРµР»РµР№Р°СѓС‚ РїРµСЂРµРєР°С‡РёРІР°Р» Р±С‹ РІСЃСЋ СЃС‚СЂР°РЅРёС†Сѓ Р·Р°РЅРѕРІРѕ.
    let vp = Size::new(300.0, 300.0);
    let mut requested: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut dispatch = |html: &str| -> Vec<String> {
        let doc = lumen_html_parser::parse(html);
        lumen_layout::collect_image_requests(&doc, vp)
            .into_iter()
            .filter(|r| !r.is_lazy)
            .filter(|r| requested.insert(r.url.clone()))
            .map(|r| r.url)
            .collect()
    };

    assert_eq!(dispatch(r#"<img src="a.png">"#), vec!["a.png".to_owned()]);
    // РЎРєСЂРёРїС‚ РґРѕРїРёСЃР°Р» РІС‚РѕСЂСѓСЋ РєР°СЂС‚РёРЅРєСѓ вЂ” РїСЂРёРµС…Р°Р»Р° С‚РѕР»СЊРєРѕ РѕРЅР°.
    assert_eq!(
        dispatch(r#"<img src="a.png"><img src="b.png">"#),
        vec!["b.png".to_owned()],
        "a.png СѓР¶Рµ Р·Р°РїСЂРѕС€РµРЅР°, РїРѕРІС‚РѕСЂРЅРѕ РЅРµ СѓС…РѕРґРёС‚"
    );
    // РќРёС‡РµРіРѕ РЅРµ РјРµРЅСЏР»РѕСЃСЊ вЂ” РЅРё РѕРґРЅРѕРіРѕ Р·Р°РїСЂРѕСЃР°.
    assert!(dispatch(r#"<img src="a.png"><img src="b.png">"#).is_empty());
}
