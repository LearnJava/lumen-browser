# BUG-1120 — `<script defer>` выполняется в порядке документа, а не после окончания разбора

**Статус:** FIXED 2026-09-25 (P6)
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

## Исправление (2026-09-25, P6)

`collect_scripts_ordered` ([scripts.rs](../crates/shell/src/scripts.rs)) раскладывает скрипты на два
списка: парсер-блокирующие классические и «исполняемые после окончания разбора» — все `type=module`
и внешние HTML-классические `defer` без `async`, вперемешку в порядке документа.
`run_scripts_with_dom` исполняет второй список после классического цикла и хвоста парсерных
вставок, перед `DOMContentLoaded`; классический элемент идёт через общий
`run_parser_classic_script` (CSP, `currentScript`, `load`/`error`). Вид элемента (модуль или
классический) снимается по разметке до исполнения — скрипт выше может переписать `type`.
Загрузка не менялась: тела и так скачиваются заранее (`resolve_script_sources`), сдвинуто
только исполнение. Не моделируется: классический `async` (по-прежнему в порядке документа);
`defer` на инлайне игнорируется, как требует спецификация.

Тесты: `collect_scripts_ordered_puts_external_defer_into_deferred_list`
(`crates/shell/src/tests/scripts_and_frames.rs`),
`external_defer_script_runs_after_parsing_before_dom_content_loaded`
(`crates/shell/src/tests/page_pipeline.rs`, оба репро заявки через полный конвейер).

## Перемер (видимое окно `--maximized`, без блокировщика, Chrome 153)

| Страница | До (main bae81ab14) | После | Chrome |
|---|---|---|---|
| `g3/defer.html` | `defer:undefined` первым | `inline-end-of-body, defer:object, DCL` | то же |
| `g6/site/deferorder.html` | `inline-head, defer:undefined, sync-ext, inline-tail` | `inline-head, sync-ext, inline-tail, defer:object` | то же |
| khanacademy | `__KA_DATA__ not found!`, 31 узел, текста 8 | ошибки нет, 525 узлов, текста 1422 | 776 / 4995 |
| coursera | `coursera is not defined`, 1887 узлов, текста 3700 | ошибки нет, 2409 узлов, текст совпадает с Chrome (en-US) | 2388 |

Остаток расхождения на обоих сайтах — не порядок скриптов, у каждого своя причина:

- **khanacademy** показывает баннер «Unsupported browser»: сервер ставит
  `KA-is-unsupported-browser: true` по UA. Lumen шлёт `Chrome/130.0.0.0` (Chrome-профиль);
  Chrome 153 с UA `Chrome/130` получает тот же `true`, с `Chrome/140` — `false`, с `Lumen/0.5.0` —
  `false`. Это [BUG-1113](BUG-1113-OPEN.md). Кроме того, все `@font-face` из
  `cdn.kastatic.org/khanacademy/*.css` запрашиваются от базы документа (`www.khanacademy.org/fonts/…`
  → 403 вместо `cdn.kastatic.org/khanacademy/fonts/…` → 200) — [BUG-1127](BUG-1127-OPEN.md).
- **coursera** выше Chrome (9115 против 5922 px): у карусельных колонок
  `max-width: 20%; flex-basis: 20%` ширина 40 px вместо 200 — процентный `max-width`
  flex-элемента резолвится от его же главного размера. Корень общий с
  [BUG-974](BUG-974-OPEN.md), дописан туда.
