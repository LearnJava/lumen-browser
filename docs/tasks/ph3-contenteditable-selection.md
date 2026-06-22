# Ph3 — contenteditable + Input Events L2 + Selection/Range

**Developer:** P1 + P4 · **Branch:** `p1-ph3-contenteditable` · **Size:** XL · **Crates:** `lumen-dom`, `lumen-js`, `lumen-shell`

---

## Status

**Phase 3 (v1.0) — FUTURE.** Roadmap item: `docs/plan/phases.md:128` —
*"`<contenteditable>` + Input Events Level 2 + Selection / Range API `[P1+P4]`"*.
Do **not** start before Phase 2 is closed and the version is bumped to 0.5.0. This
file is the standing plan; pick it up only when explicitly assigned the Phase 3 item.

Plan split (from `phases.md:128`):
- **P1** — DOM mutations + Selection/Range types + `beforeinput`/`input` event firing.
- **P4** — input dispatch (keyboard / IME / drag-drop / paste) + undo stack.

> **Note for whoever picks this up:** a surprising amount of the P1 half already
> exists (see *Current state*). The remaining work is mostly the P4 half — wiring
> real user input (mouse caret placement, IME commit, clipboard paste, drag-drop)
> into the existing DOM mutation primitives, plus making the undo stack actually
> reachable from the keyboard. Verify each "done" item against code before
> re-implementing — much of the scaffolding is in place but **unwired**.

---

## Goal

Make arbitrary `contenteditable` regions fully editable with the keyboard, IME,
clipboard, and pointer, matching the observable behaviour of `<input>`/`<textarea>`
editing that already ships, but generalised to any DOM subtree:

1. Click places a caret inside the editable host; drag selects a range.
2. Typing, Enter, Backspace/Delete, word-delete mutate the DOM at the caret and
   fire spec-correct `beforeinput` (cancelable) → mutation → `input` events.
3. IME composition (CJK / Cyrillic) shows preedit and commits into the caret.
4. Ctrl+C / Ctrl+X / Ctrl+V and drag-drop move HTML/plain-text content in and out.
5. Ctrl+Z / Ctrl+Y undo and redo through a per-host command history.
6. `window.getSelection()` / `document.createRange()` reflect the live caret/range
   and `selectionchange` fires when it moves.

---

## Current state (precedent + existing scaffolding)

### Form-field editing precedent (the model contenteditable generalises)
- `<input>`/`<textarea>` values live in a JS-side map `_input_values` keyed by nid —
  `crates/js/src/dom.rs:3610` (`var _input_values = {}`), getter/setter at
  `crates/js/src/dom.rs:4568`–`4574`. This is a flat string store, **not** DOM
  mutation; contenteditable replaces it with real text-node edits.
- Value painted from that store: `emit_input_value_text` —
  `crates/engine/paint/src/display_list.rs:4065` / call site `:4262`.
- Programmatic value writes (color/date pickers): `forms::set_value` —
  `crates/shell/src/forms.rs:157`, called from `crates/shell/src/main.rs:10971`, `:11010`.
- Form classification / control plumbing: `crates/shell/src/forms.rs:72`
  (`classify_click`), submit at `:591`.

### contenteditable detection (DONE)
- `node_is_contenteditable` — `crates/engine/dom/src/lib.rs:1796`.
- `find_editing_host` (walks ancestors to the editing host) —
  `crates/engine/dom/src/lib.rs:1808`+ (returns `Option<NodeId>`).
- Effective inheritance incl. `contenteditable=""`/`"false"` —
  `crates/engine/layout/src/style.rs:7383` (`is_effectively_contenteditable`),
  consumed by `:read-write`/`:read-only` matching (`style.rs:7333`+).
- JS `contentEditable` / `isContentEditable` IDL props —
  `crates/js/src/dom.rs:4769`–4790 (backed by `_lumen_is_contenteditable` at `:2153`).

