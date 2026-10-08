# BUG-1451 — После `display:none` или удаления сфокусированного элемента `document.activeElement` остаётся им, `:focus` продолжает совпадать; в `--screenshot` `:focus`/`:focus-within` после `el.focus()` не окрашивают ничего

**Статус:** OPEN
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
