//! BUG-480 срез 2 — мост `contentWindow`/`contentDocument` из JS родителя в
//! под-документ `<iframe>` (HTML LS §4.8.5 content navigable).
//!
//! Срез 1 (`load_frame_sub_documents`, shell) загружает каждый статический
//! `<iframe>` в отдельный `Document` с собственным V8-контекстом. Изолят у
//! каждого контекста свой, поэтому прямой передачи объектов между окнами нет
//! и не будет (см. commit-message среза 1); вместо этого этот модуль держит в
//! **родительском** изоляте реестр биндингов «хост-элемент → под-документ» и
//! семейство нативов `_lumen_f_*`, читающих под-документ через его
//! `Arc<Mutex<Document>>`. JS-фасады (Window/Document/Element) строятся шимом
//! поверх этих нативов и интернируются, поэтому `iframe.contentWindow ===`
//! `iframe.contentWindow`, а `contentDocument.defaultView === contentWindow`.
//!
//! Поток данных: shell после загрузки ребёнка вызывает
//! [`V8JsRuntime::register_frame_document`] (до диспатча trusted `load` на
//! хосте), биндинг ложится в реестр — геттеры из `iframe_element.rs` начинают
//! видеть фасады сразу, патчить враппер хоста не нужно.
//!
//! Границы среза 2:
//! - доступ только чтение; мутации из родителя и события — будущие срезы;
//! - cross-origin / opaque-sandbox (`sandbox` без `allow-same-origin`)
//!   биндинги регистрируются с `accessible: false`: `contentWindow` отдаёт
//!   фасад без `.document` (спека: WindowProxy доступен всегда),
//!   `contentDocument` — `null`;
//! - фреймы без загруженного под-документа (динамически созданные `<iframe>`,
//!   неудавшийся fetch) биндинга не имеют — оба геттера дают `null`;
//! - геометрия (`offsetWidth`, `getBoundingClientRect`) — честные нули:
//!   layout содержимого фрейма — отдельный срез.

#[cfg(feature = "v8-backend")]
use std::sync::{Arc, Mutex};

/// Один зарегистрированный под-документ `<iframe>` в реестре рантайма.
///
/// Живёт в [`FrameDocRegistry`] столько же, сколько контекст страницы:
/// биндинги никогда не удаляются по одному — замена страницы уносит весь
/// рантайм вместе с реестром (тот же lifecycle, что у [`crate::img_bitmap_store`]).
#[cfg(feature = "v8-backend")]
pub(crate) struct FrameDocBinding {
    /// `NodeId` элемента-хоста `<iframe>` в документе родителя.
    pub(crate) host_nid: u32,
    /// Под-документ. Отдельный `Arc` — его же держит shell (`FrameHandle.doc`)
    /// и JS-контекст самого ребёнка.
    pub(crate) doc: Arc<Mutex<lumen_dom::Document>>,
    /// Разрешённый адрес под-документа (`about:srcdoc`/`about:blank`/URL).
    pub(crate) url: String,
    /// `false` — cross-origin или opaque sandbox: нативы чтения отдают пустые
    /// результаты, `.document` фасада окна — `null`.
    pub(crate) accessible: bool,
}

/// Реестр биндингов «хост → под-документ» одного V8-изолята.
///
/// Общий `Arc` между `V8JsRuntime`, нативами этого модуля и вызовом
/// [`V8JsRuntime::register_frame_document`]; индекс в векторе — стабильный
/// идентификатор биндинга (`bid`) для всех нативов `_lumen_f_*`.
#[cfg(feature = "v8-backend")]
pub(crate) type FrameDocRegistry = Arc<Mutex<Vec<FrameDocBinding>>>;