### Selection / Range API (PARTIAL)
- Rust types: `DomPosition` `crates/engine/dom/src/lib.rs:518`, `Range` `:531`
  (`is_collapsed` `:545`), `Selection` `:557` (`is_collapsed` `:566`, `get_range`
  `:575`). Document accessors `get_selection` `:1042` / `set_selection` `:1047`.
- JS bindings: `Range` interface `crates/js/src/dom.rs:4997`+, `Range()` ctor `:5117`,
  `Selection` singleton `:5121`+, `getSelection`/`createRange` `:5334`–5335,
  `window.getSelection` `:10394`. InputEvent `getTargetRanges` stub returns `[]`
  (`crates/js/src/dom.rs:2834`).
- **Gaps:** several `Range` methods are stubs — `isPointInRange` returns `false`
  (`crates/js/src/dom.rs:5108`), `getComposedRanges` returns `[]` (`:5184`),
  `compareBoundaryPoints` simplistic. Cross-node range ops not handled (see below).
- **Gap:** `selectionchange` event is **not** fired anywhere — grep finds only the
  string in comments. Nothing in the shell calls `doc.set_selection(...)` from user
  input (no match for `set_selection` in `crates/shell/src/main.rs`).

### Input Events Level 2 (PARTIAL — P1 half largely DONE)
- `EditInputType` enum (12 inputTypes, `as_str`) — `crates/engine/dom/src/lib.rs:2546`+.
- `InputEvent` struct (`input_type`, `data`, `is_composing`, `is_trusted`,
  `trusted`/`untrusted` ctors) — `crates/engine/dom/src/lib.rs:2600`+.
- JS `InputEvent` interface + `beforeinput`→mutation→`input` orchestrator
  `_lumen_handle_contenteditable_key(inputType, data, targetNid)` —
  `crates/js/src/dom.rs:5199`. Fires cancelable `beforeinput`, applies the mutation,
  fires non-cancelable `input`. Tests at `crates/js/src/dom.rs:18506`+.
- Mutation bindings called by the orchestrator: `_lumen_contenteditable_insert_text`
  `crates/js/src/dom.rs:2164`, `_delete_backward` `:2185`, `_delete_forward` `:2222`,
  `_insert_paragraph` `:2258` — all read/write `doc.get_selection()`.

### DOM mutation primitives (DONE)
- `split_text_node` — `crates/engine/dom/src/lib.rs:2803`.
- `insert_text_at` — `:2845` (resolves caret container to a text node).
- `delete_range` — `:2903` (**same-container only**; cross-node returns start
  unchanged — see Steps).
- `insert_paragraph_break` — `:2957` (splits + inserts `<br>`).
- Text helpers: `node_text_content` `:2986`, `node_child_count` `:2995`,
  `node_length` `:3004`.

### Command history / undo (DONE as a type, UNWIRED)
- `CommandHistory` with `insert_text`/`delete_range`/`replace_text`/`undo`/`redo`/
  `can_undo`/`can_redo` — `crates/engine/dom/src/contenteditable.rs:144`+.
- `DomCommand` enum `:10`, `paste_into` `:329`, `drop_into` `:361`,
  `PasteData` `:40`, `DragData` `:54`. Re-exported from `lib.rs:21`.
- **Gaps:** `delete_range`/`replace_text` store an **empty** `deleted_text`/`old_text`
  (`contenteditable.rs:180`, `:201` — `// TODO: extract actual text from range`), so
  undo of a deletion currently restores nothing. The history is **not instantiated
  anywhere** — the JS `_lumen_contenteditable_*` bindings mutate the doc directly,
  bypassing `CommandHistory`, so undo/redo is currently unreachable.

### IME (PARTIAL — events emitted, NOT routed to contenteditable)
- Rust composition types: `CompositionEventType` `crates/engine/dom/src/lib.rs:2651`,
  `CompositionData` `:2676`, `CompositionEvent` (+ `start`/`update`/`end` ctors) `:2703`+.
