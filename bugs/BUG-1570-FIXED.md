# BUG-1570 — Встроенные counter-style CSS Counter Styles 3 §6 не реализованы: `hebrew`, `armenian`, `georgian`, `cjk-*`, `japanese-*`, `korean-*`, `*-chinese-*`, `ethiopic-numeric`, `arabic-indic` и ещё 30 систем счисления

**Статус:** FIXED 2026-10-22 (COUNTER-STYLES-BUILTIN, ветка `p1-counter-styles-builtin`)
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

## Как исправлено

`crates/engine/layout/src/counter_builtin.rs`: 40 предопределённых стилей §6 — таблицы `CounterStyleDef` (18 позиционных наборов цифр, аддитивные `hebrew`/`armenian`/`georgian`, алфавитные кана, `fixed` `cjk-earthly-branch`/`cjk-heavenly-stem`, `disclosure-*`) и две алгоритмические системы `CounterSystem::Builtin` (девять «длинных» восточноазиатских стилей, `ethiopic-numeric`). `counters.rs::lookup_style` ищет сначала авторский `@counter-style`, затем таблицу. `counter()`/`counters()` отдают только представление (`format_counter_with_registry`), текст `::marker` — префикс + представление + суффикс (`format_marker_with_registry`, суффикс по умолчанию `". "`); запасной стиль даёт только представление. `lower-greek` — настоящий алфавитный (`αα` после `ω`). 1150 строк таблицы ожиданий из эталонов WPT — тест `counter_builtin::wpt_cases`.

## Что осталось

- Значения счётчика — `i32`: расширенный диапазон восточноазиатских стилей (`*-extended`, > 2^31) недостижим; `korean-hangul-formal` для 10^15 в эталоне — `천조` без `일` (ошибка эталона).
- `speak-as`, направление-зависимый глиф `disclosure-closed`, перестановка суффикса при `dir=rtl` (`counter-suffix.html`).
- Область счётчика не включает следующих соседей (`counter-hebrew-nested.html`: `counter-reset` на первом `span`).
- Reftest-ы `css3-counter-styles-NNN` с `<ol start>`/`<li value>` ждут BUG-1547.
