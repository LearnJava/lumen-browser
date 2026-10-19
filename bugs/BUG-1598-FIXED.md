# BUG-1598 — `<q>` не получает кавычек: в UA-таблице нет `q::before`/`q::after { content: open-quote/close-quote }`

**Статус:** FIXED 2026-10-19 (P3)
**Заведён:** 2026-10-11 (P2, WPT-RUN-14 срез 28, 30 малых модулей `css`: `css-logical`, `compositing`, `css-scroll-anchoring`, `css-content`, `css-scrollbars`, `css-will-change`, `css-syntax`, `css-color-adjust`, `css-rhythm`, `geometry` и др.)
**Область:** layout (`crates/engine/layout/src/style/ua.rs`, `style/pseudo.rs` — UA-правила генерируемого содержимого; `box_tree/inline_build.rs:1259` — `open-quote`/`close-quote` уже работают)

## Симптом

`<p>A <q>x</q></p>` даёт `A x` — без «…». HTML rendering §15.3.7 требует `q::before { content: open-quote }` и `q::after { content: close-quote }`. Механизм кавычек (`quotes`, вложенность, `open-quote`/`close-quote`) работает: `<style>q::before{content:open-quote}q::after{content:close-quote}</style>` даёт `A “x”`. Чего нет — самого UA-правила.

## Проба

`--dump-layout`, `frag[0]` строки: `<p>A <q>x</q></p>` → `"A x"`; то же со страничным правилом выше → `"A “x”"`; `quotes:auto` и `quotes:"[" "]"` на `<q>` без правила — `"A x"`. Затем `css/css-content/quotes-NNN.html` с добавленным в `<style>` страничным правилом, `--screenshot` 800×600 против эталона, побайтово: 001, 002, 003, 020 — **совпадают с эталоном** (без правила — нет); 009, 013, 030 — не совпадают (другая причина: `lang`/`quotes` по языку, не изолирована).

## Охват

WPT-RUN-14 S28: 45 reftest `thick` в `css/css-content` — `quotes-001…034` (33), `quotes-first-letter-001…005` (5), `quotes-first-line`, `quotes-slot-scoping`, `quotes-lang-dynamic-001`, `attr-case-sensitivity-001/002/004`, `attr-chained-pseudo-001`. Часть (минимум `quotes-009/013/030`, `quotes-first-letter-*`) после добавления правила останется красной.

Связано с [BUG-216](BUG-216-OPEN.md) (DEBTOR): там «кавычки примыкают вплотную» проверялись страницей с собственным `::before`/`::after`, поэтому отсутствие UA-правила не всплыло.

## Как проверить

`css/css-content/quotes-001.html`, `quotes-002.html`, `quotes-003.html`, `quotes-020.html`.

## Исправление

`style/pseudo.rs`: `ua_quote_content` даёт `q::before` — `open-quote`, `q::after` — `close-quote` (HTML rendering §15.3.7) как базовое значение `content` псевдоэлемента; авторское правило перекрывает его обычным каскадом (`content:none`, `quotes:none` работают). Фильтр «таблица стилей не адресует псевдоэлемент» (`sheet_targets_pseudo`) для `q` обходится. `counters.rs`: проба `::before`/`::after`, двигающая глубину кавычек, включается для `<q>` и без листовых кавычек в таблице — иначе вложенный `<q>` брал бы первичную пару. Тест `bug1598_q_gets_ua_quotes_without_author_rule`. Остаток по WPT (`quotes-009/013/030`, `quotes-first-letter-*`) — другие причины, в этот баг не входят.
