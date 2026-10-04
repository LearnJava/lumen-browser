//! Секции `install_dom`: редактирование — Selection API, `contenteditable`, `designMode`.
//!
//! Вырезано из [`super::dom_core`] батчем SPLIT-JS8 без правки тел; приставка
//! контекста у площадок `reg!` — см. [`super::reg`].

use super::dom_core::log_foreign_node_id;
use super::reg;
#[allow(unused_imports)]
use super::super::*;

/// Selection API (WHATWG Selection API + DOM §4.5).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_selection(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
    dom_dirty: Arc<AtomicBool>,
    flush_stale: Arc<AtomicBool>,
    dom_touched: Arc<Mutex<DomTouched>>,
) -> JsResult<()> {
    // ── Selection API (WHATWG Selection API + DOM §4.5) ─────────────────────
    // Exposes document selection state to JavaScript. The Selection object is a
    // singleton per document; Range objects are snapshots of endpoint pairs.
    {
        // Returns [anchor_nid, anchor_offset, focus_nid, focus_offset] or null.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_selection", move || -> Option<Vec<u32>> {
            let doc = d.lock().unwrap();
            let sel = doc.get_selection();
            match (sel.anchor, sel.focus) {
                (Some(a), Some(f)) => Some(vec![
                    a.container.raw(),
                    a.offset,
                    f.container.raw(),
                    f.offset,
                ]),
                _ => None,
            }
        });
    }
    {
        // Sets selection to [anchor_nid, anchor_offset, focus_nid, focus_offset].
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let stale = Arc::clone(&flush_stale);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_set_selection",
            move |anchor_nid: u32, anchor_off: u32, focus_nid: u32, focus_off: u32| {
                let mut doc = d.lock().unwrap();
                let anchor_id = NodeId::from_raw(anchor_nid);
                let focus_id = NodeId::from_raw(focus_nid);
                // BUG-1031-class: a stale/foreign NodeId stored here panics
                // later, whenever anything (contenteditable delete, caret
                // read) resolves `Selection::anchor`/`focus` — reject it here
                // instead of letting it reach `Document::get` unguarded.
                if !doc.contains_id(anchor_id) || !doc.contains_id(focus_id) {
                    log_foreign_node_id(&doc, "_lumen_set_selection anchor", anchor_nid);
                    log_foreign_node_id(&doc, "_lumen_set_selection focus", focus_nid);
                    return;
                }
                doc.set_selection(Selection {
                    anchor: Some(DomPosition {
                        container: anchor_id,
                        offset: anchor_off,
                    }),
                    focus: Some(DomPosition {
                        container: focus_id,
                        offset: focus_off,
                    }),
                });
                // BUG-341 S7: conservative — no differential test yet proves
                // `::selection` styling is independent of live selection state
                // in this cascade, so a selection change forces a full cascade
                // rather than risk an under-approximated restyle root-set.
                record_dom_touch_unattributed(&touched);
                dirty.store(true, Ordering::Relaxed);
                stale.store(true, Ordering::Relaxed);
            }
        );
    }
    {
        // Clears the current selection.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let stale = Arc::clone(&flush_stale);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_clear_selection", move || {
            let mut doc = d.lock().unwrap();
            doc.set_selection(Selection { anchor: None, focus: None });
            record_dom_touch_unattributed(&touched);
            dirty.store(true, Ordering::Relaxed);
            stale.store(true, Ordering::Relaxed);
        });
    }
    {
        // Returns text of the current selection.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_selection_text", move || -> String {
            let doc = d.lock().unwrap();
            match doc.get_selection().get_range() {
                Some(r) => range_text(&doc, &r),
                None => String::new(),
            }
        });
    }
    {
        // Returns text covered by the given range endpoints.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, 
            "_lumen_get_range_text",
            move |start_nid: u32, start_off: u32, end_nid: u32, end_off: u32| -> String {
                let doc = d.lock().unwrap();
                let r = DomRange {
                    start: DomPosition {
                        container: NodeId::from_raw(start_nid),
                        offset: start_off,
                    },
                    end: DomPosition {
                        container: NodeId::from_raw(end_nid),
                        offset: end_off,
                    },
                };
                range_text(&doc, &r)
            }
        );
    }
    {
        // Number of direct DOM children (element offset validation).
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_node_child_count", move |nid: u32| -> u32 {
            let doc = d.lock().unwrap();
            node_child_count(&doc, NodeId::from_raw(nid)) as u32
        });
    }
    {
        // DOM-spec "length" of node: char count for text, child count for elements.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_node_length", move |nid: u32| -> u32 {
            let doc = d.lock().unwrap();
            node_length(&doc, NodeId::from_raw(nid)) as u32
        });
    }
    {
        // Text content of a node (node.textContent).
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_node_text_content", move |nid: u32| -> String {
            let doc = d.lock().unwrap();
            node_text_content(&doc, NodeId::from_raw(nid))
        });
    }
    {
        // Deletes the contents of range; returns [new_pos_nid, new_pos_offset].
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let stale = Arc::clone(&flush_stale);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_range_delete_contents",
            move |start_nid: u32, start_off: u32, end_nid: u32, end_off: u32| -> Vec<u32> {
                let mut doc = d.lock().unwrap();
                let r = DomRange {
                    start: DomPosition {
                        container: NodeId::from_raw(start_nid),
                        offset: start_off,
                    },
                    end: DomPosition {
                        container: NodeId::from_raw(end_nid),
                        offset: end_off,
                    },
                };
                let pos = lumen_dom::delete_range(&mut doc, &r);
                // BUG-341 S7: arbitrary-range content deletion can remove
                // whole elements — not attributable to a simple node set.
                record_dom_touch_unattributed(&touched);
                dirty.store(true, Ordering::Relaxed);
                stale.store(true, Ordering::Relaxed);
                vec![pos.container.raw(), pos.offset]
            }
        );
    }
    Ok(())
}

