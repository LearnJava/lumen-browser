use super::*;
use crate::BrowserSession;

fn make_session(html: &str) -> InProcessSession {
    let mut s = InProcessSession::new();
    let bytes = html.as_bytes().to_vec();
    s.run_pipeline(&bytes, Some("text/html"), "file://test".into())
        .expect("pipeline не запустился");
    s
}

#[test]
fn navigate_file_url_with_drive_letter_slash() {
    // BUG-760: `file:///<abs>` не должен оставлять ведущий `/` перед буквой диска.
    let dir = std::env::temp_dir().join("lumen_bug760");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("p.html");
    std::fs::write(&f, r#"<html><body><div id="a">x</div></body></html>"#).unwrap();
    let p = f.display().to_string().replace('\\', "/");
    let url = if p.starts_with('/') { format!("file://{p}") } else { format!("file:///{p}") };
    let mut s = InProcessSession::new();
    s.navigate(&url).expect("file:// URL должен грузиться");
    assert_eq!(s.current_url(), url);
}

#[test]
fn navigate_about_blank_headless() {
    let mut s = InProcessSession::new();
    s.navigate("about:blank").expect("about:blank — штатный документ");
    assert_eq!(s.current_url(), "about:blank");
}

#[test]
fn build_http_client_wires_hsts() {
    // BUG-402: драйвер собирал HttpClient мимо HSTS — автоматизация и
    // headless-навигация ходили по http без апгрейда даже на preload-хосты.
    // Профиль Tor выбран, чтобы общий store процесса был in-memory и тест
    // не создавал файл БД рядом с тестовым бинарником.
    let mut s = InProcessSession::new();
    s.set_fingerprint_profile(FingerprintProfile::Tor)
        .expect("профиль ставится");
    assert!(
        s.build_http_client().has_hsts(),
        "клиент драйвера обязан нести общий HSTS-store"
    );
}

#[test]
fn navigate_local_html_produces_layout() {
    let mut s = InProcessSession::new();
    // Inline HTML через run_pipeline напрямую (нет реального файла).
    let html = r#"<!DOCTYPE html><html><body><div id="box" style="width:100px;height:50px;background:red"></div></body></html>"#;
    s.run_pipeline(html.as_bytes(), Some("text/html"), "file://test".into())
        .expect("pipeline");
    let boxes = s.layout_snapshot().expect("layout_snapshot");
    assert!(!boxes.is_empty());
}

#[test]
fn query_by_tag_returns_nodes() {
    let s = make_session("<html><body><p>один</p><p>два</p></body></html>");
    let nodes = s.query("p").expect("query");
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].tag_name, "p");
}

#[test]
fn query_by_id_returns_single_node() {
    let s = make_session(r#"<html><body><div id="hero">H</div><div id="other">O</div></body></html>"#);
    let nodes = s.query("#hero").expect("query");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].node_id, nodes[0].node_id); // sanity
}

