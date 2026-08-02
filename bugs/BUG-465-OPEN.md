# BUG-465: computed/reflected color values are not serialized to canonical `rgb()` form

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** css-parser / layout (color serialization, `CSSStyleDeclaration`
value reflection)
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2`,
`css/CSS2/syntax/colors-007.html` (904/1192 сабтестов FAIL — крупнейший
единичный кластер во всём срезе)

## Симптом

```
FAIL e.style['color'] = "#ffffff" should set the property value
  - assert_equals: serialization should be canonical expected "rgb(255, 255, 255)" but got "#ffffff"
FAIL e.style['color'] = "rgb(100%, 100%, 100%)" should set the property value
  - assert_equals: serialization should be canonical expected "rgb(255, 255, 255)" but got "rgb(100%, 100%, 100%)"
```

Per CSS Color Module (§4.2/§8, CSSOM §6.7.3), reading back a color value from
`CSSStyleDeclaration`/`getComputedStyle` must always serialize to the
canonical `rgb(r, g, b)` (или `rgba(...)` с альфой) form with integer 0-255
channels, независимо от того, каким синтаксисом цвет был задан (`#fff`,
`#ffffff`, `rgb(50%, ...)`, `rgb(+255, ...)`, legacy-запятые/без запятых и
т.п.). Lumen возвращает исходную запись почти без изменений — сериализация
цвета в объект `style`/computed style не реализована вовсе, а не просто
неточна (сравнивались десятки разных входных форм для одного и того же цвета,
все дают неканонический вывод).

## Влияние вне WPT

Любой код, читающий `el.style.color`/`getComputedStyle(el).color` и
сравнивающий его с ожидаемой строкой (частый паттерн в тестах и в
color-picker/theming коде), видит непредсказуемый формат вместо
гарантированного спекой канонического.

## .ini

`tests/wpt/metadata/css/CSS2/syntax/colors-007.html.ini` — 904 сабтеста
`expected: FAIL` (полный список конкретных серилизаций — в самом `.ini`, не
дублируется здесь).
