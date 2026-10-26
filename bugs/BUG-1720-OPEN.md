# BUG-1720 — `<video>` внутри `<iframe>` не рисуется (область фрейма — пустая серая коробка)

**Статус:** OPEN
**Заведён:** 2026-10-26 (по живой проверке, main 677ee5487, Windows 10, dev-release)
**Область:** shell/engine (вложенный browsing context: `crates/shell/src/video_tick.rs`, `frame_bridge`)

## Симптом

`LUMEN_NO_ADBLOCK=1 target/dev-release/lumen.exe --maximized https://www.w3schools.com/howto/howto_css_fullscreen_video.asp`. На странице `<iframe src="tryhow_css_fullscreen_video.htm" style="border:11px solid #f1f1f1;border-radius:5px;width:100%;height:700px">`. Документ фрейма содержит `<video autoplay muted loop id="myVideo"><source src="rain.mp4" type="video/mp4"></video>` со стилем `#myVideo{position:fixed;right:0;bottom:0;min-width:100%;min-height:100%}` и фиксированной текстовой полосой `.content` внизу. В логе видео ЗАГРУЖЕНО и декодировано внутри фрейма (`[media] видео H264: Media Foundation…`, `video: загружен nid=13 (1280×720, 6040мс)`), но область iframe рисуется пустой светло-серой коробкой: не видно ни видео, ни текстовой полосы `.content` фрейма.

## Контроль

Открытие URL фрейма как верхнего документа (`https://www.w3schools.com/howto/tryhow_css_fullscreen_video.htm`) рисует и видео, и текстовую полосу правильно. Значит, дефект специфичен для вложенного browsing context.

## Гипотезы (не проверены)

1. Текстура кадра регистрируется под ключом `format!("video:{nid}")` с node id из документа фрейма (`crates/shell/src/video_tick.rs`), а отрисовка/`self.layout_source` смотрит на верхний документ — nid не совпадают или пересекаются с nid верхнего документа.
2. `position: fixed` внутри iframe считается относительно окна верхнего уровня, а не вьюпорта фрейма — бокс уезжает за пределы клипа.
3. Содержимое фрейма вообще не попадает в display list (тогда пропал бы и `.content`, что и наблюдается) — проверить без `<video>` на фрейме с `position:fixed` текстом.

Начать с `grep -rn frame_bridge crates/` и пути отрисовки дочернего документа.

## Отдельное наблюдение (не расследовалось)

`https://www.w3schools.com/html/tryit.asp?filename=tryhtml5_video_autoplay_muted`: iframe результата остаётся пустым, в логе `[JS error] Error: Element with ID "iframecontainer" not found.` Возможно, связано (пустой iframe), возможно нет — отдельная причина в скрипте страницы.
