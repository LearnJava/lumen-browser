# Ph3 — Spell check (Hunspell)

**Developer:** P3 + P4  
**Branch:** `ph3-spell-check`  
**Size:** L  
**Crates:** new `lumen-spell` crate (inside workspace), `lumen-core` (trait already exists), `lumen-paint` (squiggly render), `lumen-shell` (context menu suggestions + OS API integration)  
**Phase:** 3 (v1.0 target)

---

## Status

Phase 3 future item. **Greenfield** — no Hunspell parser or dictionary loader exists in the codebase. The trait anchor and null implementation are already in place:

- `SpellChecker` trait: `crates/core/src/ext.rs:1286`
- `NullSpellChecker` stub: `crates/core/src/ext.rs:1300`
- Tests for the null impl: `crates/core/src/ext.rs:2483`

The `SpellChecker` trait is **not wired** into the shell (`crates/shell/src/main.rs`) — `NullSpellChecker` is never instantiated or referenced outside `lumen-core`. The squiggly underline render primitive (`emit_wavy_line`) exists in paint but is not accessible as a public API for spell errors. The context menu in shell is the tab strip right-click only (`crates/shell/src/tabs/context_menu.rs`) — there is no page-level right-click context menu.

---

## Goal

Provide spell checking for user-editable text (`<input>`, `<textarea>`, `contenteditable`):

1. **Russian dictionary mandatory** — `ru_RU` (Hunspell format, `.aff` + `.dic`) bundled in the binary.
2. **English dictionary** — `en_US` bundled alongside Russian.
3. Misspelled words show a red wavy underline (squiggly) while the user types.
4. Right-clicking a underlined word opens an inline suggestion menu with correction candidates and "Add to dictionary" / "Ignore".
5. OS spell-check API (Windows 10+ `ISpellChecker`, macOS `NSSpellChecker`) integrated as an optional provider (falls back to Hunspell if unavailable).
6. **Principle #7 ("Russian first-class")** — Russian must work out of the box without any user configuration.

---

## Current state

### Trait anchor (`lumen-core`)

| Symbol | File | Line | Status |
|---|---|---|---|
| `SpellChecker` trait | `crates/core/src/ext.rs` | 1286 | Live — `check()`, `suggest()`, `locale()` |
| `NullSpellChecker` | `crates/core/src/ext.rs` | 1300 | Live — always returns `true`/empty |
| Null impl tests | `crates/core/src/ext.rs` | 2483 | Live |

### Text entry points (`lumen-shell`)

The text that needs checking flows through these sites:

| Symbol | File | Line | Notes |
|---|---|---|---|
| `inject_char()` | `crates/shell/src/main.rs` | 11495 | Called on every typed character; fires JS key events on `focused_node` |
| `focused_node` field | `crates/shell/src/main.rs` | 5480 | `Option<NodeId>` of the currently focused element; set on click |
| `form_state` field | `crates/shell/src/main.rs` | 5362 | `HashMap<NodeId, FormControlState>` — `.value: String` is the full current text of each input |
| `FormControlState.value` | `crates/shell/src/forms.rs` | 32 | The accumulated string the user typed |
| `TypeText` injection path | `crates/shell/src/main.rs` | 7595 | Alternative entry via `InputCommand` API |

After each `inject_char`, the value in `form_state[focused_node].value` reflects the current buffer. This is the string the spell checker reads.

### Wavy underline render primitive (`lumen-paint`)

| Symbol | File | Line | Notes |
|---|---|---|---|
| `emit_wavy_line(out, x, y, width, thickness, color)` | `crates/engine/paint/src/display_list.rs` | 6430 | Private `fn` — emits FillRect columns approximating a sine wave |
| `TextDecorationStyle::Wavy` dispatch | `crates/engine/paint/src/display_list.rs` | 6370 | CSS `text-decoration: underline wavy` already calls `emit_wavy_line` |
| `TextDecorationStyle` enum | `crates/engine/layout/src/style.rs` | 524 | `Solid`, `Double`, `Dotted`, `Dashed`, `Wavy` |