/// `contenteditable` mutation bindings (Input Events L2 §4.1).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_contenteditable(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
) -> JsResult<()> {
    // ── contenteditable mutation bindings (Input Events Level 2 §4.1) ─────────
    // These are called by the JS shim's _lumen_handle_contenteditable_key()
    // which fires beforeinput → calls here → fires input.
    {
        // True if nid or any ancestor has contenteditable set to a truthy value.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_is_contenteditable", move |nid: u32| -> bool {
            let doc = d.lock().unwrap();
            lumen_dom::find_editing_host(&doc, NodeId::from_raw(nid)).is_some()
        });
    }
    Ok(())
}

/// `document.designMode` and the editing command surface (HTML LS §6.6.3, BUG-353).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_design_mode(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
    dom_dirty: Arc<AtomicBool>,
    flush_stale: Arc<AtomicBool>,
    dom_touched: Arc<Mutex<DomTouched>>,
) -> JsResult<()> {
    // ── document.designMode (HTML LS §6.6.3, BUG-353) ──────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_design_mode", move || -> bool {
            d.lock().unwrap().design_mode()
        });
    }
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_set_design_mode", move |enabled: bool| {
            d.lock().unwrap().set_design_mode(enabled);
        });
    }
    {
        // Insert `text` at the current selection (or caret) inside contenteditable.
        // Replaces selected content if the selection is non-collapsed.
        // Returns true on success.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let stale = Arc::clone(&flush_stale);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_contenteditable_insert_text", move |text: String| -> bool {
            if text.is_empty() { return false; }
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            let Some(anchor) = sel.anchor else { return false; };
            let insert_pos = if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                lumen_dom::delete_range(&mut doc, &r)
            } else {
                anchor
            };
            let new_pos = lumen_dom::insert_text_at(&mut doc, insert_pos, &text);
            doc.set_selection(Selection { anchor: Some(new_pos), focus: Some(new_pos) });
            // BUG-341 S7: text insertion at an arbitrary caret position — not
            // attributable to a simple node set.
            record_dom_touch_unattributed(&touched);
            dirty.store(true, Ordering::Relaxed);
            stale.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Delete one grapheme cluster before the caret (Backspace key).
        // If the selection is non-collapsed, deletes the selection instead.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let stale = Arc::clone(&flush_stale);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_contenteditable_delete_backward", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            // Non-collapsed selection: delete it.
            if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                let pos = lumen_dom::delete_range(&mut doc, &r);
                doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
                record_dom_touch_unattributed(&touched);
                dirty.store(true, Ordering::Relaxed);
                stale.store(true, Ordering::Relaxed);
                return true;
            }
            let Some(anchor) = sel.anchor else { return false; };
            if anchor.offset == 0 { return false; }
            let text = match &doc.get(anchor.container).data {
                NodeData::Text(s) => s.clone(),
                _ => return false,
            };
            // Walk backward one UTF-8 character boundary.
            let off = anchor.offset as usize;
            let mut prev = off.saturating_sub(1);
            while prev > 0 && !text.is_char_boundary(prev) {
                prev -= 1;
            }
            let r = DomRange {
                start: DomPosition { container: anchor.container, offset: prev as u32 },
                end: anchor,
            };
            let pos = lumen_dom::delete_range(&mut doc, &r);
            doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
            record_dom_touch_unattributed(&touched);
            dirty.store(true, Ordering::Relaxed);
            stale.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Delete one grapheme cluster after the caret (Delete key).
        // If the selection is non-collapsed, deletes the selection instead.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let stale = Arc::clone(&flush_stale);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_contenteditable_delete_forward", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                let pos = lumen_dom::delete_range(&mut doc, &r);
                doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
                record_dom_touch_unattributed(&touched);
                dirty.store(true, Ordering::Relaxed);
                stale.store(true, Ordering::Relaxed);
                return true;
            }
            let Some(anchor) = sel.anchor else { return false; };
            let text = match &doc.get(anchor.container).data {
                NodeData::Text(s) => s.clone(),
                _ => return false,
            };
            let off = anchor.offset as usize;
            if off >= text.len() { return false; }
            // Walk forward one UTF-8 character boundary.
            let mut next = off + 1;
            while next < text.len() && !text.is_char_boundary(next) {
                next += 1;
            }
            let r = DomRange {
                start: anchor,
                end: DomPosition { container: anchor.container, offset: next as u32 },
            };
            let pos = lumen_dom::delete_range(&mut doc, &r);
            doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
            record_dom_touch_unattributed(&touched);
            dirty.store(true, Ordering::Relaxed);
            stale.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Split the block at the caret position (Enter key in contenteditable).
        // Finds the editing host, then calls insert_paragraph_break.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let stale = Arc::clone(&flush_stale);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_contenteditable_insert_paragraph", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            let pos = if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                lumen_dom::delete_range(&mut doc, &r)
            } else if let Some(p) = sel.anchor {
                p
            } else {
                return false;
            };
            let Some(host) = lumen_dom::find_editing_host(&doc, pos.container) else {
                return false;
            };
            let new_pos = lumen_dom::insert_paragraph_break(&mut doc, pos, host);
            doc.set_selection(Selection { anchor: Some(new_pos), focus: Some(new_pos) });
            record_dom_touch_unattributed(&touched);
            dirty.store(true, Ordering::Relaxed);
            stale.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // execCommand: bold/italic/underline/insertText/delete/selectAll/copy/cut/paste
        // Returns true if the command was handled.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let stale = Arc::clone(&flush_stale);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, 
            "_lumen_exec_command",
            move |cmd: String, value: String| -> bool {
                let mut doc = d.lock().unwrap();
                let sel = doc.get_selection().clone();
                match cmd.as_str() {
                    "selectAll" => {
                        // Select entire document body text
                        if let Some(body) = find_element_by_tag(&doc, "body") {
                            let children = doc.get(body).children.clone();
                            if !children.is_empty() {
                                let first = *children.first().unwrap();
                                let last = *children.last().unwrap();
                                let last_len = node_length(&doc, last);
                                doc.set_selection(Selection {
                                    anchor: Some(DomPosition { container: first, offset: 0 }),
                                    focus: Some(DomPosition {
                                        container: last,
                                        offset: last_len as u32,
                                    }),
                                });
                                record_dom_touch_unattributed(&touched);
                                dirty.store(true, Ordering::Relaxed);
                                stale.store(true, Ordering::Relaxed);
                            }
                        }
                        true
                    }
                    "insertText" => {
                        if let Some(pos) = sel.anchor {
                            // Delete selection first if non-collapsed
                            let pos = sel
                                .get_range()
                                .filter(|r| !r.is_collapsed())
                                .map(|r| lumen_dom::delete_range(&mut doc, &r))
                                .unwrap_or(pos);
                            let new_pos = lumen_dom::insert_text_at(&mut doc, pos, &value);
                            doc.set_selection(Selection {
                                anchor: Some(new_pos),
                                focus: Some(new_pos),
                            });
                            record_dom_touch_unattributed(&touched);
                            dirty.store(true, Ordering::Relaxed);
                            stale.store(true, Ordering::Relaxed);
                        }
                        true
                    }
                    "delete" | "forwardDelete" => {
                        if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                            let pos = lumen_dom::delete_range(&mut doc, &r);
                            doc.set_selection(Selection {
                                anchor: Some(pos),
                                focus: Some(pos),
                            });
                            record_dom_touch_unattributed(&touched);
                            dirty.store(true, Ordering::Relaxed);
                            stale.store(true, Ordering::Relaxed);
                        }
                        true
                    }
                    // bold/italic/underline: CSSOM inline style toggling (stub — returns true
                    // so editors know the command is accepted; real inline-style mutation
                    // requires Range wrapping which is Phase 3 contenteditable work).
                    "bold" | "italic" | "underline" | "strikeThrough"
                    | "justifyLeft" | "justifyCenter" | "justifyRight" | "justifyFull"
                    | "indent" | "outdent"
                    | "createLink" | "unlink"
                    | "insertOrderedList" | "insertUnorderedList"
                    | "fontName" | "fontSize" | "foreColor" | "backColor"
                    | "removeFormat" => true,
                    // copy/cut/paste: clipboard interaction is handled by the shell;
                    // returning false lets it fall through to native clipboard handling.
                    "copy" | "cut" | "paste" => false,
                    _ => false,
                }
            }
        );
    }
    Ok(())
}
