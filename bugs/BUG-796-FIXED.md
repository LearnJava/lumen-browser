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

## Перемер WPT-RUN-6, срез 26 (2026-08-23)

Подтверждён на `main` = `c14b8068c` живым замером
(`tests/wpt/verify_worker_port_storage_gaps.py --variant harness-timeout-meta`,
вариант повторяет цикл `WindowTestEnvironment.prototype.test_timeout`
дословно). Дескрипторы, снятые с одного и того же элемента, показывают
затенение прямее, чем прежняя проба:

```
htm-meta-1 ctor=HTMLMetaElement name="timeout" content=undefined
           getAttr-name="timeout" getAttr-content="long"
htm-verdict normal
htm-desc own=accessor:function proto=accessor:function
htm-via-proto "long"
htm-template-control "[object Object]"
```

То есть прототипный геттер на этом же `<meta>` отдаёт `"long"`
(`htm-via-proto`), а собственный — `undefined`; `<template>.content`
при этом исправен, так что перенос геттера на
`HTMLTemplateElement.prototype` ничего не ломает.

**Уточнение масштаба по снимку WPT-RUN-5** (обе половины прогона, 15 592
TIMEOUT с измеримой длительностью): из 3 558 TIMEOUT-id, чей исходник
объявляет `<meta name=timeout content=long>`, **2 933 обрезаны примерно на
10 с** и лишь 625 дожили до внешнего потолка. Разделение чистое и
проверяемое: у всех 625 гарнесс не доложил ничего вовсе (`test_status`
отсутствует, режет `wptrunner` на своих 60 + 5 с), а у обрезанных на 10 с
стоит собственное гарнессовое `«Test timed out»` при `test_timeout: 60`
в `extra` — то есть решение принято страницей, а не раннером.

**Два id остатка теперь стоят прямо на этом баге** —
`websockets/keeping-connection-open/001.html` (`?default`, `?wss`): тест
объявлен `timeout=long` и по своей природе идёт 20 с, поэтому при
10-секундном потолке он не может пройти ни при какой починке WebSocket-ов.
Ранее [BUG-869](BUG-869-OPEN.md) числил их за собой; там это исправлено.
В `tests/wpt/timeout_audit.py` механизм называется
`harness-long-timeout-ignored`.

## Починено 2026-08-24 (P1, ветка `p1-bug796-meta-content`)

**Статус:** FIXED.

Геттер `content` перенесён из общей таблицы членов обёртки
(`_LUMEN_WRAPPER_MEMBERS`, `crates/js/src/dom.rs`) на
`HTMLTemplateElement.prototype` — туда, где его место по IDL. С момента
[BUG-849](BUG-849-FIXED.md) эта таблица ставится не на каждый узел, а на общий
прототип-на-интерфейс, который стоит **ниже** интерфейсного прототипа, поэтому
затенение сохранялось ровно в прежнем виде: собственного свойства на элементе
уже не было, но `HTMLMetaElement.prototype`'s `content` всё равно проигрывал.
Прототип для тега `TEMPLATE` раздаёт `_lumen_element_prototype_for`, так что
после переноса дотянуться до геттера может только обёртка `<template>`.

Заодно закрыт `[SameObject]`: узел фрагмента был стабилен со времён BUG-368, а
вот **обёртка** создавалась заново на каждое чтение
(`_lumen_make_document_fragment` — литерал без интернирования), поэтому
`t.content !== t.content` и экспандо на фрагменте терялось между обращениями.
Обёртка теперь кэшируется на обёртке элемента, а та интернирована по nid
(BUG-291).

**Побочное найденное этим багом исправлено в другую сторону, чем предлагалось.**
`HTMLMetaElement.charset` добавлять было бы неправильно: IDL-атрибута `charset`
у этого интерфейса нет вовсе — сверено с вендоренным
`tests/wpt/interfaces/html.idl:234-243`, где перечислены `name`/`httpEquiv`/
`content`/`media`, а в устаревшем partial (`:3043`) — `scheme`. Добавлен
`scheme`; `charset` остаётся контентным атрибутом без рефлексии, как в спеке.

### Замер

Проба из «Как проверить фикс», `--dump-layout`, сборка `dev-release` ветки:

```
M0NAME=timeout M0CONTENT=long M0ATTR=long M1HE=refresh M1CONTENT=5
TFRAG=true TKIDS=1 TSAMEID=true TSAMEOBJ=true DIVHAS=false
```

До фикса `M0CONTENT=undefined` и `TSAMEOBJ=false`. `DIVHAS=false` —
`'content' in div` больше не истинно, то есть шаблонный член ушёл со всех
прочих элементов.

Юнит-тесты (`crates/js/src/dom.rs`, `cargo test -p lumen-js --features
v8-backend`): `content_is_a_template_member_only`,
`meta_content_reflects_its_attribute`,
`meta_content_is_writable_through_the_idl_attribute`,
`template_content_is_the_same_wrapper_object` — плюс четыре прежних
`template_content_*`/`template_inner_html_*`, оставшихся зелёными.

WPT, A/B на одной машине одним и тем же прогоном (`run_report.py --all --root
… --recursive`), арм «до» — та же ветка с `git stash` правки:

| Категория | до | после | время до | время после |
|---|---|---|---|---|
| `html/semantics/scripting-1/the-template-element` (24 id) | 22/24 harness, 447/660 сабтестов | 22/24, 447/660 | 0:23.1 | 0:24.0 |
| `html/semantics/document-metadata` (109 id) | 74/109 harness, 85/394 сабтестов | 74/109, 85/394 | 6:12.8 | 16:14.1 |

То есть регрессии нет и прироста нет — ровно то, что предсказал срез 30
(«восстановились два id и оба на нулевой балл»), — а **обратная сторона видна
прямо здесь**: та же категория с теми же результатами стала стоить в 2.6 раза
дороже, потому что висящие `timeout: long`-тесты теперь висят по правильным
60 с вместо 10. На `the-template-element` разницы нет: там `long`-тестов
попросту нет. Ожидаемое подорожание полного корпуса — +42.8 ч тестового
времени (замер среза 30, см. выше); плоский `--shard-timeout-per-id` после
этого коммита применять нельзя.
