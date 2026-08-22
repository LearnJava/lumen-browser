# BUG-827 — MutationObserver не видит узлы, вставленные парсером: записей о них нет вообще

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 20 — 3 TIMEOUT остатка, механизм `mutation-record-parser-insert`)
**Область:** `crates/js/src/dom.rs:9275` (`_mo_notify` — ровно 6 call site'ов, все внутри собственных обёрток шима: `setAttribute` :9347, `innerHTML` :9361, `appendChild` :9369, `removeChild` :9378, `textContent` :9393/:9398; вызывающих из `crates/shell/` нет ни одного), `crates/shell/src/main.rs` (парсерный путь построения дерева — JS-стороне о вставках не сообщает; та же граница, что у [BUG-804](BUG-804-OPEN.md))
**Владелец:** P1/P3 (`lumen-js` + шелл). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Наблюдатель, поставленный на `document.documentElement` с
`{childList: true, subtree: true}`, не получает ни одной записи о том, что
парсер пишет следом за ним в тот же документ:

```html
<script>
new MutationObserver(records => { /* никогда не вызывается */ })
  .observe(document.documentElement, { childList: true, subtree: true });
</script>
<div id="psag-parsed-div">parsed after the observer</div>
<script id="psag-parsed-script">/* этот скрипт выполняется */</script>
```

Скрипт ниже наблюдателя **исполняется** (то есть узлы в дерево попали и
дошли до JS), а записи о них нет.

Практическое следствие — семейство `html/dom/render-blocking`: его хелпер
ждёт элемент через наблюдателя, чтобы поймать его до загрузки ресурса,

```js
// html/dom/render-blocking/support/test-render-blocking.js
function nodeInserted(parentNode, predicate) { … new MutationObserver(callback)
    .observe(parentNode, {childList: true}); }
// html/dom/render-blocking/remove-attr-script-keeps-blocking.html
promise_setup(async () => {
  let script = await nodeInserted(document.head, node => node.id === 'script');
  …
});
```

`promise_setup` не завершается никогда, поэтому **ни один** тест файла не
стартует: снаружи это выглядит как молчащая страница без единой строки в
логе.

## Прямое измерение

`tests/wpt/verify_preload_script_audio_gaps.py` (2026-08-22, коммит
`79f7df91a`, `--seconds 5`, обе пробы живы — по 9 тиков):

| проба | ожидалось | получено |
|---|---|---|
| `mutationobserver-parser-inserted` | `mo-added DIV id=psag-parsed-div`, `mo-added SCRIPT id=psag-parsed-script` | `mo-armed`, `parsed-script-2-ran` — и ни одной записи |
| `mutationobserver-script-inserted` (контроль) | `mo-added DIV id=psag-js-div` | ровно это |

Контроль решает вопрос: наблюдатель как таковой работает, `childList`
доставляется, `addedNodes` заполнен — молчит именно парсерная вставка.

## Причина (локализована чтением кода)

Записи ставятся в очередь на JS-стороне, в тех методах шима, которые сами
меняют дерево: у `_mo_notify` (`dom.rs:9275`) шесть call site'ов, и все шесть —
обёртки `setAttribute`/`innerHTML`/`appendChild`/`removeChild`/`textContent`;
из `crates/shell/` его не зовёт никто. Дерево, которое строит парсер,
приходит в JS уже готовым:
шелл разбирает документ своим путём и не сигналит о вставках — та же
граница, из-за которой парсерные `<script>`/`<link>`/`<style>` не получают
`load`/`error` ([BUG-804](BUG-804-OPEN.md)). Отсюда и симметрия симптомов:
там нет события об **окончании загрузки** парсерного элемента, здесь — о
самом факте его **вставки**.

По спеке (DOM §4.3 «queue a mutation record») источник записи — сам шаг
«insert a node», а не конкретный API: узел, вставленный парсером, обязан
дать `childList`-запись ровно так же, как `appendChild`.

## Масштаб

Механизм `mutation-record-parser-insert` забирает **3 id** остатка снимка
WPT-RUN-5 — три `remove-attr-*-keeps-blocking.html` из
`html/dom/render-blocking` (это все файлы корпуса, которые *зовут*
`nodeInserted`; остальные шесть того же каталога висят на BUG-804 и
разобраны там).

Оценка снизу, и заметно: маркер намеренно узкий — он ловит только вызов
хелпера с этим именем. Любой тест, который ставит наблюдателя вручную и
ждёт парсерную вставку (спекулятивный парсинг, `html/syntax`,
lazy-загрузчики), сейчас списан на другие механизмы или сидит в остатке.
Вне WPT цена — целый класс страничных приёмов: наблюдатель за `<head>`,
чтобы поймать вставку рекламного/аналитического тега, на Lumen молчит.

## Направление починки (не предписание)

Общая с BUG-804 точка: дать парсерному пути шелла уведомлять JS-сторону о
вставленных узлах — тогда одна и та же нотификация закрывает и запись
мутации, и `load`/`error` элемента. Дешёвый первый шаг — сообщать о вставках
в `<head>`/`<body>` пакетами по chunk-у стриминга: запись обязана уйти в
микротаск-очередь наблюдателя, а не синхронно, так что батчинг спеке не
противоречит.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_preload_script_audio_gaps.py
   --variant mutationobserver-parser-inserted` — печатает обе строки
   `mo-added`.
2. WPT: `run_report.py --all --root html/dom/render-blocking` — три
   `remove-attr-*-keeps-blocking` перестают висеть на `promise_setup`.
