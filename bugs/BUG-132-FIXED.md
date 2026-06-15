# BUG-132

**Статус:** FIXED 2026-06-12
**Компонент:** paint
**Файл:** (нет)

## Описание

INTERACTION TEST-101 (border-radius×overflow) 4.04%: Phase 0 interface-first done — добавлена PushClipRoundedRect в DisplayCommand, box_layer_ops() генерирует её для overflow:hidden с border-radius, femtovg_backend использует scissor fallback. Phase 1 (real rounded mask): TBD
