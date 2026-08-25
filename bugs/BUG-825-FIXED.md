# BUG-825 — `<video>` — заглушка тоньше `<audio>`: нет `volumechange`, `networkState`/`currentSrc` не существуют, алгоритм выбора ресурса не запускается, а `loadedmetadata` приходит до всякого `src`

**Статус:** FIXED 2026-08-25
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 19 — побочная находка, маркера не получил, см. «Масштаб»)
**Область:** `crates/js/src/video_bindings.rs` — шим `<video>` (у `<audio>` свой, в `crates/js/src/audio_element.rs`), `crates/js/src/dom.rs` — хук вставки `<source>`
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, починен P1 2026-08-25.

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
[BUG-799](BUG-799-FIXED.md).

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
[BUG-799](BUG-799-FIXED.md): страница доживает до строки после присваивания и
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

## Срез 24 WPT-RUN-6 (2026-08-22) — сторона `<source>`, `load()` и `playbackRate`; маркер `media-resource-selection`

Замер `tests/wpt/verify_frame_load_media_gaps.py --variant
media-source-candidate --variant media-load-method --variant
media-rate-volume` (dev-release, Linux, коммит `c583a90b4`, `--seconds 5`,
страница жива — 9 тиков) добавляет к записи три факта:

1. **Кандидат-`<source>` не запрашивается.** `<video><source src=…></video>`:
   сервер пробы не получает запроса, `loadstart` не приходит, `error` не
   приходит ни на `<source>`, ни на элементе, `currentSrc` и `networkState`
   — `undefined`, `readyState === 4` до всякого источника. Тесты
   `loading-the-media-resource/resource-selection-candidate-*` манипулируют
   именно `<source>`, а не атрибутом `src`.
2. **`load()` не делает ничего.** Ни `emptied`, ни `abort`, ни `error`,
   ни повторного `loadstart`; `v.error` остаётся `undefined`.
   `load-removes-queued-error-event.html` ждёт ровно этой последовательности.
3. **`playbackRate` не существует как свойство.** `v.playbackRate` и
   `v.defaultPlaybackRate` — `undefined`; присваивание создаёт обычное поле и
   `ratechange` не диспатчится. `volume`/`muted` при этом настоящие, но
   `volumechange` не приходит и на них (это уже было записано выше для
   `<video>`; здесь подтверждено ещё раз).

Маркер `media-resource-selection` в `tests/wpt/timeout_audit.py` — **7 id**
остатка снимка WPT-RUN-5 (`event_volumechange.html`, `playbackRate.html`,
четыре `resource-selection-*`, `load-removes-queued-error-event.html`).

## Починено (P1, 2026-08-25)

`crates/js/src/video_bindings.rs` — `<video>` получил машину состояний
HTML §4.8.11 целиком, `crates/js/src/dom.rs` — хук вставки `<source>`.

**Что оказалось корнем.** Диагноз «не локализована точнее файла» подтвердился
буквально: у `<video>` не было *состояния загрузки вообще*. `readyState`
считался выражением `_gifBacked ? 4 : (_src ? 0 : 4)`, то есть «4, пока не
назначили src» — отсюда и `HAVE_ENOUGH_DATA` на пустом элементе; `src`-сеттер
для не-GIF синхронно диспатчил выдуманную пару `loadedmetadata`+`canplay` за
файл, который никто не запрашивал; `load()` перезапускал только GIF-ветку;
`volume`/`muted` были парой замыканий без событий, `playbackRate` не было.

**Что сделано.**

* **Алгоритмы §4.8.11.5** — «media load algorithm» (`load()`, присваивание
  `src`, вставка `<source>`) и «resource selection algorithm» с обеими ветками.
  Ветка атрибута заканчивается «dedicated media source failure steps»
  (`error` = `MediaError(MEDIA_ERR_SRC_NOT_SUPPORTED)`, `networkState` =
  `NETWORK_NO_SOURCE`, событие `error` на самом элементе); ветка `<source>`
  перебирает кандидатов, отсеивая по `type`/`media`, и стреляет `error`
  **на `<source>`, а не на медиа-элементе** — это ровно то различие, на
  котором стоят `resource-selection-candidate-*`. `load()` вправду
  перезапускает алгоритм: `abort` (если шла загрузка) → `emptied` → заново.
