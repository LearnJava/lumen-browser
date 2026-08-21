# BUG-804 — `load`/`error` не диспатчатся для `<script>`/`<link>`/`<style>`, вставленных ПАРСЕРОМ, и для `<style>` вообще ни при какой вставке

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 12 — `html/semantics/document-metadata/the-style-element`, `html/semantics/scripting-1/the-script-element`)
**Область:** `crates/js/src/dom.rs` — `_lumen_resource_track` (строка ~5712: белый список тегов `'script'|'link'`, `'style'` отсутствует) и вся машинерия `_lumen_resource_*`, доступная только элементам из `createElement`/`createElementNS`; `crates/shell/src/main.rs` — парсерный путь загрузки подресурсов (`Загружен скрипт:` / `Загружен CSS:` / `Пропуск скрипта…` / `Пропуск CSS…`), который грузит ресурс и НИЧЕГО не сообщает JS
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Тест ждёт `load` (или `error`) от `<script src>`, `<link rel=stylesheet>`
или `<style>` и висит до таймаута хёрнесса — вместо FAIL получается
TIMEOUT, потому что `testharness.js` не успевает опубликовать
`harness_status`. Живой прогон:

```
tests/wpt/run_report.py --all --root html/semantics/document-metadata/the-style-element --recursive
  → tests: 11/26 harness OK
  NOTRUN If the style is loaded successfully, the 'load' event must be fired
  NOTRUN If the style is loaded unsuccessfully, the 'error' event must be fired
  TIMEOUT /html/semantics/document-metadata/the-style-element/style_events.html
  TIMEOUT /html/semantics/document-metadata/the-style-element/style_load_async.html
  TIMEOUT /html/semantics/document-metadata/the-style-element/style-load-after-mutate.html
  TIMEOUT /html/semantics/document-metadata/the-style-element/style-error-01.html

tests/wpt/run_report.py --all --root html/semantics/scripting-1/the-script-element --recursive
  → tests: 298/434 harness OK, 233 TIMEOUT
  NOTRUN Test that the insertion point is defined in the load event of a parser-inserted script.
  TIMEOUT /html/semantics/scripting-1/the-script-element/script-onload-insertion-point.html
  NOTRUN no src, parser-inserted, has style sheets blocking scripts, script nesting level == 2
  TIMEOUT /html/semantics/scripting-1/the-script-element/load-error-events-3.html
```

## Воспроизведение (A/B на одной странице, живое окно)

Страница отдаётся по http (`python3 -m http.server`), браузер —
`lumen --mcp-live-port N http://127.0.0.1:PORT/p2.html`; каждый обработчик
печатает строку в консоль:

```html
<style  onload="log('parser style onload')"  onerror="log('parser style onerror')">#a{color:red}</style>
<link rel=stylesheet href="ext.css"      onload="log('parser link onload')"      onerror="log('parser link onerror')">
<link rel=stylesheet href="missing.css"  onload="log('parser link404 onload')"   onerror="log('parser link404 onerror')">
<script src="ext.js"          onload="log('parser script onload')"    onerror="log('parser script onerror')"></script>
<script src="missing-404.js"  onload="log('parser script404 onload')" onerror="log('parser script404 onerror')"></script>
<script>
  const st = document.createElement('style');  st.textContent = '#a{margin:2px}';
  st.onload = () => log('dynamic style onload');   document.head.appendChild(st);
  const s  = document.createElement('script'); s.src = 'ext2.js';
  s.onload  = () => log('dynamic script onload');  document.head.appendChild(s);
  const l  = document.createElement('link');   l.rel = 'stylesheet'; l.href = 'ext.css';
  l.onload  = () => log('dynamic link onload');    document.head.appendChild(l);
</script>
```

Вывод браузера — ровно два события из девяти:

```
Загружен скрипт: http://127.0.0.1:18777/ext.js
Пропуск скрипта  http://127.0.0.1:18777/missing-404.js: network error: HTTP 404
[JS] PROBE: external script executed          ← парсерный скрипт ВЫПОЛНИЛСЯ
[JS] PROBE: sync end
Пропуск CSS      http://127.0.0.1:18777/missing.css: network error: HTTP 404
[JS] PROBE: dynamic external executed
[JS] PROBE: dynamic link onload               ← createElement-путь работает
[JS] PROBE: dynamic script onload             ← createElement-путь работает
```

То есть:

| вставка | `<script src>` | `<link rel=stylesheet>` | `<style>` |
|---|---|---|---|
| парсером (разметка страницы) | ресурс грузится и исполняется, **события нет** — ни `load`, ни `error` на 404 | лист грузится и попадает в каскад, **события нет** | лист применяется, **события нет** |
| `createElement` + вставка | `load` есть (BUG-571) | `load` есть (BUG-722) | **события нет** |

