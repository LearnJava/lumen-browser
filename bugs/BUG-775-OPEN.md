# BUG-775 — `HTMLTrackElement` никогда не диспатчит `load`/`error`: любой скрипт, ждущий готовности `<track>`, зависает навсегда

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:13589` — `_lumen_install_reflection(HTMLTrackElement.prototype, ...)` устанавливает только атрибуты-рефлексии; `crates/js/src/video_bindings.rs` — `fireEvent()` вызывается для `<video>`/`<audio>` (`play`/`playing`/`seeking`/…), ни разу для `<track>`; `crates/shell/src/tracks.rs::load_video_tracks` — источник cue-данных, чисто внутренний, ничего не диспатчит в DOM)
**Найден:** P2, WPT-VENDOR-webvtt, 2026-08-18 — `run_report.py --all --root webvtt --recursive`

## Симптом

Любой скрипт, который создаёт `<track>`, вешает `onload`/`onerror` и ждёт
одно из них перед проверкой `video.textTracks[0].cues` — стандартный
паттерн всех тестов `webvtt/parsing/file-parsing/tests/*.html` (генерируются
по `webvtt/parsing/file-parsing/README.md`) — зависает до внешнего таймаута
`wptrunner` (~10 с) и падает `TEST_END: Test TIMEOUT`, 0 сабтестов пройдено.
Прогон категории `webvtt`: **21/72 harness OK, 1/176 сабтестов**, все 40
файлов `parsing/file-parsing/tests/` и часть `parsing/cue-text-parsing/tests/`
(например `timestamps.html`) — в TIMEOUT по этой причине.

Пример (`header-garbage.html`):

```js
var track = document.createElement('track');
track.src = 'support/header-garbage.vtt';
track.onload = this.step_func(trackLoaded);
track.onerror = this.step_func(trackError);
video.appendChild(track);
document.body.appendChild(video);
// ни trackLoaded, ни trackError не вызываются никогда
```

## Причина

`grep -rn "HTMLTrackElement" crates/js/src/dom.rs` показывает единственное
использование — установку прототипа (`_lumen_html_tag_prototypes['TRACK']`)
и рефлексию пяти атрибутов (`kind`/`src`/`srclang`/`label`/`default`,
`dom.rs:13589-13595`). Ни `readyState`, ни диспатч `load`/`error` не
устанавливаются вовсе.

`fireEvent(el, ...)` (синтетический DOM-событийный мост для `<video>`/
`<audio>`, `video_bindings.rs`) вызывается для `play`/`playing`/`pause`/
`ended`/`seeking`/`seeked`/`timeupdate` — ни разу для `track`/`load`/`error`
на `<track>`-элементе.

Реальная загрузка `.vtt` (`crates/shell/src/tracks.rs::load_video_tracks`)
— чисто внутренний Rust-слой: обходит `Document`, фетчит `src` выбранного
трека, парсит cues и кладёт результат в `TextTrackStore` для рендер-оверлея
и для `video.textTracks` (`video_bindings.rs::buildTextTracks`, читает
снэпшот лениво при первом обращении к геттеру). Ничего в этой цепочке не
знает о JS-объекте `<track>` и не вызывает `dispatchEvent`/`fireEvent` на
нём — цепочка «шелл распарсил VTT» и цепочка «страница слушает
`track.onload`» никак не связаны.

## Почему это не только тестовый шум

Спека (HTML §4.8.11 `HTMLTrackElement`) требует диспатча `load` при
успешной загрузке+парсинге и `error` при сетевой ошибке/невалидном VTT —
это единственный способ для страницы узнать, что cues готовы, если она не
хочет поллить `TextTrack.cues` вручную. Любой реальный плеер с
кастомными субтитрами (а не встроенным UA-контролом), использующий
стандартный `track.onload`-паттерн — а не только тесты — зависает
навсегда на Lumen. `TextTrack`'s собственное событие `cuechange` тоже не
проверялось этой сессией — вероятно та же дыра (нет отдельного бага;
`buildTextTracks` — не расследовано, videо_bindings.rs `checkCueChanges`
что-то диспатчит для `<video>`, для `TextTrack` самого — не проверено).

## Предлагаемый фикс

После того как `crates/shell/src/tracks.rs::load_video_tracks` (или его
JS-видимый эквивалент) успешно распарсил/не смог распарсить `src`
конкретного `<track>`, продиспатчить синтетический `load`/`error` Event на
соответствующем JS-объекте `<track>` — тем же механизмом `fireEvent`, что
уже применяется к `<video>`/`<audio>`. Требует, чтобы момент завершения
загрузки/парсинга VTT был видим JS-стороне как дискретное событие, а не
только как ленивый геттер-снэпшот (`buildTextTracks`) — сейчас в системе
нет "готово" сигнала вообще, только текущее состояние по запросу.

## Не расследовано в этой сессии

- `TextTrack.oncuechange`/`addEventListener('cuechange', ...)` — тот же
  класс гэпа или отдельный, не проверено.
- `api/` директория (VTTCue/VTTRegion конструкторы) падает отдельно и уже
  покрыта [BUG-570](BUG-570-OPEN.md) (`VTTCue`/`TextTrackCue`/`TrackEvent`
  не установлены как глобалы) — не путать с этим багом: там нет самого
  конструктора, здесь есть конструктор трека, но нет событий готовности.
- `rendering/` (581 из 820 файлов категории) — reftest-ы, `run_report.py`
  их не исполняет (`Unsupported test type reftest`), визуальная корректность
  рендера cue не проверена этой сессией.
