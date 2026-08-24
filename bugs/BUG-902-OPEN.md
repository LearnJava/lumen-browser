# BUG-902 — cue отдаёт странице только текст и тайминги: нет `getCueAsHTML()`, нет ни одной настройки cue, нет `VTTRegion`

**Статус:** OPEN
**Заведён:** 2026-08-24 (P1, при закрытии [BUG-775](BUG-775-FIXED.md))
**Область:** `crates/js/src/video_bindings.rs` — `appendCues()` строит объект cue из пяти полей; `crates/js/src/text_track_store.rs::tracks_json` и натив `__lumen_vtt_parse` — оба сериализуют `VttCue` без `settings`; `crates/engine/dom/src/vtt.rs` — `VttCueSettings` разбирается и никуда не уезжает
**Владелец:** P1/P3

## Симптом

До BUG-775 весь каталог `webvtt/parsing/cue-text-parsing/` висел в TIMEOUT и
ничего о себе не сообщал. Теперь он выполняется за доли секунды и печатает
свою настоящую причину — три независимых пробела в том, что страница может
**прочитать** у cue:

1. **`VTTCue.getCueAsHTML()` не существует.** `cue.getCueAsHTML is not a
   function` — **92 сабтеста** каталога `cue-text-parsing` (`tags`, `entities`,
   `tree-building`, `timestamps`, `text`). Метод обязан вернуть
   `DocumentFragment` по правилам разбора текста cue (WebVTT §6.4): текст
   разбирается уже сейчас, но только как плоская строка (`strip_cue_markup` в
   `crates/shell/src/tracks.rs` выбрасывает разметку для оверлея), а
   дерева из неё никто не строит.
2. **Ни одной настройки cue у объекта нет.** `cue.align`/`line`/`position`/
   `size`/`region`/`vertical`/`lineAlign`/`positionAlign` — `undefined`
   (`assert_equals: Failed with cue 0 expected (string) "center" but got
   (undefined) undefined`). Это не «не разобрано»: `lumen_dom::vtt` разбирает
   `VttCueSettings` целиком и шелл рисует по ним оверлей — потерян ровно один
   шаг, сериализация. И `tracks_json` (Rust-обход), и `__lumen_vtt_parse`
   (JS-путь) отдают одинаковые пять полей `{id,start,end,text}` + `track`, и
   `appendCues` больше ничего и не может положить. **Дешевле всего чинится
   из всего этого файла.**
3. **`VTTRegion` отсутствует целиком** — ни конструктора, ни `cue.region`, ни
   `TextTrack.regions`/`addRegion`/`removeRegion`. `REGION`-блоки заголовка
   `parse_vtt` молча пропускает.

Смежное и уже заведённое: `VTTCue`/`TextTrackCue`/`TrackEvent` не установлены
как глобальные конструкторы — [BUG-570](BUG-570-OPEN.md). Этот баг про
**объект cue, который движок уже создал**, тот — про возможность создать свой.

## Масштаб (измерено)

`run_report.py --all --root webvtt --recursive` на фиксе BUG-775
(2026-08-24, 0 мин 58 с, 88/322 harness OK, 31/178 сабтестов):

| механизм | уникальных FAIL-сабтестов |
|---|---|
| `getCueAsHTML is not a function` | 92 |
| настройки cue / regions не отдаются | 33 |

Оба множества раньше были невидимы — они прятались за TIMEOUT.

## Как проверить фикс

`webvtt/parsing/cue-text-parsing/tests/tags.html` (сейчас 28 FAIL, все на
`getCueAsHTML`), `webvtt/parsing/file-parsing/tests/settings-align.html`
(`cue.align === 'center'`), `webvtt/api/VTTCue/*`.
