# BUG-468: percentage margin/padding not re-resolved after JS `style.width` mutation on the containing block

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** layout (percentage resolution against containing block, JS-driven
relayout)
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2`, 8
файлов `css/CSS2/normal-flow/containing-block-percent-{padding,margin}-{left,right,top,bottom}.html`
(1/1 сабтест FAIL в каждом)

## Симптом

Все 8 файлов используют один и тот же паттерн:

```html
<div id="container" style="width:123px;">
  <div data-expected-width="100" data-expected-height="100"></div>
</div>
<script src="/resources/check-layout-th.js"></script>
<script>
  document.body.offsetTop;                                     // форс начального layout
  document.getElementById("container").style.width = "500px";  // мутация контейнера
  checkLayout("#container");                                    // немедленная проверка
</script>
```

Дочерний блок задаёт `padding-left:10%`/`margin-top:50%`/и т.п. (проценты
резолвятся относительно ширины containing block, CSS2.1 §10.2/§8.3). После
того как ширина `#container` меняется через JS **после** начального layout,
`checkLayout` ожидает, что процентный отступ пересчитан относительно новой
ширины (100px = 10% от 500px + 50px content, и т.п.). Lumen во всех 8 случаях
измеряет `0` вместо ожидаемого значения — либо релейаут после JS-мутации
`style.width` не применяется к процентным margin/padding потомка вовсе, либо
резолюция процента против только что изменённого containing block ломается.

## Влияние вне WPT

Динамическая смена ширины контейнера через JS (`element.style.width = ...`) —
обычный паттерн (responsive-виджеты, resize-обработчики). Если процентные
отступы дочерних элементов не пересчитываются, любой такой код визуально
"залипает" на старой раскладке до следующего relayout-триггера другого рода.

## .ini

`tests/wpt/metadata/css/CSS2/normal-flow/containing-block-percent-{padding-left,padding-right,padding-top,padding-bottom,margin-left,margin-right,margin-top,margin-bottom}.html.ini`
— по одному сабтесту `expected: FAIL` в каждом.