- Document composition state: `begin_composition` `:1469`+, plus
  `get_composition_target` / update / end (see tests `lib.rs:5503`+).
- Shell IME pipeline: `WindowEvent::Ime` `crates/shell/src/main.rs:8096` →
  `handle_ime` `:12427` → emits `Event::ImeComposition{Started,Updated,Ended}`
  (`crates/core/src/event.rs:137`–140).
- **Gap:** `handle_ime` only flips `self.ime_composing` and emits app events; nothing
  consumes `ImeCompositionEnded` to insert the committed text into the editable host
  via the DOM mutation path. Preedit is never shown in the editable region.

### Keyboard routing into contenteditable (PARTIAL)
- `crates/shell/src/main.rs:11742`–11810 routes plain (no Ctrl/Alt/Meta) keys to
  `_lumen_handle_contenteditable_key` when `find_editing_host(focused_node)` is set:
  Backspace→deleteContentBackward, Delete→deleteContentForward, Enter→insertParagraph,
  Shift+Enter→insertLineBreak, printable→insertText. Runs **before** global keybindings.
- **Gaps:** no mouse → caret placement (so `selection.anchor` is usually `None` and
  the insert bindings early-return), no Ctrl+Z/Y/C/X/V routing, no word-delete
  (Ctrl+Backspace) routing, no drag-drop.

### Automation (existing seam to reuse)
- `BrowserSession` editable selector `crates/driver/src/session.rs:693`:
  `input:not([type='hidden']), textarea, [contenteditable]` — type/paste automation
  already targets contenteditable; mentions of `type_text`/`paste`/`compose_text` in
  `InputEvent`/`CompositionEvent` doc comments are the intended trusted entry points.

---

## Architecture

```
                 ┌─────────────────────────── P4: input dispatch ───────────────────────────┐
  mouse click ──▶│ caret placement: hit-test layout → DomPosition → doc.set_selection()      │
  drag          ▶│ range selection (anchor=down, focus=move)                                  │
  key (plain)   ▶│ EXISTS main.rs:11742 → _lumen_handle_contenteditable_key                   │
  Ctrl+Z/Y      ▶│ historyUndo / historyRedo → CommandHistory.undo()/redo()                  │
  Ctrl+C/X/V    ▶│ clipboard ⇄ PasteData → paste_into / cut via deleteByCut                  │
  IME commit    ▶│ ImeCompositionEnded → insertText at caret; preedit overlay during compose │
  drag-drop     ▶│ DragData → drop_into at drop DomPosition                                   │
                 └───────────────────────────────────┬───────────────────────────────────────┘
                                                      ▼
        ┌──────────────────────── P1: DOM model + event firing ────────────────────────┐
        │ Selection/Range types (EXISTS) · selectionchange firing (NEW)                 │
        │ _lumen_handle_contenteditable_key: beforeinput → mutate → input (EXISTS)      │
        │ mutation primitives: insert_text_at / delete_range / insert_paragraph (EXISTS)│
        │   → extend delete_range to cross-node; record real text in CommandHistory     │
        └──────────────────────────────────────────────────────────────────────────────┘
```

**P1 owns** the DOM model and event firing: Selection/Range types (mostly done),
`selectionchange` dispatch, cross-node `delete_range`, real deleted-text capture in
`CommandHistory`, and the `beforeinput`/`input` orchestrator (done — extend for new
inputTypes only).

**P4 owns** turning real user input into calls against P1's model: mouse→caret,
drag→range, IME commit→insert, clipboard→paste/cut, drag-drop→drop, and Ctrl+Z/Y→
undo/redo. P4 does **not** touch the mutation primitives or the event-firing order.

---

## Team split

### P1 (DOM mutations + Selection/Range + event firing)
1. `selectionchange` event: fire on every `doc.set_selection(...)` that changes the
   selection (debounced to once per task per spec). Add a hook so the shell/IME path
   can trigger it without each call site re-implementing it.
2. Cross-node `delete_range` in `crates/engine/dom/src/lib.rs:2903` (currently
   same-container only). Needed for multi-node selection deletion and range paste.
