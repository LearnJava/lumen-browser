# BUG-1749 — `cuechange` не приходит после загрузки `<track>`, но приходит ложный при пустом наборе cue

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-VTT-SCOUT)
**Область:** js (`crates/js/src/shim/video_element.js` — `checkCueChanges` стр. 366, `_activeSig` стр. 222, отложенный вызов в `patchVideoElement` стр. ~1602)

## Симптом

```html
<video src="video.webm"><track default src="cue.vtt"></video>
<script>
  video.textTracks[0].oncuechange = () => takeScreenshot();
</script>
```

`cuechange` с непустым `activeCues` не приходит никогда, и тест `reftest-wait` висит до лимита раннера.

`checkCueChanges` вызывается только (а) по тику воспроизведения / seek и (б) один раз через `setTimeout(0)` в `patchVideoElement` — ещё до загрузки `<track>`. В (б) `_activeSig` равен `null`, сигнатура пустого набора `''` от него отличается → срабатывает ложный `cuechange` при пустом `activeCues`. После `load` трека и после смены `mode` перепроверки нет, а webm/mp4 в рефтестах не играют (`networkState=3`, `currentSrc=''`), так что тика воспроизведения тоже нет.

## Как найдено

WPT-VTT-SCOUT (`docs/wpt-vendor-notes/webvtt.md`): медиана длительности рефтестов `webvtt/rendering/` — 5.3 с при 5-секундном лимите; `support/` хелпер `waitForActiveCueAndTakeScreenshot` ждёт `cuechange`.

## Что делать

1. Начальное `_activeSig` — сигнатура пустого набора (`''`), чтобы пустой набор не порождал события.
2. Перепроверять активные cue (`checkCueChanges`) после `load` трека, после смены `TextTrack.mode` и после `addCue`/`removeCue`, по алгоритму «time marches on» (HTML §4.8.11.8) с текущим `currentTime`, даже если видео не играет.

## Как проверить

Пробник выше в `lumen --screenshot` с `reftest-wait`: снимок делается сразу после загрузки трека, а не по таймауту; `run_corpus.py --prefixes webvtt/rendering --processes 6` — медиана длительности заметно ниже 5 с.
