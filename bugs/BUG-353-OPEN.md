# BUG-353 — `element.focus()`/`blur()` and `document.activeElement` missing entirely from the JS shim

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — the live-DOM `Element`/`HTMLElement` object literal and the `document` object literal), shell (`crates/shell/src/main.rs` — `focused_node`, which already exists and is the state a `focus()` binding would drive)
**Найден:** P2, WPT-VENDOR-editing (2026-07-27), `run_report.py --all --root editing --recursive`

## Симптом

`HTMLElement.prototype.focus()` / `blur()` do not exist on any element, and
`document.activeElement` does not exist at all. Both are HTML LS core APIs
(`https://html.spec.whatwg.org/#dom-focus`, `#dom-document-activeelement`),
not optional extensions.

Verified independently of WPT, with `--dump-layout` on a two-line probe page
(a `div` created via `document.createElement`, plus a live `<input>` and
`<textarea>` from the parsed document):

```
PROBE  focus=undefined blur=undefined activeElement=undefined execCommand=function
PROBE2 input.focus=undefined textarea.focus=undefined body.focus=undefined
       win.getSelection=function designMode=undefined
```

So the gap is not element-kind-specific: it is missing on plain elements, on
form controls, and on `document.body` alike. `document.execCommand` and
`window.getSelection` — the two APIs editing tests use *alongside* `focus()` —
are both present, which is why this shows up as a hard `TypeError` in the
middle of otherwise-working editing test setup rather than as a quiet no-op.
`document.designMode` is absent as well (same editing-entry-point family; folded
into this bug rather than filed separately).

The only `focus`/`blur` in the shim (`crates/js/src/dom.rs:11557-11558`) are
no-op stubs on the object literal returned by `window.open()` — a WindowProxy
stub, unrelated to elements.

## Масштаб

In the `editing` category run: **371 subtest failures across 12 test files** are
attributable directly to this, by exact error text:

| Error | Count |
|---|---|
| `editor.focus is not a function` | 334 |
| `editingHost.focus is not a function` | 24 |
| `div.focus is not a function` | 9 |
| `editable.focus is not a function` | 4 |

Worst-hit files: `whitespaces/inserttext-at-end-of-block-when-br-always-block.html`
(187), `other/insert-paragraph-in-void-element.tentative.html` (144),
`other/exec-command-never-throw-exceptions.tentative.html` (12),
`other/selectall-in-editinghost.html` (8).

Well beyond WPT: `el.focus()` is one of the most common DOM calls in real-world
page and framework code (autofocusing a search box or a modal's first field,
focus restoration on dialog close, roving-tabindex widgets, any a11y-conscious
component). Every such call currently throws a `TypeError` that aborts the rest
of the surrounding script. `document.activeElement` is equally common in
focus-management and keyboard-navigation code.

## Причина

Plain omission in the shim, not a deliberate scope decision. The shell **already
tracks focus natively** — `crates/shell/src/main.rs` holds `focused_node:
Option<lumen_dom::NodeId>`, updates it on click (`main.rs:12945-12954`, including
`platform_bridge.focused_node_changed`), and feeds it to layout via
`lumen_layout::set_interactive_state(hovered_nid, focused_node, active_nid)` so
`:focus` styling works. There is simply no JS-side binding that reads or writes
that state: nothing named `activeElement` appears anywhere in `crates/js/`, and
no native `_lumen_focus`-style function is registered.

## Возможный фикс (не реализован в этой сессии)

- Native `_lumen_focus(nid)` / `_lumen_blur(nid)` setting the shell's existing
  `focused_node` through the same path the click handler uses (so `:focus`
  styling, IME target dispatch, and the accessibility tree all stay consistent),
  plus `_lumen_active_element()` returning the current `focused_node`.
- JS: `focus`/`blur` methods on the live `Element` object literal delegating to
  those natives, and an `activeElement` getter on `document`.
- Per spec `focus()` must also fire `focus`/`focusin` (and `blur`/`focusout` on
  the previously focused element) — the shell's `focused_node_changed` bridge is
  the natural place to hang that event dispatch, and `FocusEvent` already exists
  in the shim (`crates/js/src/dom.rs`, see `focusevent_instanceof_chain` test).
- `document.designMode` is a separate, smaller follow-on in the same area.

Not fixed in this session — P2-wpt vendors and surveys, code fixes are P3's lane
(`CLAUDE.md` developer assignments).