#[test]
fn query_by_class_filters_correctly() {
    let s = make_session(r#"<html><body><span class="red big">A</span><span class="blue">B</span><span class="red small">C</span></body></html>"#);
    let nodes = s.query(".red").expect("query .red");
    assert_eq!(nodes.len(), 2);
}

#[test]
fn computed_style_returns_properties() {
    let s = make_session(r#"<html><body><div id="x" style="font-size:24px"></div></body></html>"#);
    let props = s.computed_style("#x").expect("computed_style");
    assert!(props.is_some());
}

#[test]
fn a11y_tree_has_role() {
    let s = make_session("<html><body><button>OK</button></body></html>");
    let tree = s.a11y_tree().expect("a11y_tree");
    fn has_role(node: &A11yNode, role: &str) -> bool {
        if node.role == role {
            return true;
        }
        node.children.iter().any(|c| has_role(c, role))
    }
    assert!(has_role(&tree, "button"), "button роль не найдена");
}

#[test]
fn screenshot_returns_png() {
    let s = make_session("<html><body><div style='background:red; width:100px; height:100px;'></div></body></html>");
    let png_bytes = s.screenshot().expect("screenshot should succeed");
    // PNG signature: 89 50 4E 47 0D 0A 1A 0A
    assert!(png_bytes.len() > 8, "PNG should have content");
    assert_eq!(&png_bytes[0..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A], "PNG signature");
}

#[test]
fn current_url_after_navigate() {
    let mut s = InProcessSession::new();
    s.run_pipeline(b"<html></html>", None, "file:///my/page.html".into())
        .expect("pipeline");
    assert_eq!(s.current_url(), "file:///my/page.html");
}

#[test]
fn parse_simple_selector_tag_only() {
    let sel = parse_simple_selector("div");
    assert_eq!(sel.tag, Some("div"));
    assert!(sel.id.is_none());
    assert!(sel.classes.is_empty());
}

#[test]
fn parse_simple_selector_id() {
    let sel = parse_simple_selector("#hero");
    assert!(sel.tag.is_none());
    assert_eq!(sel.id, Some("hero"));
}

#[test]
fn parse_simple_selector_class() {
    let sel = parse_simple_selector(".red");
    assert!(sel.tag.is_none());
    assert_eq!(sel.classes, vec!["red"]);
}

#[test]
fn parse_simple_selector_tag_and_class() {
    let sel = parse_simple_selector("span.red.big");
    assert_eq!(sel.tag, Some("span"));
    assert_eq!(sel.classes, vec!["red", "big"]);
}

#[test]
fn wait_document_ready_succeeds_after_navigate() {
    let mut s = InProcessSession::new();
    s.run_pipeline(b"<html><body>text</body></html>", Some("text/html"), "file:///test".into())
        .expect("pipeline");
    // DocumentReady должен быть успешен сразу после navigate
    s.wait(WaitCondition::DocumentReady, 1000)
        .expect("wait DocumentReady");
}

#[test]
fn wait_visible_element_succeeds() {
    let mut s = make_session(r#"<html><body><div id="box" style="width:100px;height:50px"></div></body></html>"#);
    // Элемент существует в layout, поэтому Visible должна быть true
    s.wait(WaitCondition::Visible("#box".into()), 1000)
        .expect("wait Visible");
}

#[test]
fn wait_visible_nonexistent_element_times_out() {
    let mut s = make_session(r#"<html><body><div id="box"></div></body></html>"#);
    // Элемента с id="missing" нет, поэтому timeout
    let result = s.wait(WaitCondition::Visible("#missing".into()), 100);
    assert!(result.is_err(), "wait должен вернуть timeout");
    assert!(result.unwrap_err().to_string().contains("timeout"));
}

#[test]
fn wait_stable_element_succeeds() {
    let mut s = make_session(r#"<html><body><span class="text">Hello</span></body></html>"#);
    // Layout стабилен с самого начала в headless
    s.wait(WaitCondition::Stable(".text".into()), 1000)
        .expect("wait Stable");
}

#[test]
fn wait_network_idle_succeeds() {
    let mut s = make_session("<html><body>test</body></html>");
    // Network всегда idle в headless (нет async network)
    s.wait(WaitCondition::NetworkIdle, 1000)
        .expect("wait NetworkIdle");
}

#[test]
fn wait_js_idle_succeeds() {
    let mut s = make_session("<html><body>test</body></html>");
    // JS всегда idle в Phase 1 headless (нет JS engine)
    s.wait(WaitCondition::JsIdle, 1000)
        .expect("wait JsIdle");
}

// ── 8D: Auto-wait tests ─────────────────────────────────────────────────

#[test]
fn wait_visible_with_dimensions_succeeds() {
    let mut s = make_session(
        r#"<html><body><div id="box" style="width:80px;height:40px;background:blue"></div></body></html>"#,
    );
    s.wait(WaitCondition::Visible("#box".into()), 1000)
        .expect("visible element with explicit size should be found");
}

#[test]
fn wait_visible_zero_height_times_out() {
    // div without content has height=0 — not considered visible (8D.1).
    let mut s = make_session(r#"<html><body><div id="empty"></div></body></html>"#);
    let result = s.wait(WaitCondition::Visible("#empty".into()), 100);
    assert!(result.is_err(), "zero-height element must not satisfy Visible");
    assert!(result.unwrap_err().to_string().contains("timeout"));
}

#[test]
fn wait_network_idle_immediately_after_file_navigate() {
    // File navigations don't touch active_network_requests → always 0.
    let mut s = make_session("<html><body>hi</body></html>");
    assert_eq!(s.active_network_requests, 0);
    s.wait(WaitCondition::NetworkIdle, 100)
        .expect("NetworkIdle must be true after file-based navigate");
}

#[test]
fn wait_js_idle_false_when_tasks_pending() {
    let mut s = make_session("<html><body>test</body></html>");
    s.set_pending_js_tasks(5);
    let result = s.wait(WaitCondition::JsIdle, 100);
    assert!(result.is_err(), "JsIdle must timeout when pending tasks > 0");
    // Clear tasks → idle.
    s.set_pending_js_tasks(0);
    s.wait(WaitCondition::JsIdle, 100)
        .expect("JsIdle must succeed after clearing pending tasks");
}

#[test]
fn set_pending_js_tasks_updates_counter() {
    let mut s = InProcessSession::new();
    assert_eq!(s.pending_js_microtasks, 0);
    s.set_pending_js_tasks(3);
    assert_eq!(s.pending_js_microtasks, 3);
    s.set_pending_js_tasks(0);
    assert_eq!(s.pending_js_microtasks, 0);
}

// ── Тесты для core::ext::BrowserSession adapter ────────────────────────

#[test]
fn core_session_navigate() {
    let s = make_session("<html><body><h1>Hello</h1></body></html>");
    // Already navigated via make_session, just verify the URL is set
    assert!(!s.current_url().is_empty());
}

#[test]
fn core_session_screenshot() {
    let s = make_session("<html><body><div style='background:blue; width:50px; height:50px;'></div></body></html>");
    let png_bytes = lumen_core::ext::BrowserSession::screenshot(&s).expect("screenshot");
    assert!(png_bytes.len() > 8);
    assert_eq!(&png_bytes[0..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
}

#[test]
fn core_session_a11y_tree_json() {
    let s = make_session("<html><body><button>Click</button></body></html>");
    let json_str = lumen_core::ext::BrowserSession::a11y_tree(&s).expect("a11y_tree");
    // Verify it's valid JSON
    serde_json::from_str::<serde_json::Value>(&json_str)
        .expect("a11y_tree should be valid JSON");
}

#[test]
fn core_session_computed_style_json() {
    let s = make_session(r#"<html><body><div id="x" style="color:red;font-size:20px"></div></body></html>"#);
    let json_str = lumen_core::ext::BrowserSession::computed_style(&s, "#x").expect("computed_style");
    let obj: serde_json::Value = serde_json::from_str(&json_str)
        .expect("computed_style should be valid JSON");
    // Verify we have a JSON object with properties
    assert!(obj.is_object());
}

#[test]
fn computed_style_json_full_map() {
    let s = make_session(r#"<html><body><div id="x" style="color:red;font-size:20px"></div></body></html>"#);
    let json_str = s.computed_style_json("#x").expect("computed_style_json");
    let obj: serde_json::Value =
        serde_json::from_str(&json_str).expect("valid JSON object");
    let map = obj.as_object().expect("object");
    // Full map covers far more than the ~13-property BrowserSession::computed_style.
    assert!(map.len() > 30, "expected full property map, got {}", map.len());
    assert_eq!(map["color"], "rgb(255, 0, 0)");
    assert_eq!(map["font-size"], "20px");
    assert_eq!(map["display"], "block");
}

#[test]
fn computed_style_json_missing_selector_errors() {
    let s = make_session(r#"<html><body><div></div></body></html>"#);
    assert!(s.computed_style_json("#nope").is_err());
}

#[test]
fn core_session_viewport() {
    let s = make_session("<html><body></body></html>");
    let (w, h) = lumen_core::ext::BrowserSession::viewport(&s);
    assert_eq!(w, 1024);
    assert_eq!(h, 720);
}

#[test]
fn core_session_set_viewport() {
    let mut s = make_session("<html><body></body></html>");
    lumen_core::ext::BrowserSession::set_viewport(&mut s, 800, 600).expect("set_viewport");
    let (w, h) = lumen_core::ext::BrowserSession::viewport(&s);
    assert_eq!(w, 800);
    assert_eq!(h, 600);
}

// ── 9F.2: per-context fingerprint profile applied to the HTTP client ──────

#[test]
fn http_client_reflects_fingerprint_profile() {
    let mut s = InProcessSession::new();
    // Default (Standard) → Chrome HTTP + Standard TLS.
    let c = s.build_http_client();
    assert_eq!(c.fingerprint_profile(), lumen_network::HttpProfile::Chrome);
    assert_eq!(c.tls_profile(), lumen_network::TlsProfile::Standard);

    // Strict → Strict HTTP + Strict TLS (TLS profile is derived).
    BrowserSession::set_fingerprint_profile(&mut s, FingerprintProfile::Strict).unwrap();
    let c = s.build_http_client();
    assert_eq!(c.fingerprint_profile(), lumen_network::HttpProfile::Strict);
    assert_eq!(c.tls_profile(), lumen_network::TlsProfile::Strict);

    // Tor → TorBrowser HTTP + Tor TLS.
    BrowserSession::set_fingerprint_profile(&mut s, FingerprintProfile::Tor).unwrap();
    let c = s.build_http_client();
    assert_eq!(c.fingerprint_profile(), lumen_network::HttpProfile::TorBrowser);
    assert_eq!(c.tls_profile(), lumen_network::TlsProfile::Tor);
}

#[test]
fn frozen_fingerprint_keeps_client_profile() {
    let mut s = InProcessSession::new();
    BrowserSession::set_fingerprint_profile(&mut s, FingerprintProfile::Strict).unwrap();
    s.context.freeze_fingerprint();
    // A change after freeze is rejected; the built client keeps Strict.
    assert!(BrowserSession::set_fingerprint_profile(&mut s, FingerprintProfile::Standard).is_err());
    assert_eq!(
        s.build_http_client().fingerprint_profile(),
        lumen_network::HttpProfile::Strict
    );
}

#[test]
fn set_clock_frozen_stores_timestamp() {
    let mut sess = InProcessSession::new();
    lumen_core::ext::BrowserSession::set_clock(&mut sess, lumen_core::ClockMode::Frozen(1_700_000_000_000)).unwrap();
    assert_eq!(sess.context.frozen_clock_ms(), Some(1_700_000_000_000));
}

#[test]
fn set_clock_real_clears_timestamp() {
    let mut sess = InProcessSession::new();
    lumen_core::ext::BrowserSession::set_clock(&mut sess, lumen_core::ClockMode::Frozen(42)).unwrap();
    lumen_core::ext::BrowserSession::set_clock(&mut sess, lumen_core::ClockMode::Real).unwrap();
    assert_eq!(sess.context.frozen_clock_ms(), None);
}

#[test]
fn set_rng_seed_stores_and_clears() {
    let mut sess = InProcessSession::new();
    lumen_core::ext::BrowserSession::set_rng_seed(&mut sess, Some(12345)).unwrap();
    assert_eq!(sess.context.rng_seed(), Some(12345));
    lumen_core::ext::BrowserSession::set_rng_seed(&mut sess, None).unwrap();
    assert_eq!(sess.context.rng_seed(), None);
}

// ── Deterministic mode (8F) driver trait tests ────────────────────────────

#[test]
fn driver_set_clock_frozen() {
    let mut sess = InProcessSession::new();
    BrowserSession::set_clock(&mut sess, crate::ClockMode::Frozen(999_000)).unwrap();
    assert_eq!(sess.context.frozen_clock_ms(), Some(999_000));
}

#[test]
fn driver_set_clock_real_clears() {
    let mut sess = InProcessSession::new();
    BrowserSession::set_clock(&mut sess, crate::ClockMode::Frozen(1)).unwrap();
    BrowserSession::set_clock(&mut sess, crate::ClockMode::Real).unwrap();
    assert_eq!(sess.context.frozen_clock_ms(), None);
}

#[test]
fn driver_set_clock_monotonic_advances() {
    use lumen_core::ext::ClockMode;
    let mut sess = InProcessSession::new();
    BrowserSession::set_clock(&mut sess, ClockMode::Monotonic { step_ms: 10 }).unwrap();
    // First read returns 0.
    assert_eq!(sess.context.read_clock_ms(), Some(0));
    // Second read advances by step_ms.
    assert_eq!(sess.context.read_clock_ms(), Some(10));
    assert_eq!(sess.context.read_clock_ms(), Some(20));
}

#[test]
fn driver_set_rng_seed() {
    let mut sess = InProcessSession::new();
    BrowserSession::set_rng_seed(&mut sess, Some(42)).unwrap();
    assert_eq!(sess.context.rng_seed(), Some(42));
    BrowserSession::set_rng_seed(&mut sess, None).unwrap();
    assert_eq!(sess.context.rng_seed(), None);
}

#[test]
fn driver_freeze_fingerprint_locks_profile() {
    let mut sess = InProcessSession::new();
    BrowserSession::freeze_fingerprint(&mut sess, FingerprintProfile::Strict).unwrap();
    assert_eq!(sess.fingerprint_profile(), FingerprintProfile::Strict);
    // Profile is now locked — change must fail.
    assert!(BrowserSession::set_fingerprint_profile(&mut sess, FingerprintProfile::Standard).is_err());
}

#[test]
fn driver_freeze_fingerprint_standard_profile() {
    let mut sess = InProcessSession::new();
    BrowserSession::freeze_fingerprint(&mut sess, FingerprintProfile::Standard).unwrap();
    assert_eq!(sess.fingerprint_profile(), FingerprintProfile::Standard);
    assert!(sess.context.is_fingerprint_frozen());
}

#[test]
fn deterministic_config_apply() {
    use crate::determinism::DeterministicConfig;
    let mut sess = InProcessSession::new();
    let cfg = DeterministicConfig {
        clock: crate::ClockMode::Frozen(1234),
        rng_seed: Some(99),
        freeze_fingerprint: Some(FingerprintProfile::Strict),
    };
    cfg.apply(&mut sess).unwrap();
    assert_eq!(sess.context.frozen_clock_ms(), Some(1234));
    assert_eq!(sess.context.rng_seed(), Some(99));
    assert_eq!(sess.fingerprint_profile(), FingerprintProfile::Strict);
    assert!(sess.context.is_fingerprint_frozen());
}

// ── DEVX-5 slice 2: click/type_text/eval via persistent V8 runtime ──────

#[test]
#[cfg(feature = "v8")]
fn eval_returns_json_value() {
    let mut s = make_session("<html><body></body></html>");
    assert_eq!(s.eval("1 + 1").expect("eval"), "2");
}

#[test]
#[cfg(feature = "v8")]
fn eval_without_navigate_errors() {
    let mut s = InProcessSession::new();
    let err = s.eval("1").expect_err("eval before navigate must fail");
    assert!(err.to_string().contains("V8"), "unexpected error: {err}");
}

#[test]
#[cfg(feature = "v8")]
fn eval_can_read_dom_via_shim() {
    let mut s = make_session(r#"<html><body><div id="x">hi</div></body></html>"#);
    assert_eq!(
        s.eval("document.getElementById('x').textContent").expect("eval"),
        "\"hi\""
    );
}

#[test]
#[cfg(feature = "v8")]
fn click_dispatches_mouse_events_to_listener() {
    let mut s = make_session(r#"<html><body><button id="btn">Go</button></body></html>"#);
    s.eval(
        "document.getElementById('btn').addEventListener('click', \
         function() { window.__clicks = (window.__clicks || 0) + 1; });",
    )
    .expect("register listener");
    s.click(&Target::Selector("#btn".into())).expect("click");
    assert_eq!(s.eval("window.__clicks").expect("eval"), "1");
}

#[test]
#[cfg(feature = "v8")]
fn click_point_hit_tests_layout_tree() {
    let mut s = make_session(
        r#"<html><body style="margin:0"><div id="box" style="width:200px;height:100px;"></div></body></html>"#,
    );
    s.eval(
        "document.getElementById('box').addEventListener('click', \
         function() { window.__hit = true; });",
    )
    .expect("register listener");
    s.click(&Target::Point { x: 50.0, y: 50.0 }).expect("click point");
    assert_eq!(s.eval("window.__hit").expect("eval"), "true");
}

#[test]
#[cfg(feature = "v8")]
fn click_point_outside_any_box_errors() {
    let mut s = make_session("<html><body></body></html>");
    let err = s
        .click(&Target::Point { x: 9999.0, y: 9999.0 })
        .expect_err("click on empty area must fail");
    assert!(err.to_string().contains("нет элемента"), "unexpected error: {err}");
}

#[test]
#[cfg(feature = "v8")]
fn type_text_sets_value_and_dispatches_input() {
    let mut s = make_session(r#"<html><body><input id="inp" type="text"></body></html>"#);
    s.eval(
        "document.getElementById('inp').addEventListener('input', \
         function() { window.__inputs = (window.__inputs || 0) + 1; });",
    )
    .expect("register listener");
    s.type_text(&Target::Selector("#inp".into()), "hi").expect("type_text");
    assert_eq!(s.eval("document.getElementById('inp').value").expect("eval"), "\"hi\"");
    // One 'input' event per dispatched character.
    assert_eq!(s.eval("window.__inputs").expect("eval"), "2");
}

#[test]
#[cfg(feature = "v8")]
fn type_text_rejects_non_typeable_target() {
    let mut s = make_session(r#"<html><body><div id="d">x</div></body></html>"#);
    let err = s
        .type_text(&Target::Selector("#d".into()), "hi")
        .expect_err("type_text on a div must fail");
    assert!(err.to_string().contains("не является"), "unexpected error: {err}");
}

// ── BUG-338: scroll(target) routes to a nested overflow ancestor ────────

fn nested_scroll_html() -> &'static str {
    r#"<html><body style="margin:0">
            <div id="outer" style="overflow:auto;width:200px;height:100px">
                <p id="leaf" style="margin-top:400px">deep content</p>
            </div>
        </body></html>"#
}

#[test]
fn scroll_with_selector_target_scrolls_nested_container_not_page() {
    let mut s = make_session(nested_scroll_html());
    s.scroll(&Target::Selector("#leaf".into()), ScrollDelta { x: 0.0, y: 50.0 })
        .expect("scroll");

    let containers = lumen_layout::collect_scroll_containers(&s.state().unwrap().layout_root);
    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0].scroll_y, 50.0, "#outer should have absorbed the scroll");

    // Page-level (compositor) scroll must stay untouched — the delta was
    // routed to the nested container, not the root.
    let root_offset = s.active_property_trees().and_then(|t| t.scroll.nodes.first().map(|n| n.offset_y));
    assert_eq!(root_offset, Some(0.0));
}

#[test]
fn scroll_offset_survives_relayout_after_eval() {
    // BUG-1215: eval() relayouts into a fresh tree; the offset must carry over.
    let mut s = make_session(nested_scroll_html());
    s.scroll(&Target::Selector("#leaf".into()), ScrollDelta { x: 0.0, y: 50.0 })
        .expect("scroll");
    s.eval("1").expect("eval");
    let containers = lumen_layout::collect_scroll_containers(&s.state().unwrap().layout_root);
    assert_eq!(containers[0].scroll_y, 50.0);
}

#[test]
fn scroll_with_selector_target_clamps_to_container_bounds() {
    let mut s = make_session(nested_scroll_html());
    s.scroll(&Target::Selector("#leaf".into()), ScrollDelta { x: 0.0, y: 10_000.0 })
        .expect("scroll");
    let containers = lumen_layout::collect_scroll_containers(&s.state().unwrap().layout_root);
    // Content height (400px margin + line) minus 100px viewport clamps the max.
    assert!(containers[0].scroll_y > 0.0);
    assert!(containers[0].scroll_y <= containers[0].scroll_height - containers[0].clip_rect.height);
}

#[test]
fn scroll_with_point_over_container_routes_there_too() {
    // A point over #outer's own scrollport routes there — mirrors real
    // wheel-event routing (`try_scroll_overflow_container`), not a bypass.
    let mut s = make_session(nested_scroll_html());
    s.scroll(&Target::Point { x: 5.0, y: 5.0 }, ScrollDelta { x: 0.0, y: 30.0 })
        .expect("scroll");
    let containers = lumen_layout::collect_scroll_containers(&s.state().unwrap().layout_root);
    assert_eq!(containers[0].scroll_y, 30.0);
}

#[test]
fn scroll_with_target_outside_any_container_falls_back_to_page() {
    // `body` has no scrolling overflow ancestor of its own — preserves the
    // pre-BUG-338 whole-page `scroll_page_by` behavior for such targets.
    let mut s = make_session(nested_scroll_html());
    s.scroll(&Target::Selector("body".into()), ScrollDelta { x: 0.0, y: 30.0 })
        .expect("scroll");
    let containers = lumen_layout::collect_scroll_containers(&s.state().unwrap().layout_root);
    assert_eq!(containers[0].scroll_y, 0.0, "#outer must stay untouched");
    let root_offset = s.active_property_trees().and_then(|t| t.scroll.nodes.first().map(|n| n.offset_y));
    assert_eq!(root_offset, Some(30.0), "delta should land on the page-level scroll node");
}

// ── BUG-965 (regression): headless JS scroll geometry reads real state ──
//
// Filed 2026-09-03 against `InProcessSession` (`--mcp-port`/`--mcp`):
// `scrollWidth` fell back to the border-box size instead of the real
// padding-box/overflow extent, and `scrollLeft`/`scrollTop` always read
// `0` regardless of a prior JS `scrollTo()`. Both are no longer
// reproducible — `FlushHandles::maybe_flush` (the shared lazy style/layout
// flush all `getComputedStyle`/geometry getters go through, including in
// this crate) now unconditionally recomputes `scroll_states` from a fresh
// layout on every `_lumen_get_scroll_state` call and reapplies any
// previously-known JS-side scroll offset onto it first. Kept as a
// regression guard rather than a fix.

#[test]
#[cfg(feature = "v8")]
fn js_scroll_width_reads_padding_box_not_border_box_headless() {
    // Mirrors the bug's own live repro: a childless `overflow:auto` box
    // with an asymmetric border (border-box would be 280, padding-box 200).
    let mut s = make_session(
        r#"<html><body style="margin:0">
                <div id="outer" style="overflow:auto;width:200px;height:200px;
                    border-style:solid;border-width:0 0 50px 80px"></div>
            </body></html>"#,
    );
    assert_eq!(s.eval("document.getElementById('outer').scrollWidth").expect("eval"), "200");
}

#[test]
#[cfg(feature = "v8")]
fn js_scroll_top_reflects_prior_js_scroll_to_headless() {
    let mut s = make_session(nested_scroll_html());
    s.eval("document.getElementById('outer').scrollTo(0, 50)").expect("eval scrollTo");
    assert_eq!(s.eval("document.getElementById('outer').scrollTop").expect("eval"), "50");
}