/// Захватить биндинг `bid` на чтение, если он существует и разрешён.
///
/// Все нативы чтения проходят через эту точку: несуществующий bid и
/// cross-origin/opaque bid неотличимы для вызывающего JS — оба дают «пусто».
#[cfg(feature = "v8-backend")]
fn with_accessible_doc<R>(
    registry: &FrameDocRegistry,
    bid: u32,
    f: impl FnOnce(&lumen_dom::Document) -> R,
    empty: R,
) -> R {
    let reg = registry.lock().unwrap_or_else(|e| e.into_inner());
    let Some(binding) = reg.get(bid as usize) else {
        return empty;
    };
    if !binding.accessible {
        return empty;
    }
    let doc = binding.doc.lock().unwrap_or_else(|e| e.into_inner());
    f(&doc)
}

/// Первый элемент с тегом `tag` (ASCII case-insensitive) в document order.
/// Mirrors `v8_runtime::find_element_by_tag`.
#[cfg(feature = "v8-backend")]
fn find_element_by_tag(doc: &lumen_dom::Document, tag: &str) -> Option<lumen_dom::NodeId> {
    find_first_matching(doc, doc.root(), &|node| {
        node.element_name()
            .map(|n| n.local.eq_ignore_ascii_case(tag))
            .unwrap_or(false)
    })
}

/// Предзаказный обход поддерева с предикатом. Mirrors `v8_runtime::find_first_matching`.
#[cfg(feature = "v8-backend")]
fn find_first_matching(
    doc: &lumen_dom::Document,
    start: lumen_dom::NodeId,
    pred: &dyn Fn(&lumen_dom::Node) -> bool,
) -> Option<lumen_dom::NodeId> {
    let node = doc.get(start);
    if pred(node) {
        return Some(start);
    }
    for &child in &node.children.clone() {
        if let Some(found) = find_first_matching(doc, child, pred) {
            return Some(found);
        }
    }
    None
}

