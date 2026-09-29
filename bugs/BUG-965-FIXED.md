# BUG-965: headless driver (`--mcp-port`/`--mcp`) never populates `scroll_states` — `scrollWidth`/`scrollHeight`/`scrollLeft`/`scrollTop` always wrong for scroll containers

**Статус:** FIXED 2026-09-29 (P6, не воспроизводится)
**Дата:** 2026-09-03
**Компонент:** js (`crates/js/src/v8_runtime/install/platform.rs::_lumen_get_scroll_state`) /
driver (`crates/driver/src/session.rs::relayout`)
**Найден:** P3, side discovery while live-verifying [BUG-504](BUG-504-FIXED.md)'s fix

## Проверка 2026-09-29 (P6)

Живая проверка обоих заявленных симптомов на текущем `main` их не воспроизвела:

- `scrollWidth` асимметричного бордера (`border-width:0 0 50px 80px`, `width:200px`)
  читает **200** (padding-box), не 280 (border-box).
- `scrollTop` после JS `el.scrollTo(0, 50)` читает **50**, не 0.

Причина — `_lumen_get_scroll_state` (`crates/js/src/v8_runtime/install/platform.rs`)
безусловно зовёт `flush.maybe_flush()` перед каждым чтением
(`crates/js/src/v8_runtime/style_flush.rs::FlushHandles::maybe_flush`), а тот
пересчитывает `scroll_states` с нуля из свежего layout и переносит на него
предыдущий известный JS-side scroll-offset (`prev_scroll`) — независимо от
того, звал ли embedder (driver или shell) `update_scroll_states` вообще. Этот
механизм — общий код `crates/js`, появился позже подачи бага (после
2026-09-03) как часть последующей работы над CSSOM-4/lazy-flush.

`grep -rn "update_scroll_states" crates/driver/src/` по-прежнему не находит ни
одного вызова в этом крейте, но это больше не имеет значения: чтение геометрии
скролла в headless-сессии больше не зависит от явного push из
`InProcessSession::commit_layout`.

Добавлены 2 регрессионных теста в `crates/driver/src/session.rs`:
`js_scroll_width_reads_padding_box_not_border_box_headless`,
`js_scroll_top_reflects_prior_js_scroll_to_headless`.

Попутно найден смежный, но отдельный дефект: `InProcessSession::scroll()`
(нативный Rust-side скролл, минуя JS) теряется на следующем relayout, потому
что `layout_and_commit` строит полностью новый `LayoutBox`-дерево без переноса
`scroll_x`/`scroll_y` — заведён как [BUG-1215](BUG-1215-OPEN.md), не в скоуп
этой задачи.

## Механизм

`window.getComputedStyle`-adjacent scroll geometry (`Element.scrollWidth`/
`scrollHeight`/`scrollLeft`/`scrollTop`) is served by the JS-side getters in
`web_api_shim_mid.js`, which read a Rust-side map populated by
`V8JsRuntime::update_scroll_states` (`crates/js/src/v8_runtime/runtime.rs:540`):
if a node has no entry in that map, `scrollLeft`/`scrollTop` silently return
`0` and `scrollWidth`/`scrollHeight` fall back to the node's **border-box**
size (`_lumen_get_bounding_rect`) — a deliberate, documented fallback for
elements that aren't scroll containers at all (see the comment above
`get scrollWidth()` in the shim), but wrong for an actual `overflow: auto`/
`scroll` container, whose `scrollWidth`/`scrollHeight` must reflect the real
scrollable-overflow extent (padding-box floor, [BUG-504](BUG-504-FIXED.md)),
and whose `scrollLeft`/`scrollTop` must reflect the real current offset, not
a hardcoded zero.

The map is written by exactly three call sites, all in `crates/shell/`
(the live-window/GUI path): `crates/shell/src/relayout.rs:696`,
`crates/shell/src/app/about_to_wait.rs:1382`, and
`crates/shell/src/lumen/scrolling.rs:139`/`:246`. `crates/driver/src/session.rs`
— the crate backing `--mcp-port`/`--mcp` (`InProcessSession`, DEVX-5) — has
**zero** call sites (`grep -rn "update_scroll_states" crates/driver/src/`
returns nothing); its own `relayout()` (`session.rs:466`) computes
`layout_root`/`flat_tree` and stops, never touching the JS runtime's scroll
state at all. Same for `crates/mcp/src/` (zero matches). Confirmed live: a
page with `#s1 { overflow: auto; width: 200px; height: 200px; border-width:
0 0 50px 80px; }` (no children, so `scrollWidth` should read the 200px
padding-box) driven via `--mcp-port` + `tools/call eval` reads
`scrollWidth === 280` — the *border*-box size, i.e. the border-box fallback
path, meaning the container is invisible to `_lumen_get_scroll_state`
entirely despite `collect_scroll_containers` correctly listing it.

## Симптом

Any script driving the browser via `--mcp-port`/`--mcp` (headless,
`InProcessSession`) that reads `el.scrollWidth`/`scrollHeight`/`scrollLeft`/
`scrollTop` on an `overflow: auto`/`scroll` element gets a wrong answer:
size fields report the border-box (ignoring padding, and ignoring any real
overflow extent — so a container that visibly overflows can report
`scrollWidth === clientWidth`-ish border-box with no indication of
overflow), and offset fields always read `0` regardless of any prior
`scrollTo`/`scrollBy` call (the write side, `_lumen_request_scroll`, still
queues correctly — only the read-back is affected, and headless sessions
don't process the scroll-request queue against the JS-visible map anyway).

## Масштаб

Affects every headless MCP script/probe (`scripts/*.py` using
`mcp_rpc_factory` against `--mcp-port`, not `--mcp-live-port`) that asserts
on scroll geometry — not observed via WPT (wptrunner drives the browser
through BiDi against the **live window** per `docs/tasks/p2-wpt-integration.md`,
which does go through `crates/shell`'s call sites and therefore isn't
affected by this specific gap). Not yet surveyed whether any committed
script currently relies on headless scroll geometry and is silently getting
wrong numbers.

## Что нужно

`crates/driver/src/session.rs::relayout` (and any other path that commits a
fresh `layout_root` in `InProcessSession`) needs to call
`lumen_layout::collect_scroll_containers(&layout_root)` and push the result
into the JS runtime the same way `crates/shell/src/relayout.rs:676-696`
does, so `--mcp-port`'s persistent V8 runtime (`crates/js/src/v8_runtime.rs`,
already shared code with the live window) sees a populated map after every
layout pass.
