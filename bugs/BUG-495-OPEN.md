# BUG-495: `background-position-x`/`background-position-y` standalone longhands entirely unimplemented

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Тип:** доработка — остаток (edge-relative offset форма и `x-start`/`x-end`/
`y-start`/`y-end`) требует нового представления «anchor edge + offset» в
`PositionComponent`, разделяемом пятью потребителями `<position>`; см.
ревизию P3 2026-09-03 ниже
**Дата:** 2026-08-02
**Компонент:** css-parser/layout (`crates/engine/layout/src/style.rs::apply_declaration`,
`selector_query.rs::computed_style_to_map`)
**Найден:** WPT-RUN-3 срез 9 (`ROADMAP.md`) — массовый прогон `css/css-backgrounds`

## Механизм

`grep -rn "background-position-x\|background_position_x" crates/css-parser/src/
crates/engine/layout/src/` returns **zero matches** anywhere in the
workspace. Only the `background-position` *shorthand* is implemented
(`style.rs:15860`); the two standalone longhands added by CSS Backgrounds
and Borders Level 4 (`background-position-x`, `background-position-y`) have
no match arm in `apply_declaration` and no entry in
`computed_style_to_map` — distinct from [BUG-472](BUG-472-OPEN.md) (which
covers properties that *are* implemented but missing from the computed-style
map): here `CSS.supports('background-position-x', <any value>)` itself
returns `false`, confirming the property is unrecognized at the parse layer,
not just absent from the read-side map.

## Симптом

```
FAIL CSS Transitions: property <background-position-x> from neutral to [80px] at (0) should be [40px]
  - assert_true: 'to' value should be supported expected true got false
FAIL Property background-position-x value '0.5em'
  - assert_true: background-position-x doesn't seem to be supported in the computed style expected true got false
```

Two distinct signals, both confirmed against this slice's structured
wptreport:

1. `CSS.supports(prop, value) === false` for values that are valid per spec
   — `interpolation-testcommon.js`'s `'from'/'to' value should be
   supported` assertion — `animations/background-position-x-interpolation.html`
   (112 subtests) and `animations/background-position-y-interpolation.html`
   (112 subtests), 100% of each file.
2. `getComputedStyle(el).getPropertyValue(prop) === ''` unconditionally —
   `computed-testcommon.js`'s `test_computed_value` — `parsing/
   background-position-{x,y}-computed.html` (19 subtests each) and 8 of
   `parsing/background-computed.html`'s 39 subtests (the rest of that file
   is [BUG-472](BUG-472-OPEN.md), background-image/position/size/clip/
   repeat/origin/attachment/color, all *implemented* properties missing
   from the same map).

`parsing/background-position-{x,y}-{valid,invalid}.html` (13 subtests
total) are **not** attributed to this bug despite touching the same
properties — those fail on [BUG-484](BUG-484-OPEN.md) instead
(`_lumen_make_style`'s inline setter never routes through the parser for
*any* property, implemented or not, so it echoes the raw string back
regardless of whether `background-position-x` itself exists).

## Масштаб находки

5 files / 214 subtests in this slice, confirmed via source grep only for
`css/css-backgrounds` — not surveyed elsewhere in `css/`, but any WPT test
anywhere that names `background-position-x`/`-y` directly (rather than
through the shorthand) will hit the same gap.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-backgrounds/` for the 5
attributed files (`animations/background-position-{x,y}-interpolation.html`,
`parsing/background-position-{x,y}-computed.html`,
`parsing/background-computed.html` — the last shared with BUG-472).

## Срез 2026-09-03 (P3): базовая проводка влита, остаток — edge-offset/logical-keyword форма

Оба longhand-а заведены как match-arm в `apply_declaration`
(`crates/engine/layout/src/style/apply/paint.rs`), переиспользуя
`parse_position_component` (уже применявшийся для `transform-origin`/
`perspective-origin`) на своей оси. Каждая ось хранится в
`BackgroundLayer::position.{x,y}` (`ObjectPosition`), поэтому лонгхенд
трогает только свою компоненту, не сбрасывая вторую (проверено
`background_position_x_sets_only_x_axis`/`_y_sets_only_y_axis`). Оба
свойства добавлены в `SUPPORTED_PROPERTIES`
(`crates/engine/css-parser/src/lib.rs`) — закрывает симптом 1 (`CSS.supports`
проверяет только имя свойства по списку, не значение, см. BUG-501, поэтому
этого одного добавления достаточно). `computed_style_to_map` получил обе
оси (`background_position_axis_to_css`, comma-joined по слоям, как
`background-image`) — закрывает симптом 2 для покрытых форм значения.

