# BUG-1322 — `white-space: pre-wrap` и `break-spaces` не переносят строку по ширине: ведут себя как `pre`

**Статус:** FIXED 2026-10-07
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 9, `css/css-text`, первая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/inline_wrap.rs:600` — ветка `if white_space.preserves_whitespace()` отдаёт сегмент одним куском без переноса; `WhiteSpace::PreWrap`/`BreakSpaces` входят в `preserves_whitespace()`, а различие «переносится / нет» живёт в `is_nowrap()`, до которого ветка не доходит)

## Симптом

`--dump-layout`, `font: 20px`, `width: 200px`, текст `aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk`:

| `white-space` | высота блока |
|---|---|
| `normal` | 44,30 (2 строки) |
| `pre-wrap` | **22,15** (одна строка шире контейнера) |
| `break-spaces` | **22,15** |

Длинный текст из двух слов (58 + 39 символов) с `pre-wrap` — тоже одна строка (22,15); с `pre-line` — две. На `width: 8ch` текст `aaaa bbbb cccc dddd` с `pre-wrap` — одна строка `aaaa bbbb cccc dddd`. Явный `\n` в `pre-wrap` строку рвёт (forced break), мягкого переноса по ширине нет. `typography.rs:315` документирует `PreWrap` как «wraps at available width», `CAPABILITIES.md:73` отмечает значения ✅ — значения разобраны, поведение переноса не работает.

## Как найдено

WPT-RUN-14 срез 9: `css/css-text/i18n/css3-text-line-break-baspglwj-*` (113 файлов по 4 сабтеста: `white-space:normal|pre-line|pre-wrap|break-spaces`) — `pre-wrap` падает в 102 из 113, `break-spaces` — в 102. 22 файла падают **только** на этих двух сабтестах (`normal` и `pre-line` зелёные) — 22 id / 44 сабтеста; остальные ещё и по `LINEBREAK-UAX14`/[BUG-1323](BUG-1323-FIXED.md). В 136 не зелёных id текст теста содержит `white-space: pre-wrap|break-spaces` (`i18n/` 105, `overflow-wrap/` 12, `line-break/` 8, `letter-spacing/` 7, `text-align/` 3, `hyphens/` 1) — часть из них, возможно, упирается в этот же дефект; сколько — не проверено.

## Что делать

В `wrap_inline_run` разделить «пробелы сохраняются» и «перенос запрещён»: для `PreWrap`/`BreakSpaces` — обычный жадный перенос по ширине с сохранёнными пробелами (CSS Text L3 §4.1.3: висящие пробелы в конце строки у `pre-wrap` не вызывают переполнения; у `break-spaces` — занимают место и дают возможность переноса после каждого пробела). Ветку `preserves_whitespace()` оставить для `Pre`.

## Как проверить

`css/css-text/i18n/css3-text-line-break-baspglwj-001.html` (сабтесты `white-space:pre-wrap`/`break-spaces`); проба выше одной страницей с тремя `div`.

## Дополнение: WPT-RUN-14 срез 10 (2026-10-06, `css/css-text`, часть 2)

Проба на другой выборке: `width: 100px; white-space: pre-wrap`, `<i>aaaaaa</i> <i>bbbbbb</i> <i>cc</i>` — три слова на одной строке (x = 0 / 67 / 141); `日本語×6` в `pre-wrap` — одна строка (высота 24,2), в `normal` — три. В `white-space/`, `word-break/`, `text-transform/` и остальных каталогах второй половины 276 из 654 не зелёных id содержат `pre-wrap`/`break-spaces`/`white-space-collapse`/`text-wrap-mode`/`<textarea>` (по тексту, не по причине). Часть `break-spaces-newline-*`/`pre-wrap-018` падает по другой причине — [BUG-1327](BUG-1327-FIXED.md).

## Исправление (2026-10-07, P6)

Новый модуль `box_tree/inline_wrap_preserved.rs`: `wrap_preserved_segment` режет сегмент `pre-wrap`/`break-spaces` по мягким возможностям переноса — после пробела/табуляции (у `pre-wrap` — после всей серии пробелов), внутри слова по UAX #14 (`line_break::break_opportunities`), для `word-break: break-all` между любыми символами, для `overflow-wrap: break-word|anywhere` и `word-break: break-word` — по символам, если слово шире строки. Жадная укладка: пробелы в конце строки у `pre-wrap` висят и в ширину не входят, у `break-spaces` занимают место. Соседние токены одной строки склеены в один фрагмент. `wrap_inline_run` вызывает модуль для `!is_nowrap()`; `pre` остался единым фрагментом. Тесты — `box_tree/tests/pre_wrap.rs`. Пробельный узел между inline-соседями в этих режимах отбрасывается раньше раскладки — отдельный дефект [BUG-1327](BUG-1327-FIXED.md).
