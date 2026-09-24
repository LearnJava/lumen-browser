# BUG-1120 — `<script defer>` выполняется в порядке документа, а не после окончания разбора

**Статус:** OPEN
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** shell (`crates/shell/src/scripts.rs:58-64` `collect_scripts_ordered` — «defer/async are not modelled separately — the shell runs every script synchronously in document order»; цикл исполнения `scripts.rs:804`)

## Симптом

Бандлы обоих сайтов подключены `<script defer>` в `<head>`/середине документа и читают данные,
которые задаёт обычный инлайн-скрипт в конце `<body>`. В Lumen defer-скрипт исполняется раньше:

- khanacademy: `Uncaught Error: __KA_DATA__ not found!`, `#outer-wrapper` высотой 0, 31 узел против 751;
- coursera: `Uncaught ReferenceError: coursera is not defined` (модуль `Iaf0` из `en.app.*.js`),
  клиентский рендер не стартует, 1783 узла против 2670, текста 3564 против 10019 символов.

Причина прямо записана в `scripts.rs:58-64`: defer/async не моделируются, всё исполняется
синхронно в порядке документа. Тот же результат на 400-КБ документе и при медленной отдаче чанками.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g3/defer.html`:

```html
<!doctype html><html><head>
<script>window.LOG = [];</script>
<script defer src="/defer_a.js"></script>
</head><body>
<div>content</div>
<script>LOG.push('inline-end-of-body'); window.DATA = {ok: true};</script>
<script>document.addEventListener('DOMContentLoaded', function () { LOG.push('DCL'); });</script>
</body></html>
```

`g3/defer_a.js`:

```js
LOG.push('defer:' + (typeof window.DATA));
```

Второй вариант (`g6/site/deferorder.html`): inline в head → `<script defer src=d.js>` → sync-внешний → inline в конце body.

**Результат:** Lumen: `['defer:undefined','inline-end-of-body','DCL']`; второй вариант `['inline-head','defer:undefined','sync-ext','inline-tail']`. Chrome: `['inline-end-of-body','defer:object','DCL']` и `['inline-head','sync-ext','inline-tail','defer:object']`.

## Что сделать

HTML LS §4.12.1.1 «prepare the script element», шаг 31: парсерный внешний `defer`-скрипт
(без `async`) идёт в список «scripts that will execute when the document has finished parsing» и
исполняется по порядку после окончания разбора, перед `DOMContentLoaded` (§13.2.7 «the end»,
шаг 5). `type=module` без `async` — туда же. Загрузку начинать сразу, откладывать только исполнение.
Критерий: оба репро дают порядок Chrome; khanacademy и coursera перемерить.