**Покрыто:** keyword-формы (`center`/`left`/`right`, `top`/`bottom`
отклоняются на horizontal-оси и наоборот), `<percentage>`, `<length>`
(включая `em`/`calc()` через существующий `parse_length`), запятая-список
слоёв с цикличным применением — соответствует строкам 19–21, 24–27, 30-39
теста `parsing/background-position-x-computed.html`. Регресс-тесты:
`background_position_x_*`/`_y_*` в `style/tests/images.rs` (парсинг),
`computed_map_background_position_*` в `selector_query.rs` (сериализация).

**Не покрыто (осознанно отложено, тот же класс, что уже задокументирован
на `ObjectPosition::parse` для tri-/quad-формы `<position>`):**
- edge-relative offset form (`right -10px` → `calc(100% + 10px)`,
  `left -20%` → `-20%`) — требует представления «anchor edge + offset»
  поверх `PositionComponent` (сейчас только `Px`/`Percent`), которого нет
  ни у одного потребителя `<position>` в движке (`background-position`
  shorthand, `object-position`, `transform-origin`, `perspective-origin`,
  `mask-position` — ни один не поддерживает эту форму).
- `x-start`/`x-end`/`y-start`/`y-end` (writing-mode-relative keywords).
- Реальная motion-интерполяция значений при transition/animation — таблица
  Phase-0 animatable-свойств в `layout::animation` (`opacity`/`color`/
  `background-color`/`transform`/`height`) не расширена этим срезом;
  заявка тестов `animations/background-position-{x,y}-interpolation.html`
  проверяет только `CSS.supports()`-гейт (закрыт выше), не саму
  интерполяцию — реальный прогон интерполяционных сабтестов этого файла
  не выполнялся.

Остаток не выделен в отдельный баг — тот же осознанно отложенный
edge-offset-класс, что уже стоит недокументированным долгом на четырёх
других `<position>`-потребителях движка; выделять его только для этих двух
свойств было бы точечной, а не системной правкой. Статус остаётся `OPEN`
до системного расширения `<position>` (или до подтверждения живым WPT-
прогоном, что остатка достаточно мало для DEBTOR).

## Ревизия P3 2026-09-03: живой WPT-прогон 4 атрибутированных файлов — остаток не про edge-offset, это BUG-493

Живой прогон (`run_smoke.py`, свежая сборка, включает срез выше) дал числа,
не совпадающие с оптимистичным «Покрыто» среза 2026-09-03:

- `animations/background-position-x-interpolation.html`,
  `animations/background-position-y-interpolation.html` (112 сабтестов
  каждый) — **112/112 живьём**, полностью зелёные. Интерполяция
  (`interpolation-testcommon.js`) действительно закрыта этим срезом — вопрос
  «реальный прогон не выполнялся» из раздела «Не покрыто» выше закрыт: он
  выполнен, результат положительный.
- `parsing/background-position-x-computed.html`,
  `parsing/background-position-y-computed.html` (19 сабтестов каждый) —
  **1/19 живьём** в каждом, а не «покрыто keyword/percent/length» как
  утверждал срез выше. Единственный проходящий сабтест (`'left'`/`'top'`)
  проходит случайно: его ожидаемое computed-значение (`'0%'`) совпадает с
  нетронутым дефолтом узла.

Причина — не пробел в реализации BUG-495 (парсинг/apply/сериализация
проверены юнит-тестами `cargo test -p lumen-layout background_position` —
все 19 зелёные, реализация корректна), а [BUG-493](BUG-493-OPEN.md)
(ДОРАБОТКА → CSSOM-4, `getComputedStyle()` не форсирует синхронный
пересчёт стиля перед чтением): `computed-testcommon.js`'s
`test_computed_value` мутирует и читает в ОДНОМ синхронном тике скрипта, а
кэш computed-style обновляется только после релейаута — которого внутри
одного `test()`-колбэка не происходит. Прямая проба (`--mcp-port`,
`tools/call eval`) подтвердила механику: тот же `set` в одном `eval`-вызове
и `get` в отдельном (пересекающем границу релейаута DEVX-9) корректно даёт
`50%` для `'center'`; в одном тике — стабильно `0%` независимо от значения.
Полный разбор и другие подтверждения того же паттерна — в
[BUG-493-OPEN.md](BUG-493-OPEN.md), срез `css/css-backgrounds`.

