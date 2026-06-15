# BUG-026

**Статус:** FIXED 2026-05-22
**Компонент:** layout/paint

## Описание

`<img>` CSS/HTML width+height ignored — renders at natural size (remaining TEST-18 ~10%: BUG-032)

## Детали

TEST-18: `<img width="300" height="225">` рендерится в натуральном размере файла. Команда `DrawImage` должна использовать layout-rect, не натуральный размер текстуры.