`emit_wavy_line` is not `pub`. P4 must either expose it as `pub(crate)` / `pub` or add a dedicated `DisplayCommand::SpellSquiggle { rect, color }` variant that the renderer interprets identically. The `DisplayCommand::FillRect`-based wavy approach is already proven by the CSS `text-decoration: underline wavy` tests.

### Context menu (`lumen-shell`)

| Symbol | File | Line | Notes |
|---|---|---|---|
| `TabContextMenu` struct | `crates/shell/src/tabs/context_menu.rs` | full file | Tab strip right-click only; no page-level right-click menu exists |
| Right-click handler | `crates/shell/src/main.rs` | 8423 | Dispatches to tab context menu or mouse gesture; **no page content path** |

A **page-level context menu** does not exist yet — it must be created as part of this task. The tab context menu (`context_menu.rs`) is a good structural reference for geometry + `DisplayList` rendering pattern. The right-click dispatch at `main.rs:8423` is the integration point: after tab hit-testing fails, the right-click must hit-test the page content for a word under the cursor and open the spell suggestion menu if the word is misspelled.

### Dictionary storage (`lumen-shell` / new crate)

The portable data directory pattern from ad-block:

| Symbol | File | Line | Notes |
|---|---|---|---|
| `browser_data_dir()` | `crates/shell/src/adblock.rs` | 44 | `<exe_dir>/data` |
| `adblock_dir()` | `crates/shell/src/adblock.rs` | 53 | `<exe_dir>/data/adblock` |

Dictionary data should follow the same convention: `<exe_dir>/data/spell/<locale>/`. User-added words go to `<exe_dir>/data/spell/user_words.txt`.

---

## Architecture

### P3 responsibility — dictionary loader + Hunspell parser + check API

**New crate: `lumen-spell`** (add to `Cargo.toml` workspace).

```
crates/spell/
  Cargo.toml
  src/
    lib.rs          — public API: SpellCheckerImpl, load_bundled(), load_path()
    aff.rs          — .aff file parser (FLAG, PFX, SFX, COMPOUNDRULE, encoding)
    dic.rs          — .dic file parser (root word list, affix flags)
    expand.rs       — affix expansion: PFX/SFX table lookup → root word forms
    checker.rs      — SpellChecker trait impl: check() + suggest() via HashSet + edit-distance
    user_dict.rs    — per-user word list (plain text, one word/line, portable path)
  assets/           — bundled dictionaries (included via include_bytes!)
    ru_RU.aff
    ru_RU.dic
    en_US.aff
    en_US.dic
  tests/
    aff_parser.rs
    dic_parser.rs
    checker_ru.rs   — integration: check real Russian words
    checker_en.rs   — integration: check real English words
    suggestions.rs  — suggest() quality: known misspelling → expected correction
```

**Hunspell format notes** (hand-rolled parser, no external crate — see Dependencies):

- `.aff`: UTF-8 or ISO-8859-1 (encoding declared in first line `SET`). Parse `PFX`/`SFX` tables line by line; `FLAG` type (`long`, `num`, or default single-char). `COMPOUNDRULE` is deferred.
- `.dic`: first line = word count (hint, not enforced). Each line: `word/FLAGS` or bare `word`. Strip trailing `/` comment. Build `HashSet<String>` of expanded forms.
- **Affix expansion strategy:** expand all roots at load time into a flat `HashSet<String>` (memory: ~5–15 MB for Russian). No runtime trie needed for `check()`; only `suggest()` needs the affix table for generating candidates.
- **`suggest()` algorithm:** generate candidates by single-edit distance (insert/delete/transpose/substitute) from the input word, filter against the `HashSet`. Limit to top-5.

**`SpellChecker` impl:**

```rust
pub struct HunspellChecker {
    words: HashSet<String>,     // all expanded forms
    locale: String,             // "ru-RU" | "en-US"
    user_words: HashSet<String>,
    // aff data retained only for suggest()
    aff: AffData,
}

impl lumen_core::ext::SpellChecker for HunspellChecker { … }
```

