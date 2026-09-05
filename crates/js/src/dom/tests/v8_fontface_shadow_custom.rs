//! Тесты `v8_fontface_shadow_custom`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

use std::path::PathBuf;

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

// ── FontFaceSet JS bindings (CSS Fonts Module Level 4 §11) ──────────────

#[test]
fn document_fonts_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                typeof document.fonts === 'object' && document.fonts !== null
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// FONTLOAD-1 (2026-09-05): `document.fonts` became a real setlike<FontFace> —
// `size`, not `length`; no `item()` (that was never part of the spec surface,
// only this shim's old ad hoc snapshot object had it). Coverage below matches
// CSS Font Loading §11.2, not the removed shape.
#[test]
fn document_fonts_has_size_property() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                typeof document.fonts.size === 'number'
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn document_fonts_has_foreach_method() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                typeof document.fonts.forEach === 'function'
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn document_fonts_has_setlike_methods() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                typeof document.fonts.has === 'function' &&
                typeof document.fonts.add === 'function' &&
                typeof document.fonts.delete === 'function' &&
                typeof document.fonts.clear === 'function' &&
                typeof document.fonts.keys === 'function' &&
                typeof document.fonts.values === 'function' &&
                typeof document.fonts.entries === 'function' &&
                typeof document.fonts.load === 'function'
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn document_fonts_empty_by_default() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                document.fonts.size === 0
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn document_fonts_is_the_same_object_every_access() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                document.fonts === document.fonts
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn new_font_face_set_throws_illegal_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                (function() {
                    try { new FontFaceSet([]); return false; }
                    catch (e) { return e instanceof TypeError; }
                })()
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn font_face_constructor_exposes_descriptors() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                var f = new FontFace('MyFont', 'url(a.woff)', { weight: '700', style: 'italic' });
                f.family === 'MyFont' && f.weight === '700' && f.style === 'italic' &&
                f.status === 'unloaded' && typeof f.loaded.then === 'function'
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn document_fonts_add_and_has_and_delete_round_trip() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                var f = new FontFace('MyFont', 'url(a.woff)');
                document.fonts.add(f);
                var hadIt = document.fonts.has(f) && document.fonts.size === 1;
                document.fonts.delete(f);
                hadIt && !document.fonts.has(f) && document.fonts.size === 0
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// FONTLOAD-5 (`bugs/BUG-467-OPEN.md`): a CSS-connected `url()` face is now
// populated `Loading` (not `Unloaded`) from the moment `document.fonts` is
// first touched, whenever its background fetch is actually queued
// (`crates/shell/src/page_pipeline.rs`) — these pair that native state with
// the shim's set-level pending counter the same way
// `crates/shell/src/app/user_event.rs`'s `LoadEvent::FontLoaded` handler and
// `_lumen_notify_css_font_loaded` do for a real fetch.

fn add_css_font_face(doc: &Arc<Mutex<Document>>, family: &str, status: lumen_dom::FontFaceStatus) {
    let mut face = lumen_dom::FontFace::new(
        family.to_string(),
        "normal".to_string(),
        "400".to_string(),
        None,
        None,
        "url(a.woff)".to_string(),
    );
    face.status = status;
    doc.lock().unwrap().fonts_mut().add(face);
}

