# BUG-276 — wgpu-бэкенд не проходит TEST-00 против эталона Edge (4.85%, никогда не проверялось)

**Статус:** FIXED 2026-07-13 — root-caused и исправлен в `p1-wgpu-bug276`
**Компонент:** paint (`WgpuBackend` / `renderer.rs`) — PushClipRect не применял accumulated transform
**Найден:** 2026-07-12 · **Исправлен:** 2026-07-13

## Симптом

`graphic_tests/run.py` до сих пор ни разу не гонялся с `LUMEN_BACKEND=wgpu` — набор всегда
неявно проверял только дефолтный femtovg-бэкенд. Первый же прогон под wgpu:

```
LUMEN_BACKEND=wgpu python graphic_tests/run.py --only 00
TEST-00: FAIL (4.85%)  [x:1-1022 y:684-718]
```

Воспроизведено дважды подряд (не флак по гейт-паттерну `reference_gdigrab_test00_retry` —
там симптом другой, «magenta marker not found»; здесь стабильный числовой диф). Тот же
прогон с дефолтным `LUMEN_BACKEND` (femtovg) — `TEST-00: PASS (0.00%)`.

Диф локализован в полосе `y:684-718` (нижние ~35px из 720px viewport), почти на всю ширину
(`x:1-1022` из 1024). Похоже на несовпадение в нижней части окна (scrollbar? разница
viewport/DPI-округления между wgpu- и femtovg-окном? артефакт калибровки magenta-рамки под
конкретно wgpu-презентацией) — ни одна гипотеза не проверена.

## Не регрессия конкретной задачи

Найдено в ветке `p1-wgpu-default-backend-probe`, но эта ветка меняла только выбор
GPU-бэкенда (DX12/Vulkan/GL проба + цепочка резервов, `backend_probe.rs`) — рендер-код не
трогался, и проба приняла тот же DX12, что wgpu-путь и раньше использовал по умолчанию на
Windows. То есть этот дефект, вероятно, существовал и раньше — просто graphic_tests никогда
не проверялся против wgpu, чтобы его увидеть.

## Почему это важно для Ph-wgpu-default

План перевода wgpu в дефолтный бэкенд (`docs/tasks/ph-wgpu-default.md`) использует
`graphic_tests/run.py` c `LUMEN_BACKEND=wgpu` как приёмочный гейт для каждого среза Фазы 2, по
0.5%-порогу. Раз даже TEST-00 (калибровочный, самый простой тест) не проходит на wgpu сегодня,
**баз-лайн pass-rate для wgpu — не 100%**, и его нужно установить полным прогоном набора
ДО того, как сравнивать срезы Фазы 2 между собой (иначе непонятно, испортил ли срез что-то
новое, или это старый долг). Финальный гейт Фазы 3 (флип дефолта) тоже упирается в этот баг.

## Root-cause (установлен 2026-07-13)

Shell оборачивает page display list в `PushTransform(translate(0, TAB_BAR_HEIGHT=36))`, чтобы
сдвинуть контент ниже таб-бара (wgpu-путь, поскольку `supports_page_offset() = false`). Обработчик
`PushClipRect` в `renderer.rs` клал rect `(0,0,1024,720)` прямо в `clip_stack`, не применяя
накопленный transform из `transform_stack`. `sync_scissor_to_stack()` затем выставлял scissor в
устройственных координатах `(0,0,1024,720)`, обрезая контент по y=720. Но реальный контент
начинается с y=37 (tab bar) → нижние 35px (720..755) оказывались за пределами scissor →
рисовались в цвет очистки (magenta) → диф 4.85% в полосе y:684-718.

## Фикс

`crates/engine/paint/src/renderer.rs` — добавлен хелпер `apply_transform_to_clip(rect, m)`:
вычисляет AABB трансформированных углов rect через `m.transform_point_2d()` и возвращает
screen-space clip rect. Применяется в трёх обработчиках: `PushClipRect`, `PushClipRoundedRect`,
`PushClipPath` — сразу после `translate_rect(dx, dy)` и до пересечения с `clip_stack.last()`.

```
TEST-00: FAIL (4.85%)  →  TEST-00: PASS (0.00%)
```

Полный wgpu-прогон после фикса: 65/141 PASS, 38 FAIL (wgpu-специфичные расхождения с Edge),
38 DEBTOR (существующие femtovg-долги). Новый баг BUG-277 фиксирует список этих 38 тестов
как wgpu-базлайн для Фазы 3.
