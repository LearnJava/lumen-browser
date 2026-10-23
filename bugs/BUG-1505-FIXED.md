# BUG-1505 — `CSS.registerProperty`: начальное значение не видно в `getComputedStyle`, значения не абсолютизируются и не вычисляются, неверное значение принимается, повторная регистрация не бросает, `CSS.unregisterProperty` нет

**Статус:** FIXED 2026-10-23 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** js/layout (`crates/js/src/css_properties_values_api.rs` — `CSS.registerProperty`; `crates/engine/layout/src/style/property_syntax.rs`)

## Симптом

`@property --p{syntax:'<length>';inherits:false;initial-value:100px}` даёт `getComputedStyle(el).getPropertyValue('--p')` = `100px`; тот же набор через `CSS.registerProperty({name,syntax,inherits,initialValue})` — пустая строка (для `inherits:true` тоже). Следствия: переходы по зарегистрированным свойствам падают на первом `assert_equals('Element has the expected initial value')` (53 сабтеста), кадры анимации с одиночным значением не видят «подлежащее» (30 сабтестов). Помимо этого: значение `2em` у `<length>` не абсолютизируется (`2em`, ожидается `20px` при `font-size:10px`); `calc(1 + 2)` у `<number>` не вычисляется (`calc(1 + 2)`, ожидается `3`); `setProperty('--p','red')` у `<length>` принимается (`red`), а должно быть проигнорировано; второй `registerProperty` с тем же именем не бросает `InvalidModificationError`; `CSS.unregisterProperty` — `undefined`.

## Проба

Проба (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `@property` с `initial-value:100px` → `gcs.getPropertyValue('--pi')` | `100px` | `100px` |
| `registerProperty({name:'--ri',syntax:'<length>',inherits:false,initialValue:'100px'})` → то же | `` | `100px` |
| то же с `inherits:true` | `` | `100px` |
| `style.setProperty('--m','2em')`, `font-size:10px` | `2em` | `20px` |
| `<number>`, `setProperty('--n','calc(1 + 2)')` | `calc(1 + 2)` | `3` |
| `<length>`, `setProperty('--ro','red')` | `red` | `` (отвергнуто) |
| второй `registerProperty` того же имени | не бросает | `InvalidModificationError` |
| `typeof CSS.unregisterProperty` | `undefined` | `function` |

## Как найдено

WPT-RUN-14 срез 22: `css-properties-values-api/animation/custom-property-transition-*` (28 id, 53 сабтеста), `*-single-keyframe` (30 сабтестов), `registered-property-{computation,initial,cssom,inheritance}`, `register-property*`, `determine-registration`, `unit-cycles`, `var-reference-registered-properties*` — ещё 39 id (31 OK с падениями) вне `animation/`.

## Что делать

Подключить регистрацию из JS к тому же реестру, что и `@property` (там начальное значение работает); вычислять значение по синтаксису (`calc`, относительные единицы), отвергать несоответствующее; `unregisterProperty`; исключение при повторе.

## Как проверить

Таблица выше; `css/css-properties-values-api/register-property.html`, `registered-property-initial.html`, `animation/custom-property-transition-length.html`.

## Исправление (2026-10-23, P3)

- Регистрации из `CSS.registerProperty` лежат в `Document::registered_custom_properties`
  (`crates/engine/dom/src/lib.rs`) и идут в реестр каскада после `@property`-правил листа
  (регистрация из скрипта перебивает правило с тем же именем). Повторная регистрация —
  `InvalidModificationError`, `CSS.unregisterProperty` — `NotFoundError` для неизвестного имени
  (`crates/js/src/v8_runtime/install/registered_properties.rs`).
- `compute_registered_values` (`style/property_syntax.rs`) после вычисления `font-size` элемента
  приводит значение по `syntax` к computed-виду: `2em` → `20px`, `calc(1 + 2)` → `3`,
  `calc(19em - 2%)` → `calc(-2% + 190px)`, списки `<length>#` поэлементно (`syntax_string.rs`).
  Значение, не подходящее под `syntax`, игнорируется (`setProperty('--p','red')` у `<length>`).
- `CustomFlow::try_new` отключается, пока есть script-регистрации (анализ зависимостей не видит
  вычисленных значений).
- `expand_custom_property_value` (`style/substitute.rs`): собственное имя свойства считается «в
  разворачивании», поэтому `--a:var(--b,x); --b:var(--a,y)` даёт пустое значение у обоих, а не `x`.
- Shim Web Animations (`_wa_normalize_keyframes`): одиночное значение в property-indexed форме
  (`animate({'--p':'200px'})`) — список из одного кадра, подлежащее значение берётся нейтральным
  кадром.
- WPT `css/css-properties-values-api`: подтесты 470 → 640 из 1036 (`registered-property-computation`
  20 → 44/75, `registered-property-initial` 0 → 16/29, `custom-property-animation-*` одиночные кадры).

Остаток (не дефект этого бага): CSS-переходы по зарегистрированным свойствам не стартуют
(`custom-property-transition-*`: «A single animation is running expected 1 but got 0») — отсутствует
функциональность в движке переходов; интерполяция `<length-percentage>` в `calc()`-смесях
(`custom-property-animation-length-percentage*`); `registered-property-revert`,
`at-property-viewport-units*`. Три подтеста `registered-property-computation` с `vw`/`vmin`/`vmax`
перешли из PASS в FAIL, потому что эталон `getComputedStyle(min-height)` не абсолютизирует
viewport-единицы — BUG-1698.
