# BUG-799 — `<audio>`-тесты с реальной сетевой загрузкой виснут TIMEOUT так, что даже собственный `t.step_timeout()` теста не срабатывает

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 6 — `html/semantics/embedded-content/the-audio-element`)
**Область:** предположительно `crates/js/src/audio_element.rs` (`startLoad`/`__lumen_audio_load`-цикл) и/или `crates/shell/src/platform/audio_player.rs` (`fetch_audio_bytes`, сетевой поток) — **не локализовано до конкретной строки**, см. ниже
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Дисклеймер по уверенности

Вывод построен на данных прогона (`tests/wpt/run_report.py`) плюс чтении
кода — **не** на живом зонде: сегодняшняя попытка (`--mcp-live-port`)
воспроизводимо отвечала `Eval error: JS context not available` даже на
пустой странице (`<body><div id="out">hi</div></body>`) на трёх
независимых свежих процессах подряд, то есть похоже на отдельную,
сегодняшнюю поломку live-окна в этом окружении, а не на что-то
специфичное для audio — не расследовано в рамках этого среза. Прежде чем
чинить БАГ ниже, стоит подтвердить механизм живым зондом на рабочем
окружении.

## Симптом

`tests/wpt/run_report.py --all --root
html/semantics/embedded-content/the-audio-element --recursive`: **13 из 16
TIMEOUT** (harness OK только 3/16). Все 13 — тесты вида
`audio-loading-*.html`/`audio_constructor.html`, использующие реальный
`src` (например `/media/sine440.mp3?pipe=trickle(d2)`) и ждущие событие
`loadeddata`/аналог.

Показательная деталь — `audio-loading-eager.html`:

```js
async_test(t => {
  const audio = document.createElement("audio");
  audio.src = "/media/sine440.mp3?pipe=trickle(d2)";
  audio.addEventListener("loadeddata", () => { t.done(); });
  document.body.appendChild(audio);
  t.step_timeout(() => {
    assert_unreached("Eager audio should load data immediately");
  }, 5000);   // <-- тест сам себя обязан провалить FAIL через 5с
}, "...");
```

В отчёте (`.tmp/wpt-embedded-the-audio-element.html`) эта запись —
`TIMEOUT`, `subcount 0/0`, `dur 25.02s`. **`0/0` означает, что за все 25 с
не было зарегистрировано ни одного `assert_*`** — то есть тестовый
`step_timeout(…, 5000)` (обычный `setTimeout` из `testharness.js`) сам не
сработал ни разу за 5×-кратный запас времени. Если бы он сработал,
результат был бы `FAIL` (от `assert_unreached`) с `subcount 0/1`, не
`TIMEOUT` с `0/0`. Это отличает находку от «просто событие `loadeddata`
не пришло» — здесь не сработал вообще никакой JS-таймер на странице,
пока идёт сетевая загрузка audio.

Контрольная точка — `audio-loading-lazy-source-deferred.html` (единственный
audio-файл со статусом OK в этой категории, subcount 0/1 FAIL): его
единственный зарегистрированный `assert_*` упал с сообщением
`HTMLMediaElement is not defined` — это отдельный, уже задокументированный
инлайн-комментарием пробел (`dom.rs:13817-13818`: «Lumen has no
HTMLMediaElement interface yet»), не новая находка и не причина TIMEOUT
остальных 13 (тот файл НЕ висит — сразу FAIL).

## Не относится к `<video>`

`the-video-element`-категория тоже даёт TIMEOUT-ы
(video-loading-lazy-*.html, video_crash_empty_src.html и др.), но это —
**уже известный, задокументированный** пробел: `crates/js/src/video_bindings.rs`
прямо объявляет себя «Phase 1 (animated GIF playback)» — `<video>` в
Lumen умеет проигрывать только GIF через отдельный `__lumen_video_*`
API, реальные `.mp4`/`.webm` он не декодирует вовсе. Тесты `the-video-element`
используют настоящие видео-файлы, поэтому TIMEOUT там объясняется этим
уже известным Phase-1 ограничением, а не новым багом — отдельно не
заводится.

`<audio>`, в отличие от `<video>`, заявлен как «real audio playback via
`AudioPlaybackProvider`» (`audio_element.rs:1`) — то есть по документации
ДОЛЖЕН реально грузить и проигрывать сетевой источник, что и отличает эту
находку от video: здесь ожидаемо работающий путь ведёт себя как будто
JS-таймеры на странице заморожены.

## Что проверено кодом (не живьём)

`__lumen_audio_load` не блокирует JS/engine-поток напрямую — сетевой fetch
уходит в отдельный поток (`crates/shell/src/platform/audio_player.rs:307-341`,
`thread::Builder::new().name(format!("lumen-audio-fetch-{handle}"))`), то
есть простое «синхронный fetch на engine-потоке» не объясняет находку
и гипотезу стоит проверять дальше именно живым зондом, а не чтением кода.

## Масштаб

13 файлов TIMEOUT-ов только в `the-audio-element`; при широком
использовании `<audio>`/подобной идиомы `setTimeout`-внутри-теста-с-audio
в других категориях (не подсчитано) возможен более широкий эффект — вне
скоупа этого среза.

## Направление расследования (не предписание)

1. Живой зонд (после починки/перезапуска окружения под MCP): страница с
   `<audio src="http://…/большой_или_trickle_ответ">` — считает ли
   `setInterval` тик до истечения времени, и отдельно чистый
   `setTimeout(fn, 1000)` на той же странице без audio вообще (контроль —
   тормозят ли таймеры именно из-за audio-элемента или это общая
   поломка).
2. Если таймеры на audio-странице действительно не срабатывают —
   проверить, не блокирует ли что-то у `wptserve`'s `?pipe=trickle(d2)`
   (частичная/растянутая по времени отдача) конкретно наш HTTP-клиент
   (`lumen_network::HttpClient::fetch_subresource`) так, что поток
   `lumen-audio-fetch-*` не освобождает что-то общее (мьютекс/канал),
   которым течёт event loop.

## Как проверить фикс

1. Живой зонд: `step_timeout`-подобный `setTimeout` на странице с
   загружающимся `<audio src="реальный http URL">` срабатывает вовремя.
2. WPT: `the-audio-element/audio-loading-*.html` — TIMEOUT уходит к нулю
   (событие `loadeddata` либо приходит, либо тест честно проваливается
   FAIL/`assert_unreached` в течение заявленных 5 с, а не 25).
