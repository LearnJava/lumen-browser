# BUG-1371 — `unicode-bidi: bidi-override` / `<bdo>` не действуют на вложенные строчные элементы

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 14, `css/CSS2` (text, linebox, fonts, generated-content, lists, bidi-text))
**Область:** layout (`crates/engine/layout/src/bidi.rs` — переопределение направления не передаётся детям `span`/`b`/`bdo`)

## Симптом

`<p style="direction:rtl;unicode-bidi:bidi-override">`, `--screenshot` 300×200:

| разметка | видно | ожидается |
|---|---|---|
| `<p class=o>abc def</p>` | `fed cba` | `fed cba` — верно |
| `<span style="direction:rtl;unicode-bidi:bidi-override">abc def</span>` внутри обычного `<p>` | `fed cba` | верно |
| `<p class=o><span>abc</span> def</p>` | `abc fed` | `fed cba` — текст внутри `span` не перевёрнут |
| `<p class=o><b>abc</b> def</p>` | `abc fed` | `fed cba` |
| `<p><bdo dir=rtl>abc def</bdo></p>` | `abc def` | `fed cba` |

Переопределение у блока-контейнера действует на его прямой текст, но не на потомков-inline; `<bdo dir=rtl>` не действует вовсе.

## Как найдено

WPT-RUN-14 срез 14: `bidi-text/bidi-box-model-*` (38 из 105 id каталога), `bidi-*`, `unicode-bidi-applies-to-*` — рамка/текст `span` внутри `.rtol { direction: rtl; unicode-bidi: bidi-override }`; эталон — те же слова, перевёрнутые вручную. Число 44 — по правилу «в источнике есть `bidi-override`, `U+202E` или `<bdo>`», пробами подтверждены строки таблицы; `bidi-001.xht` (явные `U+202E`/`U+202C` внутри рамки) — отдельная причина, не отделена.

## Что делать

Передавать `bidi-override` потомкам-inline по CSS Writing Modes L3 §2.4.2 (override действует на весь текст внутри бокса, включая вложенный inline), `<bdo dir>` — через UA-стиль `unicode-bidi: bidi-override` (HTML LS §15.3.3).

## Как проверить

`css/CSS2/bidi-text/bidi-box-model-003.xht`, `bidi-box-model-009.xht`.
