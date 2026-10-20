# BUG-1392 — `element.style.webkitXxx` пишет в атрибут `webkit-xxx` без дефиса и не меняет раскладку

**Статус:** FIXED 2026-10-20 (P6)
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 16, `css/css-overflow` + `css/css-sizing`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — преобразование camelCase → kebab для `element.style`)

## Симптом

`<div id=a style="width:50px">`, затем скрипт:

| присваивание | `getAttribute('style')` | ожидается |
|---|---|---|
| `a.style.webkitLineClamp = 2` | `…; webkit-line-clamp: 2;` | `-webkit-line-clamp: 2` |
| `a.style.webkitTransform = 'translateX(50px)'` | `…; webkit-transform: translateX(50px);` | `-webkit-transform: …` |
| `a.style.WebkitUserSelect = 'none'` | `-webkit-user-select: none;` (верно) | |
| `'webkitTransform' in a.style` | `false` | `true` |

`style.setProperty('-webkit-line-clamp','2')` и `cssText` работают. Верно обрабатывается только форма с заглавной `W` (`WebkitX`); строчная `webkitX` — тоже допустимая по CSSOM (§6.7.1) — теряет дефис.

## Как найдено

WPT-RUN-14 срез 16: `css-overflow/line-clamp/webkit-line-clamp-{017,018,019,020,026}.html` — выставляют `style.webkitLineClamp = 3` и ждут усечения (+ 1 `css-ui/tentative/webkit-user-modify-01.html`).

## Что делать

В преобразовании camelCase → kebab: `webkit[A-Z]…` → `-webkit-…`; `in style` — по списку известных свойств.

## Как проверить

`css/css-overflow/line-clamp/webkit-line-clamp-017.html`.

## Исправление

К моменту взятия задачи преобразование `webkitX` → `-webkit-x` уже стояло в `_lumen_camel_to_kebab` (COMPAT-CSS-ALIAS), и `webkit-line-clamp-017/018/019/020/026` давали пиксель-в-пиксель эталон (проверено `--screenshot`, тест и ref). Не хватало одного: `'webkitLineClamp' in el.style` было `false` — `_lumen_is_compat_alias_attr` считал IDL-атрибутами только Compat §3 алиасы и `webkitMaskBoxImage*`. Добавлен список `_LUMEN_WEBKIT_NATIVE_ATTRS` (реализованные не-алиасные `-webkit-*`: `line-clamp`, `box-orient/align/pack/flex/direction/ordinal-group`, `text-fill-color`).

Тест: `crates/js/src/dom/tests/compat_css_alias.rs::lowercase_webkit_attribute_of_native_property_keeps_leading_dash`.
