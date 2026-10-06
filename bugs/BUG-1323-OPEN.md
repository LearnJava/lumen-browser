# BUG-1323 — U+00A0, U+202F, U+2007 (класс GL) ведут себя как пробел: по ним переносится строка

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 9, `css/css-text`, первая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/inline_wrap.rs:383,676,1228` и `inline_build.rs:148,526,558,926,998` — текст режется `str::split_whitespace()` / `char::is_whitespace()`, а у Rust `White_Space` входят NBSP, NNBSP, FIGURE SPACE; CSS Text L3 §4.1.1 «document white space» — только U+0020, U+0009, U+000A, U+000C, U+000D)

## Симптом

`font: 25px monospace; width: 50ch`, текст `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa<X>bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb` (39 + 31 символ, в одну строку не влезает), `--dump-layout`, высота блока:

| `<X>` | получено | ожидается |
|---|---|---|
| `&#x20;` SPACE | 60 (2 строки) | 60 |
| `&#xA0;` NBSP | **60** | 30 — GL, переноса нет, строка переполняется |
| `&#x202F;` NNBSP | **60** | 30 |
| `&#x2007;` FIGURE SPACE | **60** | 30 |
| `&#x2060;` WORD JOINER | 30 | 30 |
| `&#x2002;` EN SPACE | 60 | 60 (BA — перенос после, верно) |

Вторая проба, `font: 20px monospace; width: 10ch`: `aaaa bbbb&nbsp;cccc` → `aaaa bbbb` / `cccc` — перенос по NBSP; с `width: 5ch` и `aaaa&nbsp;bbbb` — две строки (должна быть одна переполняющая). Ширина серии `a&nbsp;&nbsp;&nbsp;b` верна (пробелы не схлопываются), но каждый NBSP — возможность переноса.

## Как найдено

WPT-RUN-14 срез 9: `css/css-text/i18n/css3-text-line-break-baspglwj-120/121/124.html` (`white-space:normal` и `pre-line` падают: NBSP/NNBSP/FIGURE SPACE), `line-breaking/line-breaking-atomic-003/018.html` (NNBSP/FIGURE SPACE рядом с inline-блоком, A/B на Ahem: красный цвет остаётся). Неразрывный пробел в реальном вебе повсюду («10&nbsp;км», «№&nbsp;5», инициалы) — дефект виден не только тесту.

## Что делать

В `inline_build.rs`/`inline_wrap.rs` заменить `split_whitespace()`/`is_whitespace()` на проверку CSS-пробелов (U+0020, U+0009, U+000A, U+000C, U+000D); NBSP-подобные символы остаются внутри слова, ширина — по шрифту. Затронуты свёртка пробелов (`inline_build.rs`), обрезка краёв строки и `balance_wrap`/`pretty_wrap` (`inline_wrap.rs:475,497,527`). Пересекается с `LINEBREAK-UAX14` (ROADMAP), но чинится независимо и дёшево.

## Как проверить

`css/css-text/i18n/css3-text-line-break-baspglwj-120.html`, `-121.html`, `-124.html`; проба выше — высота блока с NBSP = 30.
