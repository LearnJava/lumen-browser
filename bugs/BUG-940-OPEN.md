# BUG-940 — у `<audio>` нет `currentSrc`: `<video>` получил его починкой BUG-825, `audio_element.rs` — отдельный шим и не унаследовал ничего

**Статус:** OPEN
**Тип:** дефект реализованного кода — один член IDL, у соседнего элемента реализованный целиком.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 30 — реплей `currentSrc.html` под настоящим `testharness.js`)
**Область:** js (`crates/js/src/audio_element.rs` — `patchAudioElement`; ср. `crates/js/src/video_bindings.rs:935`/`:1069`/`:1160`, где `_currentSrc` заведён, обновляется алгоритмом выбора ресурса и опубликован геттером)
**Владелец:** P3.

## Симптом

`audio.currentSrc` — `undefined` на любом `<audio>`: до присвоения `src`,
после присвоения пустой строки и после присвоения настоящего URL. По HTML LS
§4.8.11 это `DOMString`, изначально пустая строка, а не `undefined`.

У `<video>` тот же член работает полностью — `video_bindings.rs` ведёт
`_currentSrc` через `NETWORK_NO_SOURCE`/`NETWORK_LOADING` и публикует его
геттером — потому что [BUG-825](BUG-825-FIXED.md) чинила именно `<video>`.

## Прямое измерение

Реплей настоящего теста под настоящим `testharness.js`
(`tests/wpt/verify_replaced_content_gaps.py --variant replay-media-currentsrc`,
2026-09-01, dev-release, Linux, `main` = `287562e61`):

```
replay-test [1] audio.currentSrc initial value ::
    assert_equals: expected (string) "" but got (undefined) undefined
replay-test [1] audio.currentSrc after setting src attribute "" ::
    assert_equals: expected (string) "" but got (undefined) undefined
```

`[1]` — FAIL по классификации `testharness.js`, то есть страница жива и
харнесс досчитал: это не зависание, а неверный ответ.

## Кого это держит

`html/semantics/embedded-content/media-elements/location-of-the-media-resource/currentSrc.html`
(1 id остатка WPT-RUN-5) — файл проверяет `currentSrc` у обоих элементов, и
`<audio>`-половина его роняет.

## Направление починки

Ровно то, что уже написано в `video_bindings.rs`: завести `_currentSrc` в
`patchAudioElement`, обновлять из того же места, где считается абсолютный URL
(`startLoad`, см. [BUG-924](BUG-924-OPEN.md) — там же живёт нерезолвленный
относительный путь), опубликовать геттером с начальным `''`.

Общая форма, из-за которой это и разъехалось, уже записана в `CLAUDE.md`:
пофичный шим вне `WEB_API_SHIM*` — это свой `rt.eval`, до которого правка
страничного шима не доходит. Перед «починили везде» — грепать соседние шимы.
