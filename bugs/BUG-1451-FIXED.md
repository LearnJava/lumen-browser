# BUG-1451 — После `display:none` или удаления сфокусированного элемента `document.activeElement` остаётся им, `:focus` продолжает совпадать; в `--screenshot` `:focus`/`:focus-within` после `el.focus()` не окрашивают ничего

**Статус:** FIXED 2026-10-22 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** js/shell (фокус: «focus fixup» и `:focus` в `--screenshot`)

## Симптом

(1) `i.focus(); i.style.display='none'` → `document.activeElement === i`, `i.matches(':focus')` = `true`; `i.focus(); i.remove()` → `document.activeElement` — `<input>` (должен стать `body`). Спецификация: HTML §6.6.3 «focus fixup rule» — элемент, переставший быть фокусируемой областью, теряет фокус. Страдают `focus-display-none-001`, `focus-within-display-none-001`, `focus-within-removal`, `has-focus-display-change`, `active-display-none-001` (по 1–2 сабтеста). (2) В `lumen --screenshot` (путь, которым снимают reftest'ы) стиль `:focus`/`:focus-within` после `el.focus()` (инлайн-скрипт, `onload`) не применяется: заливка `#f:focus{background:green}` — 0 зелёных px при 2 500 красных, тот же `#t:focus-within`. В MCP-сессии (`eval`) тот же стиль применяется (`getComputedStyle` зелёный). Какая из двух — потеря состояния фокуса до растра или порядок «стиль → фокус» — не установлено.

## Проба

| проверка | у нас | ожидается |
|---|---|---|
| `i.focus(); i.style.display='none'; document.activeElement === i` | `true` | `false` |
| то же, `i.matches(':focus')` | `true` | `false` |
| `i.focus(); i.remove(); document.activeElement.tagName` | `INPUT` | `BODY` |
| `--screenshot`, `#f:focus{background:green}` + `f.focus()` в скрипте | 0 зелёных px | 2 500 |
| `--screenshot`, `#t:focus-within{background:green}` + `f.focus()` | 0 зелёных px | 2 500 |
| MCP `eval`: `f.focus(); getComputedStyle(f).color` для `#f:focus{color:blue}` | `rgb(0, 0, 255)` | верно |

## Как найдено

WPT-RUN-14 срез 20: `selectors/focus-display-none-001.html`, `focus-within-display-none-001.html`, `focus-within-removal.html`.

## Что делать

(1) При скрытии/удалении сфокусированного элемента сбрасывать фокус на документ (фокус-фиксап). (2) Установить, почему состояние фокуса не доходит до `--screenshot` (и до IPC-снимка `wptrunner`).

## Как проверить

Таблица выше; `css/selectors/focus-display-none-001.html`.

## Исправление (2026-10-22, P3)

(1) Фокус-фиксап (`_lumen_focus_fixup`, `web_api_shim_tail_b.js`) теперь снимает фокус и у элемента, который перестал рендериться (`display: none` у него или у предка — `_lumen_focus_not_rendered`; «нет записи в снимке» равно «нет бокса», а без опубликованного layout элемент не считается скрытым), и сбрасывает не только JS-копию, но и фокус оболочки/движка (`_lumen_focus_drop` → `_lumen_request_blur`), поэтому `matches(':focus')` тоже становится `false`. Удаление сфокусированного элемента из документа возвращает фокус на `<body>` сразу: `document.activeElement` и `focus()` проверяют подключённость (`_lumen_focus_drop_if_disconnected`), а `focus()` не фокусирует отсоединённый элемент и перепроверяет цель после `blur`/`focusout`-обработчиков (`focus-within-removal.html`).

(2) Причина «0 зелёных px» — не порядок «стиль → фокус»: однократный headless-путь `render_source_to_png` не дренировал очередь `take_focus_requests`, а layout после скриптов считался без `set_interactive_state`. `settle_after_load` (`crates/shell/src/dump_mode.rs`) теперь берёт последний запрос фокуса и пересчитывает layout с этим `:focus`/`:focus-within`.

Тесты: `js` (`dom/tests/v8_bug560_sync_focus.rs` — `focus_fixup_drops_focus_on_display_none`, `removing_focused_element_resets_active_element`, `focus_target_removed_by_focusout_listener_leaves_no_focus`), `shell` (`tests/page_pipeline.rs::screenshot_settle_applies_focus_requested_by_script`). Реftest-ы WPT (`focus-display-none-001` и др.) прогоном не проверялись.