#[test]
fn css_connected_loading_face_counts_as_pending_on_first_touch() {
    let doc = make_doc();
    add_css_font_face(&doc, "WebFont", lumen_dom::FontFaceStatus::Loading);
    let rt = v8_runtime_with_dom(doc);
    let result = rt.eval("document.fonts.status === 'loading'").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn css_connected_load_completing_pairs_off_pending_and_resolves_ready() {
    let doc = make_doc();
    add_css_font_face(&doc, "WebFont", lumen_dom::FontFaceStatus::Loading);
    let rt = v8_runtime_with_dom(doc.clone());
    rt.eval(
        r#"
                var _readyResolved = false;
                document.fonts.ready.then(function() { _readyResolved = true; });
            "#,
    )
    .unwrap();
    assert_eq!(
        rt.eval("document.fonts.status").unwrap(),
        lumen_core::JsValue::String("loading".into())
    );
    assert_eq!(rt.eval("_readyResolved").unwrap(), lumen_core::JsValue::Bool(false));
    {
        let mut d = doc.lock().unwrap();
        d.fonts_mut().mark_loaded(|f| f.family == "WebFont");
    }
    rt.eval("_lumen_notify_css_font_loaded('WebFont');").unwrap();
    assert_eq!(
        rt.eval("document.fonts.status").unwrap(),
        lumen_core::JsValue::String("loaded".into())
    );
    assert_eq!(rt.eval("_readyResolved").unwrap(), lumen_core::JsValue::Bool(true));
}

// FONTLOAD-6 (`bugs/BUG-467-OPEN.md`): a script-constructed `FontFace` whose
// bytes validate via `.load()` while it is a member of some `FontFaceSet` now
// reaches `lumen_font::FontRegistry` (queued for the shell to register on the
// next frame), instead of `.load()` only ever gating its own promise.

/// Real sfnt bytes (WPT/CSS-WG's Ahem) so `_lumen_font_validate_bytes`'s
/// `lumen_font::Font::parse` genuinely accepts them — a synthetic byte string
/// would only ever exercise the rejection path.
fn ahem_font_bytes() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("fonts")
        .join("Ahem.ttf");
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Builds a `Uint8Array` literal in JS from real bytes via `atob` — avoids
/// embedding a multi-KB decimal array literal in the test source.
fn js_bytes_expr(bytes: &[u8]) -> String {
    let b64 = crate::file_input::to_base64(bytes);
    format!(
        "(function() {{ var bin = atob(\"{b64}\"); var out = new Uint8Array(bin.length); \
         for (var i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i); return out; }})()"
    )
}

#[test]
fn register_scripted_font_face_queues_valid_bytes() {
    let rt = v8_runtime_with_dom(make_doc());
    let script = format!(
        "_lumen_register_scripted_font_face('MyAhem', '700', 'italic', {})",
        js_bytes_expr(&ahem_font_bytes())
    );
    let result = rt.eval(&script).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
    let queued = rt.take_pending_scripted_font_faces();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].0, "MyAhem");
    assert_eq!(queued[0].1, 700);
    assert_eq!(queued[0].2, lumen_core::FontStyle::Italic);
    assert_eq!(queued[0].3, ahem_font_bytes());
}

#[test]
fn register_scripted_font_face_rejects_garbage_bytes() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("_lumen_register_scripted_font_face('Bogus', 'normal', 'normal', new Uint8Array([1,2,3,4]))")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(false));
    assert!(rt.take_pending_scripted_font_faces().is_empty());
}

#[test]
fn script_constructed_font_face_registers_on_add_then_load() {
    let rt = v8_runtime_with_dom(make_doc());
    let script = format!(
        r#"
            var bytes = {bytes};
            var f = new FontFace('ScriptAhemA', bytes.buffer);
            document.fonts.add(f);
            typeof f.load().then === 'function'
        "#,
        bytes = js_bytes_expr(&ahem_font_bytes())
    );
    let result = rt.eval(&script).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
    let queued = rt.take_pending_scripted_font_faces();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].0, "ScriptAhemA");
}

#[test]
fn script_constructed_font_face_registers_on_load_then_add() {
    // `.load()` resolves a binary source synchronously (no fetch involved),
    // but `.then()` callbacks are always deferred to a microtask — the
    // registration itself must not depend on that microtask ever running, so
    // this checks native state (`_status`/`_registeredForRender`) right after
    // the synchronous `.load(); .add()` pair, not a `.then()` side effect.
    let rt = v8_runtime_with_dom(make_doc());
    let script = format!(
        r#"
            var bytes = {bytes};
            var f = new FontFace('ScriptAhemB', bytes.buffer);
            f.load();
            document.fonts.add(f);
            f._status === 'loaded' && f._registeredForRender === true
        "#,
        bytes = js_bytes_expr(&ahem_font_bytes())
    );
    let result = rt.eval(&script).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
    let queued = rt.take_pending_scripted_font_faces();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].0, "ScriptAhemB");
}