**Load API:**

```rust
/// Load the bundled dictionary for `locale` ("ru-RU" or "en-US").
/// Falls back to NullSpellChecker if locale not bundled.
pub fn load_bundled(locale: &str) -> Box<dyn SpellChecker>;

/// Load from explicit .aff + .dic paths (for user-supplied dictionaries, Phase 3+).
pub fn load_path(aff: &Path, dic: &Path, locale: &str) -> Result<HunspellChecker, SpellError>;
```

**User dictionary** (`user_dict.rs`):

```rust
pub struct UserDict {
    path: PathBuf,     // <exe_dir>/data/spell/user_words.txt
    words: HashSet<String>,
}
impl UserDict {
    pub fn load(data_dir: &Path) -> Self;
    pub fn add(&mut self, word: &str) -> std::io::Result<()>;  // appends to file
    pub fn contains(&self, word: &str) -> bool;
}
```

### P4 responsibility — squiggly render + context menu + OS API

#### Squiggly underline render

P4 must add a way to draw spell-error squiggles on inline text runs. Two options; pick one:

**Option A (preferred): new `DisplayCommand` variant**

```rust
// In crates/engine/paint/src/display_list.rs
/// Red wavy underline for spell-check errors.
/// `rect` is the bounding box of the misspelled word (border-box coordinates).
/// Rendered identically to `text-decoration: underline wavy` in red.
SpellSquiggle { rect: Rect },
```

The renderer emits the existing `emit_wavy_line` logic for this variant. Spell errors are **not** part of the CSS cascade — always red (`#cc0000`), same thickness as `text-decoration`.

**Option B:** expose `emit_wavy_line` as `pub(crate)` and call it from a new `emit_spell_errors()` function in the same file.

#### Shell integration — where P4 wires the checker

1. **`lumen-shell/src/main.rs`** — add `spell_checker: Arc<dyn SpellChecker>` field; initialize with `lumen_spell::load_bundled("ru-RU")` at startup (proposed, line ~600).

2. **After `inject_char` / `TypeText`** (`main.rs:11495` / `7595`) — on each keystroke, extract the current word token around the cursor from `form_state[focused_node].value`, call `spell_checker.check(word)`. Store `Vec<(word_offset, word_len)>` of misspelled ranges on the `FormControlState` or a parallel map.

3. **Paint pass** — when rendering a focused text input, emit `SpellSquiggle` (or call `emit_wavy_line`) for each misspelled word range. This requires knowing the pixel `x` of each word inside the input box — use `lumen-font` metrics or the layout engine's inline fragment positions.

4. **Page-level context menu** — new file `crates/shell/src/page_context_menu.rs` (proposed). Mirrors the structure of `tabs/context_menu.rs`: `PageContextMenu` struct, `build_overlay()`, `item_at()`, `action_at()`. Actions: `MenuAction::UseSuggestion(String)`, `MenuAction::AddToDict`, `MenuAction::IgnoreAll`.

5. **Right-click dispatch** (`main.rs:8423`) — after the tab hit-test, if the click lands in the page content area and the word under the cursor is misspelled, open `PageContextMenu` with suggestions from `spell_checker.suggest(word)`. Applying a suggestion calls `forms::set_value` and forces relayout.

#### OS spell-check API integration (proposed, optional for first shipping)

| Platform | API | Notes |
|---|---|---|
| Windows 10+ | `ISpellChecker` (WinRT `Windows.Data.Text.TextPredictionGenerator`) | Via `windows` crate (already in dep graph or add provisional) |
| macOS | `NSSpellChecker` | Via `objc2` (provisional) |
| Linux | `enchant-2` library | Via FFI or `enchant` crate (provisional) |

P4 implements as a second `SpellChecker` impl that delegates to the OS. Shell selects: OS checker if available, else `lumen-spell` Hunspell, else `NullSpellChecker`. The selection logic belongs in `shell/src/main.rs` initialization.

