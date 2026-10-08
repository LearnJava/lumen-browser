# BUG-1570 — Встроенные counter-style CSS Counter Styles 3 §6 не реализованы: `hebrew`, `armenian`, `georgian`, `cjk-*`, `japanese-*`, `korean-*`, `*-chinese-*`, `ethiopic-numeric`, `arabic-indic` и ещё 30 систем счисления

**Статус:** OPEN (ДОРАБОТКА → COUNTER-STYLES-BUILTIN)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/counters.rs:1700-1740` — `format_counter` знает `decimal`, `decimal-leading-zero`, `lower/upper-alpha|latin`, `lower/upper-roman`, `lower-greek`, `disc/circle/square`)

## Симптом

`counter(a, hebrew)` при `counter-reset: a 12` даёт `12` для всех 40 имён из §6.1–§6.3: `armenian`, `lower-armenian`, `upper-armenian`, `georgian`, `hebrew`, `cjk-decimal`, `cjk-earthly-branch`, `cjk-heavenly-stem`, `hiragana`, `hiragana-iroha`, `katakana`, `katakana-iroha`, `japanese-informal`, `japanese-formal`, `korean-hangul-formal`, `korean-hanja-informal`, `korean-hanja-formal`, `simp-chinese-informal`, `simp-chinese-formal`, `trad-chinese-informal`, `trad-chinese-formal`, `ethiopic-numeric`, `arabic-indic`, `bengali`, `cambodian`, `devanagari`, `gujarati`, `gurmukhi`, `kannada`, `khmer`, `lao`, `malayalam`, `mongolian`, `myanmar`, `oriya`, `persian`, `tamil`, `telugu`, `thai`, `tibetan`, `disclosure-open`/`disclosure-closed`. Список, который `CSS-SPECS.md` называл «✅ @counter-style», покрывает только `@counter-style`-правила автора. 158 reftest `css-counter-styles/<имя>/`.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `counter-reset:a 12`, `counter(a, <имя>)` для 40 имён из списка | `12` | `יב` (hebrew), `一二` (cjk-decimal), `१२` (devanagari) — по таблицам §6 |
| `counter(a, upper-roman)`, `lower-greek`, `lower-latin` | `XII`, `μ`, `l` | то же |

## Как найдено

WPT-RUN-14 срез 25: `css/css-counter-styles/<имя>/*.html` (40 каталогов, 158 reftest `thick`).

## Что делать

Задача COUNTER-STYLES-BUILTIN: объявить 40 встроенных стилей как предопределённые `@counter-style` (CSS Counter Styles 3 §6.1–§6.3) в `CounterStyleRegistry` — `additive`/`numeric`/`alphabetic`/`fixed`/`extends` и `speak-as`; для CJK — формальные/неформальные системы с `additive` и `suffix`/`negative` из таблиц; `disclosure-open/closed` — `fixed` из треугольников. Проверка: reftest-ы `css-counter-styles/<имя>/`. Пересекается с BUG-1546 и BUG-1548 (их эталоны используют `<bdi>` и `inside`-маркер).

## Как проверить

эталоны `css-counter-styles/hebrew/css3-counter-styles-015.html`, `cjk-decimal/counter-cjk-decimal.html`.