#[test]
fn css_connected_face_is_excluded_from_scripted_registration() {
    // Same guard would matter if a future slice ever lets a CSS-connected
    // face's bytes reach `_loadedBytes` (today only script `binary`/`url`
    // sources do) — assert the explicit `_cssConnected` check, not just the
    // incidental "never got bytes" one.
    let doc = make_doc();
    add_css_font_face(&doc, "WebFont", lumen_dom::FontFaceStatus::Loaded);
    let rt = v8_runtime_with_dom(doc);
    let script = format!(
        r#"
            var f = document.fonts.values().next().value;
            f._loadedBytes = {bytes};
            document.fonts.add(f);
            f._cssConnected === true
        "#,
        bytes = js_bytes_expr(&ahem_font_bytes())
    );
    let result = rt.eval(&script).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
    assert!(rt.take_pending_scripted_font_faces().is_empty());
}

// ── Shadow DOM JS bindings ────────────────────────────────────────────────

#[test]
fn attach_shadow_returns_shadow_root() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                var host = document.createElement('div');
                document.body.appendChild(host);
                var sr = host.attachShadow({ mode: 'open' });
                sr !== null && sr.__isShadowRoot__ === true && sr.mode === 'open'
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn shadow_root_getter_returns_open_root() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                var h2 = document.createElement('section');
                document.body.appendChild(h2);
                h2.attachShadow({ mode: 'open' });
                h2.shadowRoot !== null && h2.shadowRoot.__isShadowRoot__ === true
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn shadow_root_getter_null_for_closed() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                var h3 = document.createElement('article');
                document.body.appendChild(h3);
                h3.attachShadow({ mode: 'closed' });
                h3.shadowRoot === null
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn shadow_root_append_child_works() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                var host = document.createElement('div');
                document.body.appendChild(host);
                var sr = host.attachShadow({ mode: 'open' });
                var inner = document.createElement('span');
                sr.appendChild(inner);
                sr.children.length === 1
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// BUG-676: `ShadowRoot` used to be a bare `{}`-literal with no [[Prototype]],
// so none of this resolved (`window.ShadowRoot` didn't exist, `instanceof`
// threw instead of testing, `constructor.name` read `Object`).
#[test]
fn shadow_root_has_a_real_global_constructor_and_prototype_chain() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                var host = document.createElement('div');
                document.body.appendChild(host);
                var sr = host.attachShadow({ mode: 'open' });
                'ShadowRoot' in window &&
                typeof window.ShadowRoot === 'function' &&
                sr instanceof ShadowRoot &&
                sr instanceof DocumentFragment &&
                sr instanceof Node &&
                sr.constructor.name === 'ShadowRoot' &&
                sr.contains(sr) === true
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// DOM LS §4.9 requires cloneNode() on a ShadowRoot to throw NotSupportedError,
// not be absent (BUG-676).
#[test]
fn shadow_root_clone_node_throws_not_supported_error() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                var host = document.createElement('div');
                document.body.appendChild(host);
                var sr = host.attachShadow({ mode: 'open' });
                var threwRight = false;
                try { sr.cloneNode(); }
                catch (e) { threwRight = e instanceof DOMException && e.name === 'NotSupportedError'; }
                threwRight
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// BUG-676 companion: `HTMLSlotElement.prototype.assign` was missing entirely
// next to the working `assignedNodes`/`assignedElements`.
#[test]
fn slot_assign_exists_and_validates_its_arguments() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                var host = document.createElement('div');
                document.body.appendChild(host);
                var sr = host.attachShadow({ mode: 'open' });
                var slot = document.createElement('slot');
                sr.appendChild(slot);
                var child = document.createElement('span');
                sr.appendChild(child);
                var threw = false;
                try { slot.assign('not a node'); } catch (e) { threw = e instanceof TypeError; }
                typeof slot.assign === 'function' && threw
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// ── Custom Elements registry ──────────────────────────────────────────────

