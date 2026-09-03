# BUG-510: CSS tokenizer's whitespace skip uses Unicode `White_Space`
instead of the CSS Syntax 5-character definition

**Статус:** FIXED 2026-09-03
**Дата:** 2026-08-02
**Компонент:** css-parser (`crates/engine/css-parser/src/parser.rs::skip_ws_and_comments`)
**Найден:** WPT-RUN-3 срез 18 (`ROADMAP.md`) — массовый прогон `css/css-syntax`

## Механизм

`skip_ws_and_comments` (`parser.rs:2437`) treats any character for which
Rust's `char::is_whitespace()` returns `true` as CSS whitespace
(`parser.rs:2440`). That method follows the Unicode `White_Space` property —
dozens of codepoints (NEL, various fixed/variable-width spaces, line/
paragraph separators, …). The CSS Syntax spec instead defines "whitespace" as
exactly [5 ASCII characters](https://drafts.csswg.org/css-syntax-3/#whitespace):
U+0009 TAB, U+000A LF, U+000C FF, U+000D CR, U+0020 SPACE — every other
Unicode space-like character is an ordinary token character (e.g. part of an
identifier, or on its own an invalid token) and must **not** act as a
combinator/separator.

Confirmed by test `css/css-syntax/whitespace.html`: it probes 26 non-CSS
"looks like whitespace" codepoints via `.a<char>b` selector matching. 24 of
26 correctly fail to match (so evidently most of these codepoints already
don't round-trip as an identifier character elsewhere and the selector never
matches at all — a different, unrelated failure mode that happens to produce
the spec-correct outcome). The 2 exceptions are exactly the 2 candidates that
Rust's `is_whitespace()` additionally recognizes on top of the CSS 5: U+000B
VERTICAL TAB and U+0085 NEXT LINE (NEL) — both get skipped by
`skip_ws_and_comments` as if they were the descendant combinator, so
`.a\x0Bb` and `.a\x{2029}b`-style selectors match the same element a real
`.a b` would, when the correct behavior is either no match or (per the test's
own tolerance) a thrown `SyntaxError`.

## Масштаб находки

1 file / 2 subtests (`css/css-syntax/whitespace.html`, "U+000b is *not* CSS
whitespace" / "U+0085 is *not* CSS whitespace"). Narrow in test count, but a
real spec-conformance gap in the shared tokenizer — any selector or CSS value
containing a literal U+000B or U+0085 byte is affected, not just this test.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-syntax/` for
`whitespace.html` (2 subtests `expected: FAIL`, the other 29 pass).

## Срез 2026-09-03 (P3): fixed

`skip_ws_and_comments` (`crates/engine/css-parser/src/parser.rs`) now matches
the CSS Syntax 5-character set explicitly (`matches!(c, '\t' | '\n' | '\x0C'
| '\r' | ' ')`) instead of `char::is_whitespace()`. Confirmed via the exact
mechanism the WPT test probes: a new regression test,
`valid_selector_list_rejects_non_css_whitespace_as_combinator`
(`crates/engine/css-parser/src/parser/tests/selectors.rs`), asserts
`is_valid_selector_list(".a\u{000B}b")`/`".a\u{0085}b"` are now `false` —
the compound-selector parse breaks on the un-skipped byte instead of treating
it as a descendant combinator, matching the test's "isn't valid in a selector
at all" tolerance branch. `cargo test -p lumen-css-parser`: 360/360.
`cargo clippy -p lumen-css-parser --all-targets -- -D warnings`: clean.

**Live WPT verification not performed** — no `tests/wpt/.venv` in this pool
slot. `.ini` deleted (`tests/wpt/metadata/css/css-syntax/whitespace.html.ini`)
— the file's only 2 `expected: FAIL` entries directly encode this bug's
mechanism, which the fix eliminates at the tokenizer level (the single shared
`skip_ws_and_comments` all selector/value parsing goes through), so no
uncertainty remains for a future session to re-derive.
