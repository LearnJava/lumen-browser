# BUG-934 — automation `Click`/`Type` never resolve into engine chrome (toolbar, tabs, sidebar), only into page content

**Статус:** FIXED 2026-09-20 (P3)
**Компонент:** shell (`crates/shell/src/lumen/automation.rs::resolve_automation_target`,
`crates/shell/src/lumen/click.rs::handle_click_at`/`handle_click_at_inner`,
`crates/shell/src/chrome_ui.rs::try_dispatch_chrome_click`)
**Заведён:** 2026-09-01 (P3), при срезе 53 BUG-405 — остаток срезов 51/52
("автоматизационный пробел... остаётся незаведённым отдельным тикетом")
оформлен отдельной записью, как и было запланировано.

## Исправление 2026-09-20 (P3)

Точечный фикс без правки `handle_click_at_inner` (страничного пути — он
остался нетронутым). Блок «CC-5 chrome hit-test + `data-action` dispatch»,
раньше живший только внутри `on_mouse_input`'s `ElementState::Pressed`-ветки
(`mouse_input.rs:333-370`), вынесен как есть в новый общий метод
`Lumen::try_dispatch_chrome_click(x_css, y_css, event_loop) -> bool`
(`chrome_ui.rs`) — `point_over_chrome` → `chrome_hit_test` →
omnibox-спецкейс/`chrome_action_at`+`dispatch_chrome_action`, возвращает
`true`, когда точка была над chrome (обработана здесь). `mouse_input.rs`
теперь просто зовёт его и возвращает при `true` — поведение реального мышиного
клика байт-в-байт то же, что было (тот же код, тот же порядок вызовов).

Шаринг с automation безопасен именно потому, что весь этот блок диспатчит
chrome-действие СИНХРОННО на press — секция «Why not a quick point-fix» ниже
верна для `handle_click_at_inner` (страничного press/release-состояния), но
не относится к chrome-ветке: там нет парного `Released`-хвоста, который
нужно было бы синтезировать отдельно для одного синтетического
automation-вызова.

`about_to_wait.rs`: `AutomationCommand::Click`/`::Type` теперь сначала
пробуют `try_dispatch_chrome_click(resolved.x, resolved.y, event_loop)` —
при `true` шлют `Ack` и не идут в прежний `automation_hit_mismatch` +
`handle_click_at` путь (тот остаётся для `Target::Selector`/`Target::NodeId`,
которые в chrome-документ в принципе не резолвятся, и для `Target::Point` на
странице). `AutomationCommand::Type` после открытия адресной строки чере
chrome-клик вводит текст через `address_bar.append_str` +
`query_omnibox_suggestions` (тот же путь, что `handle_address_bar_key` —
`query_omnibox_suggestions` расширен с приватного до `pub(crate)`), а не
через `inject_char`, который адресует только сфокусированный узел страничного
DOM и ничего не знает про chrome-документ.

Гейт: `cargo clippy --workspace --all-targets -- -D warnings` чист;
`scripts/scoped-test.sh` — единственный красный таргет
`-p lumen-driver --test all` (`cases::snapshot_cpu::cpu_snapshots_match_references`,
BUG-1008 — посторонний дрейф CPU-эталонов, тот же набор из 7 файлов
(`55-text-rendering`/`57-canvas-2d`/`32-list-markers`/`34-forms`/
`45-multiple-backgrounds`/`51-scrollbar-rendering`/`1000000-final`), что и на
`main` до этой правки — не регрессия).

## Симптом

MCP/BiDi automation (`AutomationCommand::Click(target)` / `::Type(target, text)`)
cannot interact with any part of the engine-drawn browser chrome (toolbar
buttons, tab strip, sidebar toggles, find-bar, address bar dropdown, context
menus, download panel, …). A `Target::Selector`/`Target::NodeId` naturally
never matches chrome elements (they live in a separate `chrome_doc`, not the
page document), but even `Target::Point { x, y }` — which resolves to a raw
viewport coordinate — silently lands on page content instead of chrome, with
no error, because the click never reaches chrome hit-testing at all.

## Root cause (verified by reading both code paths, not by report)

Two independent click-dispatch paths exist and only one of them knows about
chrome:

