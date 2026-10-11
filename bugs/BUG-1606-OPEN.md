# BUG-1606 — chrome: `.modal` с `max-height:82%` сплющивается до ~100 px (панель спецвозможностей показывает одну строку)

**Статус:** OPEN
**Заведён:** 2026-10-11 (найден живой проверкой при исправлении [BUG-1605](BUG-1605-FIXED.md))
**Область:** layout (flex column + `max-height:%` внутри `position:absolute; inset:0` flex-контейнера) / `assets/chrome/chrome.html` (`.modal`, `.modal-body`)

## Симптом

Ctrl+Shift+Q после BUG-1605 показывает модал «Специальные возможности», но высотой ~100 px: заголовок, одна строка «Размер шрифта» с обрезанными пилюлями и кнопка «Готово» поверх неё; тело с `overflow-y:auto` получило полосу прокрутки. Ожидается высота по содержимому (все четыре строки), как в макете. Палитра команд того же окна рисуется нормально.

## Воспроизведение

`lumen.exe --maximized <страница>`, Ctrl+Shift+Q, снимок окна. Правила: `.modal-overlay{position:absolute; inset:0; display:flex; align-items:center}`, `.modal{max-height:82%; display:flex; flex-direction:column; overflow:hidden}`, `.modal-body{overflow-y:auto; flex:1}`.

## Направление

Выяснить, как layout разрешает процентный `max-height` и авто-высоту flex-колонки с `flex:1; overflow:auto` ребёнком: вероятно, `flex-basis` тела считается как 0 / min-content при определённой высоте контейнера. Проверить `#certOverlay`, `#printOverlay`, `#shortcutsOverlay` — те же классы.
