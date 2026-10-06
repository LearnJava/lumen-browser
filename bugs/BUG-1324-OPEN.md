# BUG-1324 — слово, начинающее строку, не режется: `word-break: break-all` и `hyphens: manual|auto` не срабатывают на первом слове

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 9, `css/css-text`, первая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/inline_wrap.rs` — ветка `word_break == BreakAll` (:880) стоит внутри `if needs_wrap`, а `needs_wrap` требует `!current_line.is_empty()` (:735); `try_hyp_break` (:816) вызывается там же)

## Симптом

`font: 20px monospace; width: 8ch`, `--dump-layout`:

| разметка | получено | ожидается |
|---|---|---|
| `word-break:break-all`, 29 букв `a` без пробелов | **одна строка** | 4 строки (8, 8, 8, 5) |
| то же, перед словом `x ` | `x aaaaaa` / `aaaaaaaa` / `aaaaaaaa` / `aaaaaaa` — режется | то же |
| `hyphens:manual`, `aaaa&shy;bbbbbbbbbbbbb` | **одна строка** `aaaabbbbbbbbbbbbb` | `aaaa-` / `bbbbbbbbbbbbb` |
| `hyphens:manual`, `xx aaaa&shy;bbbbbbbbbbbbb` | `xx` / `aaaa-` / `bbbbbbbbbbbbb` — режется | то же |
| `hyphens:auto lang=en`, `internationalization` и `xx internationalization` | не режется ни в одном случае | перенос по словарю (Knuth–Liang для en заявлен в `CAPABILITIES.md:63`; почему не сработал и в середине строки — не разобрано) |
| `overflow-wrap:break-word`, слово первое | 4 строки — верно (отдельная ветка `ow_char_break`, :930) | 4 строки |

То есть `break-all` и мягкий перенос работают, только когда на строке уже что-то стоит; слово, **начинающее** строку (в том числе единственное в блоке), не режется — а именно такое слово и переполняет контейнер.

## Как найдено

WPT-RUN-14 срез 9: `css/css-text/hyphens/` (48 не зелёных id из 59: 47 thick, 1 без пиксельного вердикта), `overflow-wrap/` и `word-break` (34 id помимо 12 Ahem-identical из BUG-1273; по пробе `overflow-wrap: break-word` на первом слове **работает**, так что причина этих 34 другая — не установлена), `line-break/line-break-anywhere-*` (часть из 51). Пробой подтверждены только `word-break: break-all` и `&shy;` (таблица выше); что остальные id `hyphens/` падают по той же причине — **не проверено**; часть зависит от BUG-1273 (шрифт теста не Ahem), `hyphenate-character`/`hyphenate-limit-chars` — свойств нет вовсе (CSS-SPECS.md:76).

## Что делать

Ветки `break-all`, мягкого и автоматического переноса вызывать и для слова, начинающего строку: условие — «слово шире доступной ширины», а не «`needs_wrap` и непустая строка» (CSS Text L3 §5.2, §6.1). Для `hyphens: auto` словарь подключён (`hp.hyphenate`) — отдельно выяснить, почему он молчит.

## Как проверить

`css/css-text/hyphens/hyphens-manual-011.html`, `hyphens-auto-002.html`; `word-break`-тесты — в срезе S10; проба выше (`--dump-layout`).

## Дополнение: WPT-RUN-14 срез 10 (2026-10-06, `css/css-text`, часть 2)

`word-break/word-break-break-all-*` — 37 не зелёных id (30 в `word-break-break-all-0NN`, 9 `-inline-*`; 10 из них с `pre-wrap`). Проба: `width: 100px; word-break: break-all`, `aaaa…` (28 символов) — одна строка (высота 24,2); `xx aaaa…` — слово после пробела режется (3 строки, 96,8). Тот же дефект, что в основном описании.