- **Real mouse input** (`crates/shell/src/app/window_event/mouse_input.rs`,
  `on_mouse_input`, `ElementState::Pressed` branch, line ~319): checks
  `self.point_over_chrome(x_css, y_css)` **first**; if true, routes through
  `self.chrome_hit_test(...)` and `self.dispatch_chrome_action(...)` and
  returns — chrome swallows the click before it can reach the page. This is
  the CC-4/CC-5 dispatch path documented at that call site.
- **Automation clicks** (`crates/shell/src/app/about_to_wait.rs`,
  `AutomationCommand::Click`/`::Type` handlers): call
  `self.resolve_automation_target(&target)` (`crates/shell/src/lumen/automation.rs:50`)
  to get `(x, y)`, then call `self.handle_click_at(x, y)`
  (`crates/shell/src/lumen/click.rs:24`) directly. `handle_click_at_inner`
  (the ~700-line function it wraps) checks a long list of open overlays
  (DevTools inspector, color/date pickers, `<select>` dropdown, tab context
  menu, download panel, command palette, …) but **never calls
  `point_over_chrome`/`chrome_hit_test`** — confirmed by `grep -n
  "point_over_chrome\|chrome_hit_test" crates/shell/src/lumen/click.rs`
  returning zero matches. A synthetic click at chrome's own screen
  coordinates (e.g. a toolbar button's `(x, y)`) falls straight through to
  the page hit-test/scrollbar-track logic at the bottom of the function.

The same asymmetry applies to `AutomationCommand::Type`, which reuses
`resolve_automation_target` + `handle_click_at` to focus the target before
injecting characters.

## Why this matters (concrete consequences already hit)

- BUG-405 срез 48 (content_epoch overlay-cache diagnostics): a census script
  driving 55 `AutomationCommand::Click` calls against hover/`:active`-gated
  chrome UI never once produced a state change, because none of the clicks
  could reach chrome — the census was structurally blind to that entire
  class of overlay source, not just low-signal.
- BUG-405 срез 51: the same gap blocked a live measurement of
  `strips_used=4` (a chrome layout state requiring toggling the vertical
  sidebar + right sidebar via UI clicks), forcing that slice to fall back to
  a hand-built `Rect` instead of a real click-driven state.
- Any future WPT-style or E2E test that needs to click a real toolbar
  button, switch tabs, or open the find-bar through the MCP/BiDi automation
  surface will silently misfire (click lands on whatever page content is
  under those coordinates) rather than erroring — the "succeeds but does
  nothing" shape BUG-436 already named for a different gap.

## Why not a quick point-fix

`handle_click_at_inner` is not a simple hit-test-then-dispatch function: it
is a single ~700-line state machine coupling press-time overlay swallowing
(pickers, panels, drag/DnD arm-time, `:active` set) with release-time
cleanup in the sibling `Released` branch of `on_mouse_input`
(`mouse_input.rs`, second half) — resize drags, tab drag-and-drop, DnD
`drop`/`dragend`, pointer-capture release. Automation's `Click` is a single
synthetic call with no separate press/release pairing, so naively adding a
`point_over_chrome` branch to `handle_click_at_inner` would leave chrome
clicks without the matching release-time bookkeeping that real mouse input
gets for free from the `ElementState::Released` branch — a correct fix needs
to decide how (or whether) automation clicks synthesize both press and
release against chrome, not just add one more `if` before the page hit-test.
That design decision is out of scope for a single P3 slice; recorded here so
the next session has the exact two call sites and the reason the obvious
patch is unsafe.

## How to reproduce

1. Start a live window: `lumen.exe --mcp-live-port <port> samples/page.html`.
2. Drive an `AutomationCommand::Click` (or the MCP/BiDi tool that wraps it)
   at the pixel coordinates of any toolbar button (e.g. the address bar,
   `y_css` inside `toolbar::CHROME_H`). Compare against a real mouse click at
   the same coordinates (`graphic_tests/run.py`'s driver or manual use)
   which does trigger the chrome action.
3. Alternatively, read `crates/shell/src/lumen/click.rs` for
   `point_over_chrome`/`chrome_hit_test` — absent — versus
   `crates/shell/src/app/window_event/mouse_input.rs:319` — present.

## Связь

Named as an open remainder by BUG-405 срезы 48/49/50/51/52 (`bugs/BUG-405-FIXED.md`)
without its own ticket; this file is that ticket. Not a BUG-405 slice itself —
filed separately per that remainder's own instruction ("остаётся
незаведённым отдельным тикетом").
