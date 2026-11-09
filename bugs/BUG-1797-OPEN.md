# BUG-1797 — геттер `<audio>.src` шима замыкается на `_src` момента патча: `setAttribute` не доходит до отражения

**Статус:** OPEN
**Заведён:** 2026-11-09 (P3, попутно к [BUG-1775](BUG-1775-FIXED.md))
**Область:** js (`crates/js/src/audio_element.rs:346,509`)

## Симптом

`el.setAttribute('src', value)` не меняет того, что читает `el.src`: геттер, поставленный
`Object.defineProperty` в патче аудио-шима, замыкается на `var _src`, снятый один раз из
`getAttribute` при патче (`audio_element.rs:346`), и обновляется только собственным сеттером
свойства (`:513-516`), а не прямой записью атрибута. Для элемента, созданного без `src`,
геттер навсегда возвращает пустую строку — WPT
`html/infrastructure/urls/resolving-urls/query-encoding/attributes.sub.html?encoding=windows-1252`,
подтест «Getting <audio>.src» FAIL (`"" did not end with ?%FF`) — единственный остаток после
фикса [BUG-1775](BUG-1775-FIXED.md) (25/26, 2026-11-09).

## Воспроизведение

`var a = document.createElement('audio'); a.setAttribute('src', 'x.mp3'); a.src` → `""`
(ожидание — абсолютный URL от базы документа). `getAttribute('src')` при этом честно
возвращает `x.mp3`: расхождение только в IDL-геттере.

## Направление

Геттер должен читать атрибут живьём (`el.getAttribute('src')` с резолвом как у остальных
URL-атрибутов — `_lumen_reflect_url`/`_url_resolve_doc` в шимах), `_src` оставить только
внутреннему состоянию загрузки. Свериться с видео-шимом: `Getting <video>.src` после
BUG-1775 проходит.
