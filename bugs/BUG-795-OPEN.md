# BUG-795 — `<track>` never fires `load`/`error`, and `HTMLTrackElement.track` is entirely unimplemented

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 4 — разбор массового TIMEOUT в `html/semantics/embedded-content`)
**Область:** `crates/js/src/dom.rs` (`HTMLTrackElement.prototype` reflection block, `dom.rs:13800-13806` — no `track` accessor installed) и `crates/js/src/video_bindings.rs`/`crates/js/src/text_track_store.rs` (существующая `TextTrack`/`TextTrackList` машинерия, но выстроенная только вокруг `<video>.textTracks`, ни разу не привязанная к отдельному `<track>`-узлу)
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-инструментальной задачи, здесь не чинится.

## Симптом

Два независимых, но однокоренных пробела в `<track>` (HTML §4.8.11):

1. **`HTMLTrackElement` никогда не диспатчит `load`/`error`.** `grep` по
   `crates/js/src/video_bindings.rs` находит диспатч `loadedmetadata` для
   `<video>`/`<audio>`, но ни одного места, диспатчащего `load`/`error` на
   `<track>` — событие, которое спека требует слать после успешного/неудачного
   разбора WebVTT-ресурса. Любой код, ждущий `trackElement.onload`, висит
   вечно.
2. **`HTMLTrackElement.prototype.track` (readonly IDL-атрибут, должен отдавать
   связанный `TextTrack`) не установлен вовсе.** Единственный
   `_lumen_install_reflection(HTMLTrackElement.prototype, …)` в `dom.rs`
   рефлектит только контентные атрибуты (`kind`/`src`/`srclang`/`label`/
   `default`) — обращение к `trackElement.track` даёт `undefined`.
   `TextTrack`/`TextTrackList` уже существуют (`video_bindings.rs:277+`,
   `buildTextTracks(el, nid)`), но собираются только из `<video>.textTracks`
   по `nid` видео-элемента — ни разу не вызываются для одиночного `<track>`.

## Минимальное репро (прямые логи прогона, без testdriver)

`html/semantics/embedded-content/media-elements/track/track-element/cors/*.html`
(`cors/support/common.js::loadTrack`, синхронный вызов сразу после создания
узла):

```js
var video = document.createElement('video');
window.track = document.createElement('track');
...
video.appendChild(track);
document.body.appendChild(video);
track.track.mode = 'showing';   // <- здесь
```

Живой лог `run_report.py` (`.tmp/media_elements.log`, тест
`track/track-element/cors/011.html`):

```
FAIL track CORS: Anonymous, same-origin, no headers - Cannot set properties of undefined (setting 'mode')
TypeError: Cannot set properties of undefined (setting 'mode')
    at Test.loadTrack (<anonymous>:53:22)
```

Это подтверждает пункт 2 напрямую (`track.track` === `undefined`). Пункт 1
подтверждён не прямым логом, а чтением исходника: единственный потребитель —
`html/semantics/embedded-content/media-elements/track/track-element/track-helpers.js::check_cues_from_track`,
используемый **каждым** `track-webvtt-*.html` файлом, ждёт `trackElement.onload`
и никогда не получает управление — TEST_END для всех них TIMEOUT, без единой
FAIL/ERROR строки в логе (в отличие от cors/*, там до TIMEOUT успевает
проскочить синхронный TypeError).

## Масштаб (измерено, не оценено)

Свежий прогон категории на этой машине (`run_report.py --all --root
html/semantics/embedded-content/media-elements --recursive`, 308 тестов,
8 мин 17 с): **133/308 TIMEOUT (43.2 %)**, из них **75 — в
`track/track-element/`** (56 % всех TIMEOUT категории):

| подкаталог | TIMEOUT-файлов |
|---|---|
| `track/track-element/cors/*.html` | 32 |
| `track/track-element/track-webvtt-*.html` (плоские файлы) | 43 |

Оба множества замыкаются на один и тот же пробел — либо на `.track ===
undefined` (cors), либо на никогда не приходящий `load` (webvtt). Для родительской
задачи (`WPT-RUN-6`, категория `html/semantics/embedded-content`, 335 TIMEOUT
по снимку WPT-RUN-5) это даёт **~22 % (75/335)** объяснённых TIMEOUT одним
механизмом — самый крупный найденный вклад среди подкатегорий `embedded-content`
на данный момент (следующая по размеру — `loading-the-media-resource`, 7).

## Как проверить фикс

1. Живой прогон `cors/011.html` (или любого `cors/*.html`) — TypeError
   `Cannot set properties of undefined (setting 'mode')` исчезает.
2. `track-webvtt-valign.html` (или любой `track-webvtt-*.html`) — тест
   перестаёт быть TIMEOUT, `trackElement.onload` срабатывает.
3. Повторный `run_report.py --all --root
   html/semantics/embedded-content/media-elements/track/track-element
   --recursive` — TIMEOUT-счётчик уходит существенно ниже 75/86 (86 =
   `find track/track-element -name '*.html' | wc -l` минус `-manual.html`).