---

## Team split

| Step | Owner | Deliverable |
|---|---|---|
| 1. New `lumen-spell` crate skeleton | P3 | `Cargo.toml` + `src/lib.rs` stub + workspace registration |
| 2. `.aff` / `.dic` parser | P3 | `aff.rs`, `dic.rs`, parser tests |
| 3. Affix expansion + `HashSet` build | P3 | `expand.rs`, integration test on real Russian words |
| 4. `SpellChecker` impl (`check` + `suggest`) | P3 | `checker.rs`, `checker_ru.rs` + `checker_en.rs` tests |
| 5. Bundle dictionaries + `load_bundled()` | P3 | `assets/`, `include_bytes!` macro |
| 6. User dictionary | P3 | `user_dict.rs`, portble path via `browser_data_dir()` |
| 7. `SpellSquiggle` display command / `emit_wavy_line` exposure | P4 | Paint change |
| 8. Shell wiring: checker field + per-keystroke check | P4 | `main.rs` changes |
| 9. Squiggle emit in paint pass for focused inputs | P4 | Paint/shell integration |
| 10. `PageContextMenu` + right-click dispatch | P4 | New `page_context_menu.rs` |
| 11. OS API integration | P4 | Windows `ISpellChecker` first; macOS/Linux deferred |
| 12. Graphic test | P4 | New `graphic_tests/NN-spell-squiggle.html` + COVERAGE.md entry |

---

## Entry points

### Existing (real file:line)

| File | Line | Relevance |
|---|---|---|
| `crates/core/src/ext.rs` | 1286 | `SpellChecker` trait — P3 implements `HunspellChecker` against this |
| `crates/core/src/ext.rs` | 1300 | `NullSpellChecker` — remains as fallback |
| `crates/shell/src/main.rs` | 11495 | `inject_char()` — P4 hooks spell check here after every char |
| `crates/shell/src/main.rs` | 7595 | `TypeText` path — same hook needed here |
| `crates/shell/src/main.rs` | 5480 | `focused_node` field — identifies which element to read value from |
| `crates/shell/src/main.rs` | 5362 | `form_state: forms::FormState` — source of `.value` string |
| `crates/shell/src/forms.rs` | 32 | `FormControlState.value` — the text buffer to check |
| `crates/shell/src/main.rs` | 8423 | Right-click handler — P4 adds page context menu dispatch here |
| `crates/engine/paint/src/display_list.rs` | 6430 | `emit_wavy_line` — P4 exposes or wraps for spell squiggles |
| `crates/engine/paint/src/display_list.rs` | 6370 | Wavy dispatch in CSS text-decoration — reference for new variant |
| `crates/engine/layout/src/style.rs` | 524 | `TextDecorationStyle` — reference only; spell errors bypass CSS cascade |
| `crates/shell/src/tabs/context_menu.rs` | 1 | Reference for page context menu implementation |
| `crates/shell/src/adblock.rs` | 44 | `browser_data_dir()` — reuse for `<exe_dir>/data/spell/` |

### Proposed (new files / fields)

| File | Notes |
|---|---|
| `crates/spell/` | New crate — P3 creates |
| `crates/shell/src/main.rs` | Add `spell_checker: Arc<dyn SpellChecker>` field (~line 600 init block) |
| `crates/shell/src/main.rs` | Add `misspelled: HashMap<NodeId, Vec<(usize, usize)>>` for word ranges |
| `crates/shell/src/page_context_menu.rs` | New file — P4 creates, modelled on `tabs/context_menu.rs` |
| `crates/engine/paint/src/display_list.rs` | Add `DisplayCommand::SpellSquiggle { rect: Rect }` variant |

---

## Steps

1. **P3: Bootstrap `lumen-spell` crate**
   - `cargo new --lib crates/spell` — add to workspace `[members]`.
   - Add `lumen-core` as dependency (for `SpellChecker` trait).
   - Write empty stubs for all modules; `cargo check -p lumen-spell` must pass.

