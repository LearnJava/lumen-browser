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

// ── host_content_size (BUG-480 срез 13) ──────────────────────────────────────

/// Вьюпорт под-документа — КОНТЕНТНЫЙ бокс хоста, а не его `rect`
/// (border-бокс): `<iframe width=400 height=200>` с рамкой и padding должен
/// дать ребёнку ровно 400×200, а не 400+10+6.
///
/// Второй ассерт на `rect` не декоративный: без него тест прошёл бы и в том
/// случае, если бы вычитание рамок вовсе не выполнялось, а атрибуты
/// резолвились как border-бокс.
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

    let size = crate::frames::host_content_size(host);
    assert!(
        (size.width - 400.0).abs() < 0.5 && (size.height - 200.0).abs() < 0.5,
        "контентный бокс = атрибуты width/height, получено {size:?}"
    );
    assert!(
        host.rect.width > 410.0 && host.rect.height > 210.0,
        "border-бокс шире контентного на padding 3+3 и рамку 5+5: {:?}",
        host.rect
    );
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
