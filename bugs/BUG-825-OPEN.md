# BUG-825 — `<video>` — заглушка тоньше `<audio>`: нет `volumechange`, `networkState`/`currentSrc` не существуют, алгоритм выбора ресурса не запускается, а `loadedmetadata` приходит до всякого `src`

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 19 — побочная находка, маркера не получил, см. «Масштаб»)
**Область:** `crates/js/src/dom.rs` — шим медиа-элементов (`<audio>` и `<video>` собираются разными путями), `crates/shell/src/video_bindings.rs`
**Владелец:** P1/P3 (`lumen-js` + шелл). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Один и тот же тест, прогнанный по обоим тегам, проходит на `audio` и виснет
на `video`:

```js
// html/semantics/embedded-content/media-elements/event_volumechange.html
function volumechange_test(tagName) {
  async_test(function (t) {
    var e = document.createElement(tagName);
    e.volume = 0.5;
    e.onvolumechange = t.step_func(function () { ... t.done(); });
  }, "setting " + tagName + ".volume fires volumechange");
}
volumechange_test("audio");   // проходит
volumechange_test("video");   // висит до таймаута враннера
```

Никакого ресурса здесь не участвует — элемент даже не в документе, — так
что это не про декодирование (Phase 1) и не про
[BUG-799](BUG-799-OPEN.md).

## Прямое измерение

`tests/wpt/verify_stream_scroll_message_gaps.py` (2026-08-21, коммит
`6e60c8aa8`, `--seconds 6`, страницы живы):

| проба | получено |
|---|---|
| `media-volumechange` | `audio-defaults volume=1 muted=false readyState=0 networkState=0`; `audio-volumechange volume=0.5` + вариант через `addEventListener` — оба есть. Для `video`: `video-defaults volume=1 muted=false readyState=4 networkState=undefined` и **ни одного** `video-volumechange` |
| `media-resource-selection` | `video-loadedmetadata` приходит **до** присваивания `src`; после `v.src = ".ssmgap-missing.mp4"` и `v.load()` — ни `loadstart`, ни `error`, ни `emptied`; `networkState=undefined`, `currentSrc=undefined` всё время |

Три расхождения читаются прямо из этих строк:

1. `<video>` не диспатчит `volumechange` ни в форме `onvolumechange`, ни
   через `addEventListener` (у `<audio>` работают обе);
2. у `<video>` нет `networkState` и `currentSrc` (у `<audio>`
   `networkState` есть и равен 0);
3. алгоритм выбора ресурса не запускается: присваивание `src` и вызов
   `load()` не дают ни одного события, а `readyState` равен `4`
   (`HAVE_ENOUGH_DATA`) с самого начала — то есть элемент рапортует
   «ресурс полностью загружен» до того, как ему что-либо назначили.

## Причина

Не локализована точнее файла: `<audio>` и `<video>` в шиме собираются
разными путями, и «аудио»-путь заметно полнее. Пункт (3) выглядит как
константный `readyState` без состояния загрузки вообще — тогда события
`loadstart`/`error`/`emptied`/`durationchange` просто некому ставить в
очередь. Уточнение причины — часть починки, а не этого отчёта.

## Масштаб

**Маркера в `timeout_audit.py` этот баг не получил** — правило по исходнику
не удалось сделать чистым: тесты категории перебирают теги переменной
(`volumechange_test("audio"); volumechange_test("video")`), а любое
достаточно широкое правило (`<video>` + любое немедийное событие)
затягивало 5 ложных id `html/infrastructure/urls/.../query-encoding`
(они упоминают `<video>` в общем хелпере) и 4 id `web-animations`
(совпадение по `playbackRate`). Заводить механизм с такой точностью
хуже, чем оставить id в остатке.

По прикидке — около **10 id** остатка снимка WPT-RUN-5 в
`html/semantics/embedded-content/media-elements` и `the-video-element`
(`event_volumechange.html`, `location-of-the-media-resource/currentSrc.html`,
`playing-the-media-resource/playbackRate.html`, четыре
`loading-the-media-resource/resource-selection-*`,
`video-loading-lazy-*`). Оценка снизу: категория в снимке в основном
отваливается раньше, на реальном декодировании.

## Перезамер 2026-08-22 (срез 22): запроса нет вовсе, доказано сервером

`tests/wpt/verify_perf_idb_sse_gaps.py --variant req-video-src` (dev-release,
Linux, коммит `bafa603d9`) поднимает http-сервер, который записывает каждый
запрошенный путь. После `video.setAttribute('src', …)` сервер **не видит
ничего**: ни запроса за медиафайлом, ни события `loadstart`/`error` на
странице, а `networkState` и `currentSrc` остаются `undefined` при
`readyState === 4`. Это отделяет «ресурс запрошен, но событие не пришло» от
«алгоритм выбора ресурса не запускался вовсе» — верно второе. Тот же замер
для `<audio>` (`--variant req-audio-src`) подтверждает
[BUG-799](BUG-799-OPEN.md): страница доживает до строки после присваивания и
дальше не идёт. Оба id (`fetch/metadata/generated/element-video`,
`element-audio`) с этого среза атрибутированы маркеру
`element-subresource-never-requested` ([BUG-848](BUG-848-OPEN.md)), чей ref
называет и этот баг.

## Направление починки (не предписание)

Свести `<video>` к тому же объекту состояния, что и `<audio>`: общий набор
`volume`/`muted`/`networkState`/`readyState`/`currentSrc` с постановкой
`volumechange` в очередь задач при изменении, и заготовка алгоритма выбора
ресурса, которая хотя бы честно даёт `loadstart` при назначении `src` и
`error` при неудачной загрузке. Настоящее декодирование (Phase 1) для
перечисленных тестов не нужно — им хватает корректных состояний и событий.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_stream_scroll_message_gaps.py
   --variant media-volumechange --variant media-resource-selection` —
   `video-volumechange` приходит, `networkState`/`currentSrc` определены,
   `readyState` стартует с 0, после `src` идёт `video-loadstart`, а для
   отсутствующего файла — `video-error`.
2. WPT: `run_report.py --all --root html/semantics/embedded-content/media-elements
   --recursive` — `event_volumechange.html` и `loading-the-media-resource/*`
   перестают висеть.
