# BUG-136

**Статус:** FIXED 2026-06-13
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

INTERACTION TEST-105 (float/clear×margin) 4.84%: три дефекта float-раскладки. (1) c1 — пустой in-flow блок с margin:0 100px между двумя флоатами схлопывался в width 0: margin/width резолвились против суженной флоатами полосы вместо полной ширины CB (CSS 2.1 §9.5). Fix: для пустого auto-width блока (`!has_in_flow_content`, не-BFC) резолвить геометрию против content_width и клиппить к не-флоат-полосе → (465,25,100). (2) c2 — clear:both;margin-top:30px ставил блок на float_bottom+margin (175); clearance должна поглощать margin (CSS 2.1 §9.5.2) → max(natural,float_bottom)=145. Fix: clearance_pre + start_y-ветка. (3) c3 — третий float не помещался (146×3 > 300) но не переносился, висел за краем; теперь `next_float_bottom`+drop-цикл (rule 8) → перенос на y=489. c4/c5 без изменений (визуально верны). Верифицировано --dump-layout всех 6 ячеек + 3 регресс-теста. TEST-37 геометрия идентична (пустые блоки ml=mr=0 → клип == прежнему сужению).
