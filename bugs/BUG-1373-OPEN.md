# BUG-1373 — `::first-letter` не захватывает пунктуацию после первой буквы (CSS Pseudo-Elements L4 §5.1)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 15, `css/CSS2` (остальные каталоги: selectors, css1, syntax, box-display, visufx, visudet, …))
**Область:** layout (`crates/engine/layout/src/box_tree/pseudo_text.rs::first_letter_text_len` — пунктуация после буквы не входит в `::first-letter`)

## Симптом

CSS Pseudo-Elements L4 §5.1: `::first-letter` — первая типографская буквенная единица строки, **вместе с предшествующей и последующей пунктуацией** (классы Ps/Pe/Pi/Pf/Po). `first_letter_text_len` (`pseudo_text.rs:77`) пропускает только ведущую пунктуацию; в её докстринге прямо сказано: «leading punctuation only (the spec also includes punctuation immediately following the letter)».

`--dump-display-list`, `div::first-letter{color:green;font-size:36px}`:

| разметка | первый `DrawText` (36 px) | ожидается |
|---|---|---|
| `<div>T)est</div>` | `"T"` | `"T)"` |
| `<div>)T)est</div>` | `")T"` | `")T)"` |
| `<div>(Test</div>` | `"(T"` | верно |

## Как найдено

WPT-RUN-14 срез 15: `css/CSS2/selectors/first-letter-punctuation-001…339` — **339 id**, и в каждом разметка `<div>&#xNN;T&#xNN;est</div>` (пунктуация и до, и после буквы), эталон — `<span>&#xNN;T&#xNN;</span>est`. A/B (копии пар с `::first-letter` вместо `:first-letter`, `.tmp/ab15.py`, не закоммичены): ни один не стал `identical`. Второй A/B (`.tmp/ab15b.py`: в тесте и эталоне убрана завершающая пунктуация): **26 из 339 → `identical`** (18 с ASCII-пунктуацией, 8 с типографскими кавычками), остальные 313 — BUG-1374 (набор символов).

## Что делать

В `first_letter_text_len` после первой буквы поглощать идущие подряд символы пунктуации (тот же предикат, что и для ведущих). Юнит-тесты — `box_tree/tests/pseudo_first_line.rs`.

## Как проверить

`css/CSS2/selectors/first-letter-punctuation-067.xht`…`-070.xht` (кавычки), `-001.xht` после BUG-1366 и BUG-1374 — `reftest_pixdiff.py --viewport 800x600 --ahem` даёт `identical`. Закрытие вместе с BUG-1366 и BUG-1374 — до 339 id.
