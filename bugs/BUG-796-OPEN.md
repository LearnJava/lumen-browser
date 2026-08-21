# BUG-796 — собственный `content` на обёртке каждого элемента затеняет рефлекторный `HTMLMetaElement.content`; из-за этого WPT режет `timeout: long` по 10-секундному потолку

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-5, срез 29 — разбор стоимости TIMEOUT в корпусном прогоне)
**Область:** `crates/js/src/dom.rs` — обёртка элемента (`_lumen_make_element`, литерал со свойством `get content()`, «HTMLTemplateElement.content (HTML LS §4.12.3)») против таблицы рефлексии `_lumen_install_reflection(HTMLMetaElement.prototype, [... ['content','content','string'] ...])`
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`document.getElementsByTagName('meta')[0].content` возвращает **`undefined`**,
хотя атрибут в разметке есть и `getAttribute('content')` отдаёт его правильно.
`name`/`httpEquiv` при этом рефлектятся нормально.

Проба (`--dump-layout`, сборка `dev-release` ветки `p2-wpt-run-5`):

```html
<!doctype html>
<meta name="timeout" content="long">
<body><script>
var m = document.getElementsByTagName("meta")[0];
document.body.textContent = "NAME=" + m.name + " CONTENT=" + m.content
  + " ATTR=" + m.getAttribute("name") + "/" + m.getAttribute("content");
</script></body>
```

```
seg[0] "COUNT=1 NAME=timeout CONTENT=undefined ATTR=timeout/long"
```

Расширенная проба: `m0.content=undefined`, `m1.httpEquiv=refresh`,
`m1.content=undefined`, `m2.charset=undefined`, `link.rel=stylesheet`,
`link.href=ok`, `proto=HTMLMetaElement`.

## Причина (локализована)

Свойство **есть** в таблице рефлексии (`dom.rs`,
`_lumen_install_reflection(HTMLMetaElement.prototype, …)`, строка
`['content', 'content', 'string']`), и геттер на прототипе установлен —
но обёртка элемента объявляет **собственное** свойство `content` на *каждом*
элементе, а не только на `<template>`:

```js
// HTMLTemplateElement.content (HTML LS §4.12.3) — returns the template's
// DocumentFragment content container, or null when not a template element.
get content() {
    if ((_lumen_get_tag_name(nid) || '').toUpperCase() !== 'TEMPLATE') return undefined;
    ...
}
```

Собственное свойство побеждает прототипное, поэтому для `<meta>` вызывается
шаблонный геттер и возвращает `undefined`. Проверено дескрипторами прямо в
странице:

```
own=content | protoHasContent=true | protoHasName=true
ownDesc=get/set/enumerable/configurable:undefined | elemProtoIsMeta=true
get=function
```

То есть на прототипе рабочий геттер (`get=function`), а на самом объекте —
собственный аксессор, отдающий `undefined`.

## Во что это обходится WPT

`testharness.js` определяет свой внутренний потолок так
(`tests/wpt/resources/testharness.js:226`):

```js
var metas = document.getElementsByTagName("meta");
for (var i = 0; i < metas.length; i++) {
    if (metas[i].name === "timeout") {
        if (metas[i].content === "long") return settings.harness_timeout.long;  // 60 000
        break;
    }
}
return settings.harness_timeout.normal;   // 10 000
```

`metas[i].name` совпадает, `metas[i].content` — `undefined`, срабатывает
`break`, и тест, объявленный `timeout: long`, получает **10 с вместо 60 с**.

Измерено на живом корпусном прогоне (Linux-половина, 384 шарда): для
`long`-объявленных id, доехавших до TIMEOUT, медианная длительность
**бимодальна** — 31 шард с ≥20 `long`-id распадаются на 15 шардов с медианой
10.2 с и 16 шардов с медианой 65.0 с. Вторая мода — внешний потолок
`wptrunner` (60 + 5 с `extra_timeout`), он применяется, когда гарнесс до
отчёта не доехал вовсе; первая — как раз наш случай: гарнесс работает и сам
себя убивает по «нормальному» потолку. Пример: `referrer-policy`, 1 325 из
1 390 id объявлены `long`, все проверенные исполненные id — тоже `long`
(`Counter({'long': 192})`), а медиана их TIMEOUT — 10.2 с. В файле теста
`<meta name="timeout" content="long">` присутствует.

Масштаб: `long`-объявленных id в корпусе **8 196** (12 % от 67 735,
измерено WPT-RUN-5 срезом 15). Каждый такой тест, который на нашем движке
доезжает до гарнесса, но не успевает за 10 с, получает ложный TIMEOUT —
занижение публикуемой цифры pass-rate. Обратная сторона: после починки
корпусный прогон подорожает (такие тесты будут висеть 65 с вместо 10 с) —
бюджет шарда, выведенный из объявленных потолков (`run_corpus.py::shard_timeout`,
срез 15), это уже учитывает.

Вне WPT задет обычный веб-код: `<meta name="viewport">`, `og:*`, CSP,
`http-equiv`-разбор из JS — любой скрипт, читающий `meta.content`, получает
`undefined` вместо строки.

## Побочно найденное (тем же зондом)

`HTMLMetaElement.charset` не реализован вовсе — его нет в таблице рефлексии
(там `name`/`content`/`httpEquiv`/`media`). Отдельный, более мелкий пробел;
чинить логично тем же коммитом.

## Направление починки (не предписание)

`content` — IDL-атрибут `HTMLTemplateElement`, а не общего элемента, поэтому
его место — на `HTMLTemplateElement.prototype`, а не в общем литерале обёртки.
Прототипы у нас уже потеговые: проба показывает
`tproto=HTMLTemplateElement`, `t.content` отдаёт объект-фрагмент, то есть
перенос геттера туда достижим. Любой вариант, при котором на не-`<template>`
собственного `content` не остаётся, закрывает баг; проверять — приведённой
выше однострочной пробой плюс `t.content instanceof DocumentFragment` на
`<template>`.

## Как проверить фикс

1. Проба выше печатает `CONTENT=long`, а не `CONTENT=undefined`.
2. `<template>` не сломан: `t.content` по-прежнему отдаёт один и тот же
   фрагмент между обращениями (`t.content === t.content`).
3. На WPT: `tests/wpt/run_report.py` по `referrer-policy` — медиана
   длительности TIMEOUT-ов уезжает с ~10 с на ~65 с (тесты те же, потолок
   правильный), а тесты, укладывающиеся между 10 и 60 с, перестают быть
   TIMEOUT.