* **События ставятся в очередь задач, а не диспатчатся на месте.** Это не
  косметика, а условие работоспособности: `e.volume = 0.5; e.onvolumechange =
  …` — порядок `event_volumechange.html` и половины медиа-набора WPT — при
  синхронной доставке не видит ничего. Тот же урок, что измерил BUG-808 для
  `EventWatcher`: немедленное событие хуже отсутствующего.
* **`volumechange`/`ratechange`** — по спеке они привязаны к *изменению*
  значения, поэтому у сеттеров стоят проверки на равенство; `volume` вне
  диапазона бросает `IndexSizeError`, а не молча клампит.
* **`HTMLMediaElement` и `MediaError` заведены как интерфейсы.** Их не было
  вовсе (комментарий dom.rs: «Lumen has no HTMLMediaElement interface yet»),
  так что константам `NETWORK_*`/`HAVE_*` было негде жить, а
  `video instanceof HTMLMediaElement` бросал. Прототипы `<video>`/`<audio>`
  перевешены на новый через `Object.setPrototypeOf` — меняется только ссылка
  [[Prototype]], все строки рефлексии, уже установленные dom.rs, остаются
  собственными свойствами этих прототипов.
* **Побочно:** собственные аксессоры `controls`/`loop` удалены. Они держали
  значение в замыкании и **не писали контент-атрибут**, то есть
  `video.controls = true` был невидим для layout и paint; рефлексия dom.rs
  делает это правильно. `duration` без ресурса теперь `NaN`, а не `Infinity`
  (`Infinity` читался как «загружен бесконечный стрим»). Появились
  `currentSrc`, `seeking`, `buffered`/`seekable`/`played`, `fastSeek`.

**Сдвиги контракта, которые стоит знать.** Не-GIF ресурс больше не притворяется
загруженным: `play()` на нём реджектится `NotSupportedError` вместо тихого
`resolve`. Промис `play()` на элементе *без* ресурса, наоборот, резолвится
(спека оставила бы его висеть до начала воспроизведения) — висящий промис
уносит остаток файла `testharness.js` вместе с собой, это форма BUG-823.

**Замер (та же проба, что в «Как проверить фикс»):**

```
media-volumechange   video-defaults volume=1 muted=false readyState=0 networkState=0,
                     video-volumechange-listener, video-volumechange volume=0.5
media-resource-selection
                     video-src-set networkState=3 currentSrc=, video-load-called networkState=3,
                     video-emptied networkState=3, video-loadstart networkState=2,
                     video-error networkState=3
```

До фикса те же две пробы давали `video-defaults … readyState=4
networkState=undefined` без единого `video-volumechange`, и `video-loadedmetadata`
**до** присваивания `src` при полном молчании после него.

11 новых юнит-тестов (`video_bindings::tests_v8::media_element`), 12/12 дампов
`dump_golden.py` совпадают с эталоном (правка не двигает display-list).

## Остаётся вне рамок

* `<audio>` живёт по своей, более старой модели (`audio_element.rs`) и
  по-прежнему диспатчит события синхронно. Сводить их в один шим — отдельная
  работа: у аудио настоящий провайдер воспроизведения и другой цикл загрузки.
* Декодера, кроме GIF, нет, поэтому «выбор ресурса» физически не может
  закончиться успехом для mp4/webm — это Phase 1, а не этот баг.
* `<video poster>` по-прежнему не запрашивается ([BUG-848](BUG-848-OPEN.md)).
* `<source>`, добавленный **после** того, как элемент уже осел без ресурса
  (`networkState` = `NETWORK_NO_SOURCE`), не перезапускает выбор: хук слушает
  только `NETWORK_EMPTY`. Спека в этом месте продолжает приостановленный
  алгоритм, а не запускает новый, и перезапуск дал бы лишние `emptied`.
* Парсерно-написанный `<video>` патчится до первого скрипта страницы, поэтому
  его события видны; парсерный `<track>` — нет, это класс
  [BUG-804](BUG-804-OPEN.md).
