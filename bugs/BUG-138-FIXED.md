# BUG-138

**Статус:** FIXED 2026-06-13
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

INTERACTION TEST-107 (shadow×radius×overflow): box-shadow на скруглённом боксе эмитился квадратным FillRect, игнорируя border-radius — `emit_box_shadows` теперь эмитит FillRoundedRect с радиусами border-box + spread (CSS Backgrounds L3 §7.1.1); клип тени overflow:hidden-родителем уже работал