3. Capture real deleted/old text in `CommandHistory::delete_range`/`replace_text`
   (`contenteditable.rs:180`, `:201`) using `range_text`/`node_text_content` so undo
   restores content.
4. Fill `Range` stubs that block correctness: `isPointInRange`
   (`crates/js/src/dom.rs:5108`), `compareBoundaryPoints`, `getTargetRanges` for
   `beforeinput` (`:2834`).
5. Extend `_lumen_handle_contenteditable_key` for new inputTypes as P4 routes them
   (`insertFromPaste`, `deleteByCut`, `deleteWord*`, `historyUndo`/`historyRedo`).

### P4 (input dispatch + undo stack)
1. **Mouse caret:** on click inside an editing host, hit-test the layout to a
   `DomPosition` and `doc.set_selection(Selection{anchor,focus})` collapsed; on drag,
   keep anchor at mouse-down, move focus on drag. This is the prerequisite that makes
   the existing insert bindings stop early-returning (they require `sel.anchor`).
2. **Undo stack wiring:** instantiate one `CommandHistory` per editing host (or per
   tab), route the existing mutation bindings *through* it, and bind Ctrl+Z →
   `historyUndo`, Ctrl+Y / Ctrl+Shift+Z → `historyRedo`.
3. **Clipboard:** Ctrl+C/X/V via `platform::clipboard` ⇄ `PasteData`/`DragData`;
   Ctrl+X = copy + `deleteByCut`; Ctrl+V = `paste_into`.
4. **IME:** consume `Event::ImeCompositionEnded` (`main.rs:12458`) to insert committed
   text via `insertText`; show preedit during `ImeCompositionUpdated`.
5. **Drag-drop:** on drop over an editing host, build `DragData` and call `drop_into`
   at the drop `DomPosition`.

---

## Entry points (real file:line — `[PROPOSED]` marks new code)

| Concern | Location | State |
|---|---|---|
| editable detection | `crates/engine/dom/src/lib.rs:1796`, `:1808` | EXISTS |
| Selection/Range types | `crates/engine/dom/src/lib.rs:518`, `:531`, `:557` | EXISTS |
| selection accessors | `crates/engine/dom/src/lib.rs:1042`, `:1047` | EXISTS |
| `selectionchange` firing | hook near `set_selection` `lib.rs:1047` | **[PROPOSED] P1** |
| mutation primitives | `crates/engine/dom/src/lib.rs:2803`, `:2845`, `:2903`, `:2957` | EXISTS |
| cross-node `delete_range` | `crates/engine/dom/src/lib.rs:2903` | **[PROPOSED] P1** (extend) |
| InputEvent / inputTypes | `crates/engine/dom/src/lib.rs:2546`, `:2600` | EXISTS |
| Composition types | `crates/engine/dom/src/lib.rs:2651`, `:2703` | EXISTS |
| CommandHistory / undo | `crates/engine/dom/src/contenteditable.rs:144` | EXISTS (real-text capture **[PROPOSED] P1**) |
| paste/drop helpers | `crates/engine/dom/src/contenteditable.rs:329`, `:361` | EXISTS |
| JS beforeinput/input orchestrator | `crates/js/src/dom.rs:5199` | EXISTS |
| JS mutation bindings | `crates/js/src/dom.rs:2164`, `:2185`, `:2222`, `:2258` | EXISTS |
| JS Selection/Range | `crates/js/src/dom.rs:4997`, `:5121`, `:5334` | PARTIAL (stubs `:5108`, `:5184`) |
| keyboard → contenteditable | `crates/shell/src/main.rs:11742` | PARTIAL (plain keys only) |
| Ctrl+Z/Y/C/X/V routing | near `main.rs:11742` / keybindings `:11812` | **[PROPOSED] P4** |
| mouse → caret/range | shell pointer handlers (no current `set_selection` call) | **[PROPOSED] P4** |
| IME → contenteditable | `crates/shell/src/main.rs:12427` (`handle_ime`), consume `Ended` `:12458` | **[PROPOSED] P4** |
| drag-drop → drop_into | shell drop handler | **[PROPOSED] P4** |
| clipboard backend | `crates/shell/src/platform/clipboard.rs` | EXISTS (reuse) |

