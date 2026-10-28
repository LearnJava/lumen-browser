# UX-LEVEL0 — базовое поведение указателя и фокуса, как в любом браузере

Решение пользователя 2026-10-28 (дорожка «уровень 0», ADR-034). Источник —
`docs/ux-polish-audit-2026-10.md` (пункты P0, ещё не сделанные на 2026-10-28).

## UX-CURSOR-AUTO — `cursor: auto` над ссылкой и текстом (P6, S)

**Depends on:** —. **Files:** `crates/shell/src/input/winit_events.rs:44-50`
(`css_cursor_to_winit`), `crates/shell/src/lumen/cursor.rs:142-150`, hit-test в layout
(результат `HitTestResult`, поле `cursor`), тесты рядом.

Сейчас `CssCursor::Auto → CursorIcon::Default` всегда: над обычной ссылкой без
`cursor:pointer` в CSS сайта стрелка, над текстом — стрелка вместо I-beam. В Chrome UA-лист
даёт `a:any-link { cursor: pointer }`, а `auto` над текстом = `text`, над редактируемым —
`text`.

1. UA-правило `:any-link { cursor: pointer }` в `crates/engine/layout/src/style/ua.rs`
   (проверить, что `:any-link` матчится; если нет — `a[href], area[href]`).
2. Разрешение `auto` по результату hit-test: попадание в текстовый фрагмент или
   `input`/`textarea`/`[contenteditable]` → `Text`, иначе `Default`. Нужен признак «попал в
   текст» в результате hit-test (если его нет — добавить).

**Done when:** unit-тесты: ссылка без стилей → `Pointer`; абзац текста → `Text`; пустой фон →
`Default`; `cursor: default` на ссылке сайта перебивает UA. Живая проверка на 2–3 сайтах.

## UX-FOCUS-RING — видимое кольцо фокуса и честный `:focus-visible` (P6, S)

**Depends on:** —. **Files:** `crates/engine/layout/src/style/ua.rs`,
`crates/engine/layout/src/style/matching/forms.rs:197-200`, место, где ставится фокус
(`FOCUS_NID`), обработка Tab/мыши в shell.

Сейчас в UA-листе нет ни одного правила `outline` для фокуса, а `:focus-visible` ≡ `:focus`.
Клавиатурная навигация по сайту невидима.

1. UA-правило `:focus-visible { outline: auto 1px -webkit-focus-ring-color }` (или ближайшее,
   что умеет `outline` в Lumen: 2px solid, цвет фокуса из темы).
2. Флаг «фокус пришёл с клавиатуры» (Tab, клавиши, `focus()` после клавиатурного ввода) рядом с
   `FOCUS_NID`; `:focus-visible` матчится при нём, а для `input`/`textarea` — всегда
   (правило Chrome). Клик мышью по ссылке/кнопке кольца не даёт.

**Done when:** unit-тесты на матчинг `:focus-visible` (Tab → да, клик по кнопке → нет, клик по
input → да); graphic-тест с фокусом по Tab; полный `graphic_tests/run.py` и эталоны в том же
коммите.
