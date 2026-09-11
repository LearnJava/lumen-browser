# BUG-1047: `position:absolute` + `width:auto` + только `right` (без `left`) растягивается на всю ширину containing block вместо shrink-to-fit

**Статус:** OPEN
**Найден:** P6, живой сайт (`bankruptcy-platform`, внешний Keycloak-стенд), 2026-09-11

## Симптом

Иконка-кнопка «показать/скрыть пароль» внутри `<input type="password">`
перехватывает клики и ввод текста по всей ширине поля, а не только в
области самой иконки — `click`/`type` через MCP на сам `#password` либо
попадают в кнопку («Element click intercepted: point (x, y) hits
`<button>` … instead of the target element»), либо, если клик попадает по
чистому `{point:…}` (в обход BUG-1044 hit-check), фокус всё равно уходит
на `<button>`, и последующий `type` падает с `Element is not a mutable
text field` — сам `<input>` при этом никогда не получает фокус штатным
кликом.

## Причина

Разметка — типичный паттерн «иконка внутри инпута»:

```html
<div class="password-wrapper" style="position:relative; width:420px">
  <input id="password" type="password" class="field-input">
  <button class="eye-btn" style="position:absolute; right:12px; /* left не задан */">
    <svg>…</svg>
  </button>
</div>
```

Замер через `getComputedStyle`/`getBoundingClientRect` в живом окне:

```
wrapper: {rect: [1153.2, 257.18, 420, 48]}
button:  {rect: [1141.2, 281.18, 420, 21], computedWidth: "auto",
          computedPosition: "absolute", computedInsetRight: "12px"}
```

У кнопки `position:absolute; width:auto; right:12px`, **`left` не
указан**. По CSS 2.1 §10.3.7 (resolution algorithm для абсолютно
позиционированных блоков, «Rule 1»–«Rule 3») это ровно тот случай, где
`left` — `auto`, и при `width:auto` кнопка должна получить width по
shrink-to-fit (по содержимому — SVG-иконка, реалистично ~16–20px), а не
растягиваться до ширины containing block. Lumen же выдаёт кнопке
`width: 420px` — буквально ширину `.password-wrapper`, как будто заданы
ОБА инсета (`left:0; right:12px`), из-за чего блок обязан растянуться.

Итоговый layout-бокс кнопки перекрывает практически весь `<input>` по
ширине (кнопка `x=1141` при wrapper `x=1153` — даже чуть шире и левее
самого инпута), поэтому hit-test в любой точке поля находит `<button>`
раньше `<input>`.

Не локализовано глубже getComputedStyle/getBoundingClientRect — сам код
резолва auto-width для `position:absolute` с одним заданным инсетом не
найден (`crates/engine/layout/src/box_tree/*`, кандидаты `bfc.rs`,
`multicol_abspos.rs` — ни один не содержит явной реализации CSS 2.1
§10.3.7 «Rule 1-3» resolution, возможно свёрнуто в общий intrinsic-width
путь `box_tree/intrinsic.rs`).

## Как воспроизвести

Живой стенд `http://my-stand.local/` (bankruptcy-platform, Keycloak login
form), поле `#password`. Или минимальный кейс — любая страница с:

```html
<div style="position:relative">
  <input>
  <button style="position:absolute; right:12px">×</button>
</div>
```

и сравнить `button.getBoundingClientRect().width` с шириной обёртки —
в Chrome/Edge будет узкой (по содержимому), в Lumen равна ширине обёртки.

## Влияние

Блокирует MCP-автоматизацию (`click`/`type`) на любом поле с подобным UI
паттерном («показать пароль», «очистить поле», «поиск с иконкой» и т.п.)
— крайне распространённая вёрстка. Не блокирует обычного пользователя
мышью (клик по видимой иконке всё равно физически в её пределах, хотя
теперь и остальная площадь поля тоже кликабельна как кнопка — с реальной
мышью это может давать неожиданный «щелчок мимо» при клике рядом с текстом
пароля).

## Побочная находка на том же стенде

Изначально принят за баг зависший после логина дашборд
(`TypeError: Cannot read properties of null (reading 'data')` в
`react-hot-toast`/`goober`, инжекция стилей через `styleEl.firstChild.data`)
— на момент обнаружения тестовый `lumen.exe` был собран **до** правки
BUG-982 (`innerHTML` теперь верно сохраняет whitespace-only фрагмент),
пересборка `dev-release` полностью убрала симптом. Отдельного бага не
заводится — ложная тревога от устаревшего бинарника, а не факт неполадки.
