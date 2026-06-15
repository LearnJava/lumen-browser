# BUG-038

**Статус:** FIXED 2026-05-26
**Компонент:** layout

## Описание

list-style-position: inside — маркер занимал отдельную строку; li высотой 2× от нормы; fix: не продвигать child_y, сдвигать InlineRun вправо на marker_w
