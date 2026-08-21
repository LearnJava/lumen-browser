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
измерено WPT-RUN-5 срезом 15); на Linux-половине прогона 1 745 из них
получили TIMEOUT именно по внутреннему 10-секундному потолку (вторая мода,
449 id, — внешний потолок `wptrunner`, туда баг не дотягивается).

**Цена измерена, и она не в цифре (WPT-RUN-5 срез 30).**
`tests/wpt/long_timeout_audit.py` прогоняет A/B на одном бинаре: арм A —
страница как её получил корпусный прогон, арм B — та же страница с шимом
перед её `testharness.js`, чинящим `content` на уже разобранных `<meta>`
(`timeout_multiplier` в обоих армах 1, то есть арм B — это «баг починен», а
не «больше времени всем»). Выборок две — 48 случайных из всей популяции и 40
из страты «в исходнике нет структурной причины висеть» (вторая снимает
возражение, что первая меряет в основном невендоренные хелперы), всего **88
id**. Восстановились **два**
(`/fetch/range/non-matching-range-response.html` 10.2 → 30.1 с `OK`,
`/long-animation-frame/loaf-forced-layout-duration.html` 10.3 → 18.2 с `OK`)
и **оба на нулевой балл**. Сабтестов суммарно 475 против 475, PASS 15 против
15, и ни у одного из 88 id их число между армами не изменилось: за вшестеро
большее время гарнесс не добрал ничего. Верхняя граница занижения по правилу
трёх — **≤ 105 id из 3 079, то есть ≤ 0.16 п.п.** Причина: 2 167 из 2 504
отдаваемых напрямую id этой популяции грузят хелпер, которого в чекауте нет
(`/common/security-features/resources/common.sub.js`,
`/common/subset-tests.js`), то есть ждут не времени, а файла.

Обратная сторона осталась в силе и стала числом: после починки корпусный
прогон подорожает — выборки стоили 11.6 против 48.4 и 9.9 против 40.0 мин
(**×4.1–4.2**), в пересчёте на 3 079 срезанных внутренним потолком id это
**+42.8 ч** тестового времени на полный проход (≈7 ч настенных при
`--processes 6`). Бюджет шарда, выведенный
из объявленных потолков (`run_corpus.py::shard_timeout`, срез 15), это уже
закладывает; плоский `--shard-timeout-per-id` после фикса применять нельзя.

Иначе говоря, чинить баг стоит не ради pass-rate, а ради корректности
движка (см. ниже про обычный веб-код) — и чинить его дешевле **после** того,
как довендорены хелперы (`WPT-RUN-11`), иначе прогон купит 24 часа ожидания
тестов, которые всё равно ничего не проверяют.

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