---

## Steps

1. **P1 — selectionchange + Range stubs.** Add `selectionchange` dispatch on selection
   change; fill `isPointInRange`/`compareBoundaryPoints`/`getTargetRanges`. Tests in
   `crates/js/src/dom.rs` (Selection/Range block `:18173`+).
2. **P1 — cross-node delete + real undo text.** Extend `delete_range` to multi-node
   ranges; capture deleted/old text in `CommandHistory`. Unit tests in
   `crates/engine/dom/src/lib.rs` and `contenteditable.rs`.
3. **P4 — mouse caret/range.** Hit-test layout → `DomPosition`; set collapsed
   selection on click, range on drag. Unblocks the existing insert bindings.
4. **P4 — undo stack wiring.** Per-host `CommandHistory`; route mutations through it;
   bind Ctrl+Z/Y. Extend `_lumen_handle_contenteditable_key` for historyUndo/Redo.
5. **P4 — clipboard.** Ctrl+C/X/V via `platform::clipboard` ⇄ `PasteData`; wire
   `insertFromPaste` / `deleteByCut`.
6. **P4 — IME.** Route `ImeCompositionEnded` → `insertText`; render preedit during
   `ImeCompositionUpdated`.
7. **P4 — drag-drop.** Build `DragData` on drop, call `drop_into` at drop position.
8. **Docs.** Update `CAPABILITIES.md` (contenteditable / Selection / Input Events
   rows), `subsystems/dom.md`, and on completion `docs/plan/phases.md:128`. Regenerate
   `SYMBOLS.md` for any new public API.

---

## Tests

- **Unit (dom):** cross-node `delete_range`; `CommandHistory` undo restores deleted
  text; `split_text_node`/`insert_paragraph_break` edge cases. Extend existing block
  `crates/engine/dom/src/lib.rs:4054`+ and `contenteditable.rs:386`+.
- **Unit (js):** `selectionchange` fires on selection move; `getSelection()` reflects
  caret after insert; `beforeinput` cancel blocks mutation (precedent test
  `crates/js/src/dom.rs:18664`); paste/cut inputTypes mutate correctly. Extend
  `crates/js/src/dom.rs:18173`+ / `:18506`+.
- **Integration (shell):** click places caret; type → DOM text updates; Enter splits
  paragraph; Ctrl+Z/Y round-trip; IME commit inserts; Ctrl+V pastes; drag-drop moves.
- **Graphic test:** a `contenteditable` page exercising caret + typed text + paragraph
  break (text geometry only — glyph AA divergence ignored per test rules). Add to
  `graphic_tests/` with the magenta-frame pattern and register in `run.py`.

---

## Definition of done

- Clicking inside any `contenteditable` host places a caret; typing, Enter,
  Backspace/Delete, and word-delete mutate the DOM at the caret.
- `beforeinput` (cancelable) → mutation → `input` fires in spec order for every edit;
  canceling `beforeinput` blocks the mutation.
- IME composition shows preedit and commits committed text into the caret.
- Ctrl+C/X/V and drag-drop move HTML/plain-text content in and out of the host.
- Ctrl+Z/Y undo and redo through a per-host `CommandHistory` that restores real text.
- `window.getSelection()` / `document.createRange()` reflect the live caret/range and
  `selectionchange` fires when the selection moves.
- `cargo clippy -p lumen-dom -p lumen-js -p lumen-shell --all-targets -- -D warnings`
  clean; new unit + integration + graphic tests pass.
- `CAPABILITIES.md`, `subsystems/dom.md`, `docs/plan/phases.md:128`, and `SYMBOLS.md`
  updated in the same merge.
