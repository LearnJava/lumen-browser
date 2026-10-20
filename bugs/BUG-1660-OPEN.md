# BUG-1660 — Самостоятельный видеоресурс (`video/mp4`, `video/webm`) открывается как текст

**Статус:** OPEN
**Заведён:** 2026-10-20 (P2, база SPR-w COMPAT-CORPUS-3)
**Область:** shell/net (документ верхнего уровня с `Content-Type: video/*`)

## Симптом

Сценарии `video-mp4-h264` и `video-webm-vp8` (`docs/perf/scenarios/media-mail-docs.toml`) открывают прямой адрес ролика
(`interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4`, `.webm`). Chrome показывает медиа-просмотрщик с `<video>`.
Lumen открывает документ как текстовый: «видимый текст» страницы — сырые байты контейнера (`ftypmp42 mp42mp41isomavc1 … moov`,
у webm — `EBML … webm`), элемента `<video>` нет, оракул `eval` (`document.querySelector('video')`) не выполняется.

Распознанный экран провала — `content` (кадр и начало текста — `.tmp/compat-scenarios/<прогон>/video-mp4-h264.png`, в `results.jsonl`).

## Причина (гипотеза, не проверена)

Для `video/*` и `audio/*` на верхнем уровне нет ветки «обернуть в минимальный документ с `<video src controls>`» — тело
отдаётся в текстовый разбор, как неизвестный тип.

## Как проверить

`python scripts/compat_scenarios.py --only video-mp4-h264,video-webm-vp8 --exe <lumen.exe>` — оба должны пройти; либо открыть адрес
в живом окне: должен быть проигрыватель, а не байты.