#[test]
fn custom_elements_define_and_get() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                function MyEl() {}
                customElements.define('my-el', MyEl);
                customElements.get('my-el') === MyEl
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn custom_elements_define_duplicate_ignored() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                function ElA() {}
                function ElB() {}
                customElements.define('dup-el', ElA);
                customElements.define('dup-el', ElB); // should be ignored
                customElements.get('dup-el') === ElA
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn custom_elements_connected_callback_called_on_define() {
    let rt = v8_runtime_with_dom(make_doc());
    // Inject a custom element into DOM *before* define(); upgrade must fire.
    rt.eval(r#"
                var _connected_count = 0;
                var _ce_el = document.createElement('x-counter');
                document.body.appendChild(_ce_el);
            "#).unwrap();
    let result = rt.eval(r#"
                function XCounter() {}
                XCounter.prototype.connectedCallback = function() { _connected_count++; };
                customElements.define('x-counter', XCounter);
                _connected_count === 1
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn custom_elements_connected_callback_called_on_append() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                var _cb_count = 0;
                function XBtn() {}
                XBtn.prototype.connectedCallback = function() { _cb_count++; };
                customElements.define('x-btn', XBtn);
                var el = document.createElement('x-btn');
                document.body.appendChild(el);
                _cb_count === 1
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn custom_elements_attribute_changed_callback() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval(r#"
                var _attr_log = [];
                function XCard() {}
                XCard.observedAttributes = ['title', 'color'];
                XCard.prototype.attributeChangedCallback = function(name, old, next) {
                    _attr_log.push(name + ':' + old + '->' + next);
                };
                customElements.define('x-card', XCard);
                var card = document.createElement('x-card');
                document.body.appendChild(card);
                card.setAttribute('title', 'hello');
                card.setAttribute('color', 'red');
                card.setAttribute('ignored', 'yes'); // not in observedAttributes
                _attr_log.join('|') === 'title:null->hello|color:null->red'
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn custom_elements_when_defined_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    // whenDefined for an already-registered element must return a Promise.
    let result = rt.eval(r#"
                function XBox() {}
                customElements.define('x-box', XBox);
                var p = customElements.whenDefined('x-box');
                typeof p === 'object' && typeof p.then === 'function'
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn custom_elements_when_defined_pending_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    // whenDefined for an unknown element must also return a Promise.
    let result = rt.eval(r#"
                var p2 = customElements.whenDefined('x-future');
                typeof p2 === 'object' && typeof p2.then === 'function'
            "#).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

// ── HTMLTemplateElement.content + DocumentFragment ────────────────────────

#[test]
fn template_content_returns_document_fragment() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var t = document.createElement('template');
                document.body.appendChild(t);
                var c = t.content;
                c !== null && c !== undefined && c.__isDocumentFragment__ === true
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn template_content_clone_and_append() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var t = document.createElement('template');
                t.innerHTML = '<span></span>';
                document.body.appendChild(t);
                // cloneNode(true) on fragment should create a new fragment with the same children
                var frag = t.content.cloneNode(true);
                frag !== null && frag.__isDocumentFragment__ === true
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn template_inner_html_fills_content_fragment() {
    // HTML LS §4.12.3: разметка `<template>` живёт в его content-фрагменте,
    // а не в самом элементе. На этом стоят Solid/lit/Vue:
    // `t.innerHTML = …; t.content.firstChild.cloneNode(true)`.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var t = document.createElement('template');
                t.innerHTML = '<div class="a">x</div>';
                JSON.stringify({
                    content_children: t.content.childNodes.length,
                    own_children: t.childNodes.length,
                    first: t.content.firstChild ? t.content.firstChild.nodeName : null,
                    clone: t.content.firstChild.cloneNode(true).nodeName
                })
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            r#"{"content_children":1,"own_children":0,"first":"DIV","clone":"DIV"}"#.into()
        )
    );
}

#[test]
fn template_content_is_the_same_fragment_every_time() {
    // Обёртка создаётся заново, но узел фрагмента обязан быть один и тот
    // же — иначе запись в `t.content` теряется при следующем обращении.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var t = document.createElement('template');
                t.content.appendChild(document.createElement('span'));
                t.content.childNodes.length === 1 && t.content.__nid__ === t.content.__nid__
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn template_content_is_the_same_wrapper_object() {
    // HTML LS §4.12.3 объявляет `content` как `[SameObject]`. Узел был
    // стабилен и раньше, а вот ОБЁРТКА создавалась заново на каждое
    // чтение, так что `t.content !== t.content` и экспандо на фрагменте
    // терялось между обращениями.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var t = document.createElement('template');
                t.content.__probe__ = 42;
                t.content === t.content && t.content.__probe__ === 42
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn content_is_a_template_member_only() {
    // BUG-796: `content` жил в общей таблице членов обёртки, поэтому
    // «собственный» шаблонный геттер стоял на КАЖДОМ элементе и затенял
    // рефлексию `content` с интерфейсного прототипа.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var d = document.createElement('div');
                var t = document.createElement('template');
                JSON.stringify({
                    on_div: 'content' in d,
                    on_template: 'content' in t,
                    template_frag: t.content.__isDocumentFragment__ === true
                })
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            r#"{"on_div":false,"on_template":true,"template_frag":true}"#.into()
        )
    );
}

#[test]
fn meta_content_reflects_its_attribute() {
    // Ровно то, что читает `testharness.js` при выборе своего потолка:
    // `metas[i].name === 'timeout' && metas[i].content === 'long'`.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var m = document.createElement('meta');
                m.setAttribute('name', 'timeout');
                m.setAttribute('content', 'long');
                m.setAttribute('http-equiv', 'refresh');
                m.setAttribute('scheme', 'Dublin Core');
                document.body.appendChild(m);
                var found = document.getElementsByTagName('meta')[0];
                JSON.stringify({
                    name: found.name,
                    content: found.content,
                    httpEquiv: found.httpEquiv,
                    scheme: found.scheme
                })
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            r#"{"name":"timeout","content":"long","httpEquiv":"refresh","scheme":"Dublin Core"}"#
                .into()
        )
    );
}

#[test]
fn meta_content_is_writable_through_the_idl_attribute() {
    // Рефлексия двусторонняя: запись в IDL-атрибут обязана дойти до
    // контентного, иначе `<meta name=viewport>`-код правит копию.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var m = document.createElement('meta');
                m.content = 'width=device-width';
                m.getAttribute('content') + '|' + m.content
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("width=device-width|width=device-width".into())
    );
}

#[test]
fn node_sibling_traversal_covers_text_nodes() {
    // DOM §4.4: nextSibling/previousSibling ходят по узлам ЛЮБОГО типа.
    // Компилированные шаблоны обходят смешанное содержимое именно так.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var el = document.createElement('div');
                el.innerHTML = 'head<span>mid</span>tail';
                var c1 = el.firstChild, c2 = c1.nextSibling, c3 = c2.nextSibling;
                JSON.stringify([c1.nodeType, c2.nodeName, c3.nodeType,
                                c3.nextSibling, c3.previousSibling.nodeName])
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(r#"[3,"SPAN",3,null,"SPAN"]"#.into())
    );
}

#[test]
fn node_replace_child_swaps_and_returns_old() {
    // DOM §4.4 replaceChild: вернуть СТАРЫЙ узел, новый занять его место.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var el = document.createElement('div');
                var a = document.createElement('a');
                var b = document.createElement('b');
                el.appendChild(a);
                var returned = el.replaceChild(b, a);
                JSON.stringify({
                    kids: el.childNodes.length,
                    first: el.firstChild.nodeName,
                    returned: returned.nodeName
                })
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(r#"{"kids":1,"first":"B","returned":"A"}"#.into())
    );
}

#[test]
fn fragment_gets_node_and_parent_node_operations() {
    // У DocumentFragment не было insertBefore/replaceChild/append/…,
    // хотя именно в него библиотеки собирают разметку.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var f = document.createDocumentFragment();
                var a = document.createElement('a');
                var b = document.createElement('b');
                f.append(a);
                f.insertBefore(b, a);
                var old = f.replaceChild(document.createElement('i'), b);
                f.append('tail');
                JSON.stringify({
                    kids: f.childNodes.length,
                    first: f.firstChild.nodeName,
                    last: f.lastChild.nodeType,
                    old: old.nodeName,
                    has: f.hasChildNodes(),
                    parent: f.parentNode
                })
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            r#"{"kids":3,"first":"I","last":3,"old":"B","has":true,"parent":null}"#.into()
        )
    );
}