2. **P3: Download and bundle dictionaries**
   - Obtain `ru_RU.aff` + `ru_RU.dic` (LibreOffice / Mozilla licensed OSI dictionaries, LGPL or MPL).
   - Obtain `en_US.aff` + `en_US.dic` (hunspell-en-us, LGPL).
   - Place under `crates/spell/assets/`. Wire `include_bytes!` in `lib.rs`.
   - Commit a `docs/decisions/ADR-NNN-spell-dictionaries.md` recording the license of the bundled dictionaries (required before ship).

3. **P3: `.aff` parser** (`aff.rs`)
   - Parse `SET`, `FLAG`, `PFX`/`SFX` tables (key headers only; COMPOUNDRULE deferred).
   - Unit tests with synthetic minimal `.aff` content.
   - Integration test: parse real `ru_RU.aff` without panic.

4. **P3: `.dic` parser + expansion** (`dic.rs`, `expand.rs`)
   - Parse word list, expand all forms via affix table → `HashSet<String>`.
   - Integration test: `"кот"`, `"коту"`, `"коте"` all present after expand of Russian dict.

5. **P3: `HunspellChecker` implementation**
   - `check()`: lowercase input, lookup in `words ∪ user_words`.
   - `suggest()`: single-edit distance candidates filtered through `words`, limit 5, sort by edit distance.
   - Tests: `checker_ru.rs` (known Russian misspellings), `checker_en.rs` (English).

6. **P3: User dictionary** (`user_dict.rs`)
   - Load/persist `<exe_dir>/data/spell/user_words.txt`.
   - `add()` appends one line; file created on first add.

7. **P4: `DisplayCommand::SpellSquiggle`**
   - Add variant to `crates/engine/paint/src/display_list.rs` enum (proposed).
   - In `walk()` / femtovg backend: render via same logic as `emit_wavy_line`, color `#cc0000`.
   - Unit test: `SpellSquiggle` emits multiple `FillRect` columns (mirrors `style_wavy_emits_sampled_columns` test at line 7787).

8. **P4: Shell wiring**
   - Add `spell_checker` field to the main app struct; initialize with `lumen_spell::load_bundled("ru-RU")`.
   - After `inject_char()` (`main.rs:11495`) and the `TypeText` branch (`main.rs:7595`): re-tokenize the word at the cursor in `form_state[focused_node].value`, call `spell_checker.check()`, update `misspelled[focused_node]`.
   - In the paint/render pass for focused inputs: for each `(offset, len)` in `misspelled[focused_node]`, compute the pixel rect of that word run and emit `SpellSquiggle`.

9. **P4: Page context menu**
   - Create `crates/shell/src/page_context_menu.rs` modelled on `crates/shell/src/tabs/context_menu.rs`.
   - `PageContextMenu::open_for_word(suggestions: Vec<String>, ...)` → `build_overlay()` shows suggestions + "Add to dictionary" + "Ignore".
   - Wire into right-click dispatch at `main.rs:8423`: if clicked node is a focused input and word under cursor is misspelled → open page context menu; applying a suggestion calls `forms::set_value` + relayout.

10. **P4: OS API (Windows first)**
    - Add `windows` crate (provisional) or use existing if present.
    - `WindowsSpellChecker` wraps `ISpellChecker`; at startup try to create it for `"ru-RU"` and `"en-US"`.
    - If successful, use instead of `lumen-spell`. Fall back to Hunspell otherwise.

11. **P4: Graphic test**
    - New `graphic_tests/NN-spell-squiggle.html`: a text input pre-filled with a misspelled Russian word, rendered so the squiggle is visible without interaction.
    - Add to `TESTS` dict in `graphic_tests/run.py`.
    - Add row to `graphic_tests/COVERAGE.md`.

---

## Dependencies

### `lumen-spell` crate — hand-rolled Hunspell parser, no external crate

**Rationale:** The Hunspell `.aff`/`.dic` format is well-documented and the subset needed (PFX/SFX expansion, word lookup, single-edit suggestions) is implementable in ~500 lines. External options:

