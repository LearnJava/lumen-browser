# BUG-1610 — EME: `navigator.requestMediaKeySystemAccess` не определён (696 TypeError в `encrypted-media`)

**Статус:** OPEN
**Заведён:** 2026-10-11 (P2, WPT-RUN-7 срез 68, baseline `encrypted-media`)
**Область:** js (Encrypted Media Extensions — `navigator.requestMediaKeySystemAccess`, `MediaKeys`, `MediaKeySession`, `MediaSource`)

## Симптом

Почти все тесты категории начинаются с `navigator.requestMediaKeySystemAccess(...)`. Вызова нет, подтест падает с `TypeError: navigator.requestMediaKeySystemAccess is not a function`. Рядом `ReferenceError: MediaSource is not defined` (6 вхождений: тесты воспроизведения через MSE).

## Охват

Прогон `--update-expected --all --root encrypted-media --recursive`: 696 вхождений `requestMediaKeySystemAccess is not a function` в логе, 69/99 harness OK, 29/256 подтестов. Подтесты, которые проходят, — те, что не доходят до EME-вызова.

## Как проверить

`python tests/wpt/run_report.py --all --root encrypted-media --recursive --processes 8` и поискать `requestMediaKeySystemAccess` в логе. Или `typeof navigator.requestMediaKeySystemAccess` в консоли: ожидается `function`, сейчас `undefined`.

## Замечание

Это недостающая функциональность, а не дефект реализованного кода. Решение о том, нужен ли браузеру EME (включая ClearKey без CDM), — продуктовое. Тесты `drm-*` без Widevine всё равно не пройдут: достаточно ClearKey-реализации для `clearkey-*`.
