# BUG-1505 — `CSS.registerProperty`: начальное значение не видно в `getComputedStyle`, значения не абсолютизируются и не вычисляются, неверное значение принимается, повторная регистрация не бросает, `CSS.unregisterProperty` нет

**Статус:** OPEN
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