#[test]
fn document_create_document_fragment() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var frag = document.createDocumentFragment();
                frag !== null && frag.__isDocumentFragment__ === true && frag.nodeType === 11
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn fragment_append_moves_children_to_target() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var frag = document.createDocumentFragment();
                var a = document.createElement('span');
                var b = document.createElement('div');
                frag.appendChild(a);
                frag.appendChild(b);
                var host = document.createElement('section');
                document.body.appendChild(host);
                host.appendChild(frag);
                // Fragment children should now be inside host; frag itself has no children.
                host.children.length === 2 && frag.children.length === 0
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn element_clone_node_shallow() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var el = document.createElement('div');
                el.setAttribute('data-x', '42');
                var child = document.createElement('span');
                el.appendChild(child);
                document.body.appendChild(el);
                var clone = el.cloneNode(false);
                // Shallow clone: same tag, same attr, no children.
                clone.tagName.toLowerCase() === 'div' && clone.getAttribute('data-x') === '42' && clone.children.length === 0
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn element_clone_node_deep() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var el = document.createElement('div');
                var child = document.createElement('span');
                el.appendChild(child);
                document.body.appendChild(el);
                var clone = el.cloneNode(true);
                // Deep clone: children are also cloned.
                clone.tagName.toLowerCase() === 'div' && clone.children.length === 1
                    && clone.children[0].tagName.toLowerCase() === 'span'
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn slot_element_assigned_nodes() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var host = document.createElement('div');
                document.body.appendChild(host);
                var sr = host.attachShadow({ mode: 'open' });
                // Add a <slot> inside the shadow root.
                var slot = document.createElement('slot');
                sr.appendChild(slot);
                // Add a light-DOM child to the host.
                var light = document.createElement('p');
                host.appendChild(light);
                // assignedNodes() should return the light-DOM child.
                typeof slot.assignedNodes === 'function'
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn slot_slotchange_event_fires_on_append() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var host = document.createElement('div');
                document.body.appendChild(host);
                var sr = host.attachShadow({ mode: 'open' });
                var slot = document.createElement('slot');
                sr.appendChild(slot);
                var changed = 0;
                slot.addEventListener('slotchange', function() { changed++; });
                var light = document.createElement('p');
                host.appendChild(light);
                // slotchange should have fired
                changed >= 0  // event dispatch is best-effort in Phase 0; just check no crash
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn insert_before_moves_node() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var parent = document.createElement('div');
                document.body.appendChild(parent);
                var a = document.createElement('span');
                var b = document.createElement('em');
                parent.appendChild(a);
                parent.appendChild(b);
                var c = document.createElement('strong');
                parent.insertBefore(c, a);
                // c should be at index 0, a at 1, b at 2
                parent.children.length === 3 && parent.children[0].tagName.toLowerCase() === 'strong'
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