/// Конкатенация текстовых узлов поддерева. Mirrors `v8_runtime::collect_text_content`.
#[cfg(feature = "v8-backend")]
fn collect_text_content(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> String {
    if let lumen_dom::NodeData::Comment(s) = &doc.get(id).data {
        return s.clone();
    }
    fn inner(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
        let node = doc.get(id);
        if let lumen_dom::NodeData::Text(s) = &node.data {
            out.push_str(s);
        }
        for &child in &node.children.clone() {
            inner(doc, child, out);
        }
    }
    let mut out = String::new();
    inner(doc, id, &mut out);
    out
}

/// Зарегистрировать нативы `_lumen_f_*` + оценить JS-шим фасадов.
///
/// Вызывается из `install_dom` (список `install_v8!`) с клоном реестра
/// рантайма — тем же, куда пишет [`V8JsRuntime::register_frame_document`].
#[cfg(feature = "v8-backend")]
pub(crate) fn install_frame_bridge_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    registry: FrameDocRegistry,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn1, into_v8_fn2, into_v8_fn3};
    use lumen_core::ext::JsRuntime as _;

    // bid → есть ли биндинг вообще (для contentWindow, который существует
    // даже при accessible=false).
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_binding",
            into_v8_fn1(move |host_nid: u32| -> Option<u32> {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                reg.iter()
                    .position(|b| b.host_nid == host_nid)
                    .map(|i| i as u32)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_accessible",
            into_v8_fn1(move |bid: u32| -> bool {
                reg.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(bid as usize)
                    .is_some_and(|b| b.accessible)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_host",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                reg.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(bid as usize)
                    .filter(|b| b.accessible)
                    .map(|b| b.host_nid)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_url",
            into_v8_fn1(move |bid: u32| -> String {
                match reg.lock().unwrap_or_else(|e| e.into_inner()).get(bid as usize) {
                    Some(b) if b.accessible => b.url.clone(),
                    _ => String::new(),
                }
            }),
        )?;
    }

    // ── Document-level чтение ────────────────────────────────────────────────
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_body",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    find_element_by_tag(d, "body").map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_head",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    find_element_by_tag(d, "head").map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_document_element",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| d.document_element().map(|n| n.index() as u32), None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_title",
            into_v8_fn1(move |bid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| {
                    find_element_by_tag(d, "title")
                        .map(|nid| collect_text_content(d, nid))
                        .unwrap_or_default()
                }, String::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_by_id",
            into_v8_fn2(move |bid: u32, id: String| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    find_first_matching(d, d.root(), &|node| {
                        matches!(&node.data, lumen_dom::NodeData::Element { .. })
                            && node.get_attr("id") == Some(id.as_str())
                    })
                    .map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query",
            into_v8_fn2(move |bid: u32, sel: String| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all(d, &sel).into_iter().next().map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query_all",
            into_v8_fn2(move |bid: u32, sel: String| -> Vec<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all(d, &sel)
                        .into_iter()
                        .map(|n| n.index() as u32)
                        .collect()
                }, Vec::new())
            }),
        )?;
    }

    // ── Element-level чтение ─────────────────────────────────────────────────
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query_scoped",
            into_v8_fn3(move |bid: u32, nid: u32, sel: String| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all_scoped(d, lumen_dom::NodeId::from_index(nid as usize), &sel)
                        .into_iter()
                        .next()
                        .map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query_all_scoped",
            into_v8_fn3(move |bid: u32, nid: u32, sel: String| -> Vec<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all_scoped(d, lumen_dom::NodeId::from_index(nid as usize), &sel)
                        .into_iter()
                        .map(|n| n.index() as u32)
                        .collect()
                }, Vec::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_children",
            into_v8_fn2(move |bid: u32, nid: u32| -> Vec<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    d.get(lumen_dom::NodeId::from_index(nid as usize))
                        .children
                        .iter()
                        .filter(|&&c| matches!(&d.get(c).data, lumen_dom::NodeData::Element { .. }))
                        .map(|&c| c.index() as u32)
                        .collect()
                }, Vec::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_parent_element",
            into_v8_fn2(move |bid: u32, nid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    let id = lumen_dom::NodeId::from_index(nid as usize);
                    d.get(id).parent.and_then(|pid| {
                        matches!(&d.get(pid).data, lumen_dom::NodeData::Element { .. })
                            .then(|| pid.index() as u32)
                    })
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_attr",
            into_v8_fn3(move |bid: u32, nid: u32, name: String| -> Option<String> {
                with_accessible_doc(&reg, bid, |d| {
                    d.get(lumen_dom::NodeId::from_index(nid as usize))
                        .get_attr(&name)
                        .map(str::to_owned)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_has_attr",
            into_v8_fn3(move |bid: u32, nid: u32, name: String| -> bool {
                with_accessible_doc(&reg, bid, |d| {
                    d.get(lumen_dom::NodeId::from_index(nid as usize))
                        .get_attr(&name)
                        .is_some()
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_tag",
            into_v8_fn2(move |bid: u32, nid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| {
                    match &d.get(lumen_dom::NodeId::from_index(nid as usize)).data {
                        lumen_dom::NodeData::Element { name, .. } => name.local.clone(),
                        _ => String::new(),
                    }
                }, String::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_is_text",
            into_v8_fn2(move |bid: u32, nid: u32| -> bool {
                with_accessible_doc(&reg, bid, |d| {
                    matches!(
                        &d.get(lumen_dom::NodeId::from_index(nid as usize)).data,
                        lumen_dom::NodeData::Text(_)
                    )
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_is_comment",
            into_v8_fn2(move |bid: u32, nid: u32| -> bool {
                with_accessible_doc(&reg, bid, |d| {
                    matches!(
                        &d.get(lumen_dom::NodeId::from_index(nid as usize)).data,
                        lumen_dom::NodeData::Comment(_)
                    )
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_text",
            into_v8_fn2(move |bid: u32, nid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| {
                    collect_text_content(d, lumen_dom::NodeId::from_index(nid as usize))
                }, String::new())
            }),
        )?;
    }

    rt.eval(FRAME_BRIDGE_SHIM)?;
    Ok(())
}

/// JavaScript shim: фасады Window/Document/Element над нативами `_lumen_f_*`.
///
/// Точка входа для геттеров `iframe_element.rs` — две глобальные функции,
/// принимающие `__nid__` хоста; всё остальное спрятано в замыкании модуля.
#[cfg(feature = "v8-backend")]
const FRAME_BRIDGE_SHIM: &str = r#"(function() {
  'use strict';

  // Интерны фасадов, ключ — bid (стабильный индекс биндинга). Живут столько
  // же, сколько контекст страницы: identity фасадов обязана быть постоянной.
  var wins = {};
  var docs = {};
  var elems = {};

  function bidOrNull(hostNid) {
    if (hostNid === null || hostNid === undefined) return null;
    var bid = _lumen_frame_binding(hostNid);
    return (bid === null || bid === undefined) ? null : bid;
  }

  function frameElem(bid, nid) {
    if (nid === null || nid === undefined || nid < 0) return null;
    var cache = elems[bid];
    if (!cache) { cache = {}; elems[bid] = cache; }
    var cached = cache[nid];
    if (cached) return cached;
    var el = {
      __nid__: nid,
      get nodeType() {
        if (_lumen_f_is_text(bid, nid)) return 3;
        if (_lumen_f_is_comment(bid, nid)) return 8;
        return 1;
      },
      get localName()  { return _lumen_f_tag(bid, nid); },
      get tagName()    { var t = _lumen_f_tag(bid, nid); return t ? t.toUpperCase() : t; },
      get nodeName()   { var t = _lumen_f_tag(bid, nid); return t ? t.toUpperCase() : t; },
      get id()         { var v = _lumen_f_attr(bid, nid, 'id'); return v !== null && v !== undefined ? v : ''; },
      get className()  { var v = _lumen_f_attr(bid, nid, 'class'); return v !== null && v !== undefined ? v : ''; },
      get textContent(){ return _lumen_f_text(bid, nid); },
      getAttribute: function(n) { return _lumen_f_attr(bid, nid, String(n)); },
      hasAttribute: function(n) { return _lumen_f_has_attr(bid, nid, String(n)); },
      get children() { return _lumen_f_children(bid, nid).map(function(c) { return frameElem(bid, c); }); },
      get childElementCount() { return _lumen_f_children(bid, nid).length; },
      get firstElementChild() { return frameElem(bid, _lumen_f_children(bid, nid)[0]); },
      get lastElementChild() {
        var ch = _lumen_f_children(bid, nid);
        return frameElem(bid, ch[ch.length - 1]);
      },
      get parentElement() { return frameElem(bid, _lumen_f_parent_element(bid, nid)); },
      querySelector: function(sel) { return frameElem(bid, _lumen_f_query_scoped(bid, nid, String(sel))); },
      querySelectorAll: function(sel) {
        return _lumen_f_query_all_scoped(bid, nid, String(sel)).map(function(c) { return frameElem(bid, c); });
      },
      // BUG-480 срез 2: содержимое фрейма не layout'ится — честные нули вместо
      // выдуманных размеров (layout фреймов — будущий срез).
      get offsetWidth()  { return 0; },
      get offsetHeight() { return 0; },
      get clientWidth()  { return 0; },
      get clientHeight() { return 0; },
      get scrollWidth()  { return 0; },
      get scrollHeight() { return 0; },
      getBoundingClientRect: function() {
        return { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0 };
      },
    };
    cache[nid] = el;
    return el;
  }

  function docFacade(bid) {
    var cached = docs[bid];
    if (cached) return cached;
    var d = {};
    function el(nid) { return frameElem(bid, nid); }
    Object.defineProperty(d, 'body',              { get: function() { return el(_lumen_f_body(bid)); }, configurable: true });
    Object.defineProperty(d, 'head',              { get: function() { return el(_lumen_f_head(bid)); }, configurable: true });
    Object.defineProperty(d, 'documentElement',   { get: function() { return el(_lumen_f_document_element(bid)); }, configurable: true });
    Object.defineProperty(d, 'title',             { get: function() { return _lumen_f_title(bid); }, configurable: true });
    Object.defineProperty(d, 'URL',               { get: function() { return _lumen_f_url(bid); }, configurable: true });
    Object.defineProperty(d, 'documentURI',       { get: function() { return _lumen_f_url(bid); }, configurable: true });
    // Ребёнок получил window load ещё в срезе 1 — readyState к моменту доступа
    // всегда «complete»; отдельного трекинга переходов срез 2 не ведёт.
    Object.defineProperty(d, 'readyState',        { get: function() { return 'complete'; }, configurable: true });
    Object.defineProperty(d, 'defaultView',       { get: function() { return winFacade(bid); }, configurable: true });
    d.getElementById = function(id) { return el(_lumen_f_by_id(bid, String(id))); };
    d.querySelector = function(sel) { return el(_lumen_f_query(bid, String(sel))); };
    d.querySelectorAll = function(sel) {
      return _lumen_f_query_all(bid, String(sel)).map(function(n) { return frameElem(bid, n); });
    };
    docs[bid] = d;
    return d;
  }

  function winFacade(bid) {
    var cached = wins[bid];
    if (cached) return cached;
    var w = {};
    var hostNid = _lumen_f_accessible(bid) ? _lumen_f_host(bid) : null;
    Object.defineProperty(w, 'document', {
      get: function() { return _lumen_f_accessible(bid) ? docFacade(bid) : null; },
      configurable: true,
    });
    w.window = w;
    w.self = w;
    w.frames = w;
    // Бридж живёт в изоляте родителя, глубина фреймов <= MAX_FRAME_DEPTH(2):
    // parent и top — настоящий window этого контекста. Читается лениво:
    // шим исполняется до WEB_API_SHIM-определений? Нет — после, но тестовые
    // изоляты без полного DOM могут не иметь window вовсе.
    Object.defineProperty(w, 'parent', {
      get: function() { return typeof window !== 'undefined' ? window : null; },
      configurable: true,
    });
    Object.defineProperty(w, 'top', {
      get: function() { return typeof window !== 'undefined' ? window : null; },
      configurable: true,
    });
    Object.defineProperty(w, 'closed', { get: function() { return false; }, configurable: true });
    w.length = 0;
    Object.defineProperty(w, 'frameElement', {
      get: function() {
        return (hostNid !== null && typeof _lumen_make_element === 'function')
          ? _lumen_make_element(hostNid)
          : null;
      },
      configurable: true,
    });
    Object.defineProperty(w, 'name', {
      get: function() {
        if (hostNid === null) return '';
        var a = _lumen_get_attr(hostNid, 'name');
        return (a === null || a === undefined) ? '' : a;
      },
      configurable: true,
    });
    Object.defineProperty(w, 'location', {
      get: function() {
        var href = _lumen_f_url(bid);
        return { href: href, toString: function() { return href; } };
      },
      configurable: true,
    });
    w.close = function() {};
    wins[bid] = w;
    return w;
  }

  globalThis._lumen_frame_content_document = function(hostNid) {
    var bid = bidOrNull(hostNid);
    if (bid === null || !_lumen_f_accessible(bid)) return null;
    return docFacade(bid);
  };
  globalThis._lumen_frame_content_window = function(hostNid) {
    var bid = bidOrNull(hostNid);
    if (bid === null) return null;
    return winFacade(bid);
  };
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Рантайм с установленным бриджем и одним биндингом.
    ///
    /// `html` парсится как полный под-документ; `accessible=false` моделирует
    /// cross-origin/opaque-sandbox фрейм.
    fn with_frame(html: &str, accessible: bool, f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(Vec::new()));
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        let doc = lumen_html_parser::parse(html);
        registry.lock().unwrap().push(FrameDocBinding {
            host_nid: 7,
            doc: Arc::new(Mutex::new(doc)),
            url: "about:srcdoc".to_owned(),
            accessible,
        });
        // Прод-контекст всегда имеет window (WEB_API_SHIM); тестовый изолят —
        // нет, а шим бриджа связывает w.parent/w.top с ним.
        rt.eval("var window = globalThis;").unwrap();
        f(&rt);
    }

    /// Рантайм с бриджем и пустым реестром — моделирует страницу без
    /// загруженных фреймов.
    fn with_empty_registry(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(Vec::new()));
        install_frame_bridge_v8(&rt, registry).unwrap();
        f(&rt);
    }

    fn eval_bool(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn unbound_iframe_gives_null_for_both_getters() {
        with_empty_registry(|rt| {
            assert!(eval_bool(
                rt,
                "_lumen_frame_content_window(7) === null && _lumen_frame_content_document(7) === null"
            ));
        });
    }

    #[test]
    fn content_document_facade_reads_child_tree() {
        with_frame(
            "<html><body><div id='a' class='x'>hello</div></body></html>",
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d !== null && d.body.tagName === 'BODY' && d.body.nodeType === 1"
                ));
                assert!(eval_bool(rt, "_lumen_frame_content_document(7).title === ''"));
                assert!(eval_bool(
                    rt,
                    "var e = _lumen_frame_content_document(7).getElementById('a'); \
                     e.localName === 'div' && e.id === 'a' && e.className === 'x' \
                     && e.textContent === 'hello' && e.getAttribute('class') === 'x'"
                ));
                assert!(eval_bool(
                    rt,
                    "var all = _lumen_frame_content_document(7).querySelectorAll('div'); \
                     all.length === 1 && all[0].id === 'a'"
                ));
                assert!(eval_bool(
                    rt,
                    "_lumen_frame_content_document(7).querySelector('body').tagName === 'BODY' \
                     && _lumen_frame_content_document(7).querySelector('nothing') === null"
                ));
            },
        );
    }

    #[test]
    fn facades_are_interned_and_cross_linked() {
        with_frame("<html><body></body></html>", true, |rt| {
            assert!(eval_bool(
                rt,
                "var w1 = _lumen_frame_content_window(7), w2 = _lumen_frame_content_window(7); \
                 w1 === w2"
            ));
            assert!(eval_bool(
                rt,
                "var d1 = _lumen_frame_content_document(7), d2 = _lumen_frame_content_document(7); \
                 d1 === d2 && w1.document === d1 && d1.defaultView === w1"
            ));
            assert!(eval_bool(
                rt,
                "w1.window === w1 && w1.self === w1 && w1.frames === w1 \
                 && w1.parent === window && w1.top === window && w1.closed === false"
            ));
            assert!(eval_bool(
                rt,
                "var b1 = d1.body, b2 = d1.body; b1 === b2 && b1.parentElement === d1.documentElement"
            ));
        });
    }

    #[test]
    fn inaccessible_frame_hides_document_but_keeps_window() {
        with_frame("<html><body><p>secret</p></body></html>", false, |rt| {
            assert!(eval_bool(rt, "_lumen_frame_content_document(7) === null"));
            assert!(eval_bool(
                rt,
                "var w = _lumen_frame_content_window(7); \
                 w !== null && w.document === null && w.location.href === ''"
            ));
        });
    }

    #[test]
    fn unknown_host_returns_null_without_touching_registry() {
        with_frame("<html><body></body></html>", true, |rt| {
            assert!(eval_bool(
                rt,
                "_lumen_frame_content_window(99) === null && _lumen_frame_content_document(99) === null"
            ));
        });
    }

    #[test]
    fn element_children_and_parent_walk_child_tree_only() {
        with_frame(
            "<html><body><ul><li>one</li><li>two</li></ul></body></html>",
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "var ul = _lumen_frame_content_document(7).querySelector('ul'); \
                     ul.children.length === 2 && ul.children[0].textContent === 'one' \
                     && ul.firstElementChild.textContent === 'one' \
                     && ul.lastElementChild.textContent === 'two' \
                     && ul.children[0].parentElement === ul \
                     && ul.parentElement.tagName === 'BODY'"
                ));
            },
        );
    }

    #[test]
    fn geometry_is_zero_until_frame_layout_lands() {
        with_frame("<html><body><p>x</p></body></html>", true, |rt| {
            assert!(eval_bool(
                rt,
                "var b = _lumen_frame_content_document(7).body; \
                 b.offsetWidth === 0 && b.getBoundingClientRect().width === 0"
            ));
        });
    }
}