**`.ini` приведён к живому состоянию:** оба `.ini` интерполяционных файлов
удалены (полностью зелёные), оба `.ini` computed-файлов сохранены с
единственным случайно проходящим сабтестом убранным из списка FAIL —
остальные 18 в каждом остаются `expected: FAIL`, но теперь attributed to
BUG-493, не к остатку этого бага.

**Итог по BUG-495 собственно:** точечная реализация (parse/apply/serialize)
верна и подтверждена и юнит-тестами, и живым WPT (там, где сама проверка не
упирается в BUG-493 — т.е. на интерполяции). Остаток статьи — edge-offset
форма (`right -10px`) и `x-start`/`x-end` — по-прежнему не реализован и
по-прежнему осознанно отложен (тот же класс, что на 4 других
`<position>`-потребителях), но это уже не главный источник WPT-провалов в
этой находке: главный источник — BUG-493, вне скоупа P3. Статус остаётся
`OPEN` (не DEBTOR — собственный edge-offset пробел ещё не оценён живым
прогоном, а прогон затруднён тем, что BUG-493 маскирует почти весь
computed-value сигнал).

## Ревизия P3 2026-09-03: переклассифицирован в ДОРАБОТКА → CSS-SPECS.md

Взят как следующий top-down пункт `STATUS-P3.md` (BUGS.md:59, после BUG-341
пропущен как приостановленный пользователем, BUG-480/BUG-490/BUG-491 —
как уже помеченные ДОРАБОТКА). Оба условия теста ДОРАБОТКА
(`docs/probe-method.md` §8) выполнены:

1. **Функциональности нет вовсе.** Чтение `PositionComponent`
   (`crates/engine/layout/src/style/values/flexgrid.rs:522`) подтверждает
   ровно два варианта — `Px(f32)`/`Percent(f32)`; представления «anchor
   edge + offset» (нужного, чтобы вернуть `calc(100% + 10px)` для
   `right -10px` или `-20%` для `left -20%`, как того требует
   `test_computed_value` в `background-position-x-computed.html`) нет
   нигде в типе.
2. **Объём — общий тип, а не одно свойство.** `parse_position_component`/
   `ObjectPosition::parse` — единая функция на ВСЕХ потребителей
   `<position>` в движке (подтверждено грепом `parse_position_component`/
   `ObjectPosition::parse` по `crates/engine/layout/src`):
   `background-position` шорткод и оба лонгхенда (`style/apply/paint.rs`),
   `object-position` (`style/apply/layout.rs`), `transform-origin`/
   `perspective-origin` (`style/apply/motion.rs`). Расширение требует не
   match-arm, а нового варианта в разделяемом enum + пересчёта `resolve()`
   (сейчас `free_space * percent` ИЛИ `px`, нужно `free_space * percent +
   px` одновременно) + серийализации по формату `calc()`, которую спека
   требует для смешанных anchor+offset значений (см. три формы ожидаемого
   вывода в тестовых assertions: голый `-20%`, `calc(100% + 10px)`,
   `calc(100% - 10px)` — знак зависит от anchor'а и требует точного
   соответствия сериализации Typed OM, не просто печати числа).

Точечного P3-фикса на этом остатке нет — расширение `PositionComponent`
меняет разделяемый тип, используемый пятью потребителями сразу, что
требует проектирования (форма варианта, формат сериализации), не
локальной правки одного файла. Тот же прецедент, что BUG-491/492
(2026-09-03 ранее в этой же сессии P3): заведено не в `ROADMAP.md`, а в
`CSS-SPECS.md` — P4 и так владеет этим файлом как очередью CSS-свойств, а
`<position>`-синтаксис относится к CSS Values L4/L5 (уже 🟡-модуль).
Статус переведён в `OPEN (ДОРАБОТКА → CSS-SPECS.md)`; строка снята с
`STATUS-P3.md`.
