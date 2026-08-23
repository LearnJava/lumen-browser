# BUG-840 — исключение из колбэка `PerformanceObserver` съедается шимом: `try { obs._cb(list, obs); } catch(e) {}` превращает FAIL теста в TIMEOUT

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, маркера намеренно нет)
**Область:** `crates/js/src/dom.rs:11539` (`_perf_deliver_to_observer`)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
addEventListener('error', e => console.log('window-error', e.message));
new PerformanceObserver(() => { throw new Error('boom'); }).observe({type: 'mark'});
performance.mark('m');
// колбэк входит, бросает — и об этом не узнаёт никто:
// ни window.onerror, ни stderr браузера
```

## Прямое измерение

`tests/wpt/verify_perf_idb_sse_gaps.py --variant po-callback-throw`
(2026-08-22, dev-release, Linux, коммит `bafa603d9`, `--seconds 6`, страница
жива — 11 тиков):

| ожидалось | получено |
|---|---|
| `po-cb-entered` + `window-error …psig-observer-boom` (или хотя бы `script error:` в stderr) | `po-cb-entered`, `po-after-throw alive` — и ничего больше |

## Причина (локализована чтением кода)

```js
// dom.rs:11539
function _perf_deliver_to_observer(obs, entries) {
    …
    try { obs._cb(list, obs); } catch(e) {}
}
```

Пустой `catch` — ровно та же форма, что уже описана в
[BUG-828](BUG-828-OPEN.md) (`oncomplete` у `OfflineAudioContext`): исключение
из колбэка страницы гасится хостовой обёрткой. Спека (Performance Timeline L2
§6.2.1, HTML LS «report an exception») требует сообщить об исключении в
`window.onerror`.

Практическое следствие для WPT: `testharness.js` узнаёт о провале
`step_func`-колбэка через событие `error`. Мост «исключение → `window.onerror`»
для таймеров, rAF и скриптов заведён 2026-08-22
([BUG-591](BUG-591-FIXED.md), частично), но сюда он не достаёт: исключение
гасится раньше, внутри самого шима. Поэтому любой провалившийся ассерт внутри
колбэка наблюдателя — не FAIL, а молчаливый TIMEOUT: тест просто перестаёт
подавать признаки жизни.

## Масштаб

Маркера в `timeout_audit.py` намеренно нет: правило по исходнику неотличимо
от «тест использует PerformanceObserver», а id таких тестов уже
атрибутированы более ранним причинам (`resource-timing-entry-never-delivered`,
BUG-809). Баг заведён по прямому замеру — как BUG-825 и BUG-829.

## Направление починки (не предписание)

Вместо пустого `catch` — пробросить исключение в тот же путь, которым шим
сообщает об ошибках обработчиков событий (`_lumen_report_exception`
/ будущий мост `TryCatch` → `window.onerror` из BUG-591). Тот же приём
закрывает и BUG-828.

## Как проверить фикс

`tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py --variant
po-callback-throw` — ожидается `window-error` (или `script error:` в stderr) с
текстом `psig-observer-boom`.