| Crate | Problem |
|---|---|
| `hunspell-rs` | C FFI to `libhunspell` — LGPL, requires dynamic linking or bundling a C library; increases binary size; cross-compilation friction on Windows MSVC toolchain |
| `spellbook` (pure Rust) | Promising but unstable API (no 1.0); last release 2023; incomplete `suggest()` for Russian |
| `zspell` | Actively maintained pure-Rust Hunspell; consider as provisional if hand-rolled parser proves too costly |

**Decision for Ph3:** implement a minimal hand-rolled parser covering PFX/SFX + word list. If the parser grows beyond 1,500 lines or cannot handle the bundled Russian dictionary correctly, switch to `zspell` (provisional, graduation criterion: handles `ru_RU.aff` without errors).

### OS spell-check API dependencies (provisional)

| Crate | Platform | Note |
|---|---|---|
| `windows` (feature `Windows_Data_Text`) | Windows | Provisional; graduation: ships with Windows 10+ without extra install |
| `objc2` + `AppKit` | macOS | Provisional; Phase 3+ only |

---

## Tests

### Unit tests (in `lumen-spell`)

- `.aff` parser: minimal synthetic `PFX`/`SFX` tables round-trip.
- `.dic` parser: word + flags parsed, bare word parsed.
- Affix expansion: known Russian root → expected inflected forms present in `HashSet`.
- `check()`: known correct word returns `true`; known misspelling returns `false`.
- `suggest()`: `"ошибак"` → includes `"ошибок"` or similar; `"teh"` → includes `"the"`.
- User dict: `add()` persists to file; `check()` returns `true` after add.

### Integration tests (real dictionaries, in `lumen-spell/tests/`)

- `checker_ru.rs`: load bundled `ru_RU`, check 20 correct Russian words, check 10 known misspellings.
- `checker_en.rs`: same for `en_US`.
- `suggestions.rs`: `suggest("спасибо")` returns empty (correct word); `suggest("сапасибо")` returns non-empty.

### Paint tests (in `lumen-paint`)

- `SpellSquiggle` variant produces multiple `FillRect` columns (mirrors `style_wavy_emits_sampled_columns` at `crates/engine/paint/src/display_list.rs:7787`).
- Squiggle color is red (`#cc0000`), not inheriting from text color.

### Shell tests (in `lumen-shell`)

- `inject_char` after typing a misspelled word populates `misspelled[focused_node]` with the word range.
- Applying a context menu suggestion via `MenuAction::UseSuggestion` calls `set_value` and clears the misspelled range.

---

## Definition of done

- [ ] `lumen-spell` crate compiles with `cargo check -p lumen-spell`.
- [ ] Bundled `ru_RU` and `en_US` dictionaries load without error at startup.
- [ ] `HunspellChecker::check("привет")` returns `true`; `check("превет")` returns `false`.
- [ ] `HunspellChecker::suggest("превет")` includes `"привет"` or a sensible candidate.
- [ ] Typing a misspelled Russian word in a focused `<input>` shows a red wavy underline.
- [ ] Right-clicking the underlined word shows a suggestion menu with up to 5 corrections.
- [ ] "Add to dictionary" persists the word to `<exe_dir>/data/spell/user_words.txt`; the underline disappears.
- [ ] OS spell-check API used on Windows 10+ when available.
- [ ] `cargo clippy -p lumen-spell --all-targets -- -D warnings` passes.
- [ ] `cargo clippy -p lumen-shell --all-targets -- -D warnings` passes.
- [ ] `cargo test -p lumen-spell` passes (including real-dictionary integration tests).
- [ ] Graphic test `NN-spell-squiggle.html` passes at ≤ 0.5% diff threshold.
- [ ] `CAPABILITIES.md` updated (spell check ✅).
- [ ] `CSS-SPECS.md` not affected (spell check is not a CSS property).
- [ ] `SYMBOLS.md` regenerated (`python scripts/gen_symbols.py`).
- [ ] ADR filed for dictionary license choice.