Ресурс при этом реально загружается и применяется — молчит именно
уведомление страницы, поэтому дефект невидим для всего, кроме кода,
который на событие подписан.

## Причина (локализована чтением кода)

Машинерия событий `load`/`error` (`_lumen_resource_pending`,
`_lumen_resource_try_prepare`, `_lumen_resource_fire`) была написана
[BUG-571](bugs/BUG-571-FIXED.md) под `<script>` и обобщена
[BUG-722](bugs/BUG-722-FIXED.md) на `<link rel=stylesheet>`. Вход в неё
ровно один — `_lumen_resource_track(nid, tag)`, который вызывается ТОЛЬКО
из `document.createElement`/`createElementNS` и имеет жёсткий белый список
тегов:

```js
function _lumen_resource_track(nid, local) {
    var tag = String(local).toLowerCase();
    if (tag !== 'script' && tag !== 'link') return;   // ← 'style' не проходит
    ...
}
```

Отсюда обе половины дефекта:

1. **`<style>` не проходит белый список** — ни из парсера, ни из
   `createElement`. `style.onload` не сработает никогда, даже на пути,
   который для `<script>`/`<link>` уже работает. Это самая дешёвая часть
   фикса: у инлайнового `<style>` нет сетевой загрузки, задача сводится к
   «после того как лист разобран и попал в каскад — задачным хопом
   выстрелить `load`» (а для `@import`, который не разрешился, — `error`,
   как требует `style-error-01.html`).

2. **Парсерный путь не подключён к машинерии вообще.** Комментарий над
   `_lumen_resource_pending` объявляет это намеренным, ссылаясь на
   спецификационный флаг *already started*: «Deliberately NOT covered …
   scripts that came from the document parser». Флаг корректен по своему
   назначению — он запрещает ПОВТОРНЫЙ запуск скрипта при перемещении
   элемента по дереву. Но HTML LS §4.12.1 (\"execute the script block\",
   шаги с `fire an event named load at el` / `named error`) требует
   выстрелить событие и для парсерного скрипта тоже, а §4.6.7 — для
   парсерного `<link>`. Шелл эти ресурсы грузит своим путём (строки
   `Загружен скрипт:` / `Загружен CSS:` / `Пропуск скрипта…` /
   `Пропуск CSS…` в `crates/shell/src/main.rs`) и не даёт JS-стороне
   никакого сигнала об исходе.

## Почему это TIMEOUT, а не FAIL

Тот же класс, что [BUG-622](bugs/BUG-622-OPEN.md), [BUG-795](bugs/BUG-795-OPEN.md)
и хелпер-404 из среза 7: тест регистрирует `async_test`/`promise_test`,
который резолвится только из обработчика события. Событие не приходит,
`harness_status` не публикуется, wptrunner убивает страницу по таймауту.
Каждый такой тест стоит ~9 с настенного времени против 0.05 с у
разрешившегося (WPT-RUN-5, срез 15).

## Охват

- **Остаток `unclassified` снимка WPT-RUN-5:** 27 id ловятся строгим
  маркером (событийный атрибут на `<style>`/`<link>`/`<script>` в разметке,
  либо `createElement('style')` с ожиданием `load`/`error` НА ТОЙ ЖЕ
  переменной). Крупнейшие: `html/semantics` 15,
  `content-security-policy/style-src` 3.
- **По корпусу:** 156 файлов-источников — `<script … onload/onerror=>` 90,
  `<link … onload/onerror=>` 47, `<style … onload/onerror=>` 12,
  `createElement('style')` с ожиданием события 9.

Числа — нижняя граница: маркер требует, чтобы ожидание было привязано к
той же переменной (иначе `check-layout-th.js`, создающий `<style>` для
подсветки ошибок и не ждущий его, забирал бы себе 40 тестов `css-grid`),
и не видит форм вида `document.querySelector('style').addEventListener('load', …)`.

## Не путать

- [BUG-571](bugs/BUG-571-FIXED.md) / [BUG-722](bugs/BUG-722-FIXED.md) —
  `createElement`-путь для `<script>`/`<link>`; **работает**, подтверждено
  A/B выше. Чинить их заново не нужно.
- [BUG-630](bugs/BUG-630-OPEN.md) (`<img>`), [BUG-795](bugs/BUG-795-OPEN.md)
  (`<track>`), [BUG-798](bugs/BUG-798-OPEN.md) (`<embed>`/`<object>`) — тот
  же КЛАСС («элемент не сообщает об исходе загрузки»), но другие элементы и
  другой код; общего фикса с ними нет.
- [BUG-459](bugs/BUG-459-OPEN.md) — URL внешнего `<script type=module>`;
  ортогонально, событий не касается.
