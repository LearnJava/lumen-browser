# BUG-936 — `PushMaskLayer`/`PopMaskLayer` wgpu composite renders no visible mask effect

**Статус:** OPEN
**Компонент:** paint (`crates/engine/paint/src/renderer.rs` — `MaskLayerComposite` execution, `crates/engine/paint/src/renderer/pipelines.rs::build_mask_layer_pipeline`, `crates/engine/paint/src/renderer/shaders.rs::MASK_LAYER_SHADER_SRC`)
**Найден:** 2026-09-01 (P1, LIB-9), при первом реальном использовании `PushMaskLayer`/`PopMaskLayer` (CSS Masking L1 §5) — механизм существовал в `renderer.rs` до LIB-9, но ни один emit-путь его не строил, поэтому GPU-композит ни разу не исполнялся до сих пор (единственные конструкторы команды были юнит-тестами display-list, не живым рендером).

## Симптом

`graphic_tests/156-svg-mask.html` (LIB-9, SVG `<mask>` через 6 панелей) рендерится
**без какого-либо маскирования** в живом окне (wgpu, дефолтный бэкенд): каждая
панель показывает содержимое элемента полностью, как если бы маска не
применялась вовсе (мозаика rect/circle/path видна целиком, luminance- и
alpha-панели с одинаковой серой заливкой маски визуально неразличимы — обе
показывают полностью непрозрачный контент). Диф против Edge — 10.02%
(`x:20–660 y:21–341`).

**Ровно тот же HTML через `--screenshot` (CPU-растеризатор, `cpu_raster.rs`)
рендерится КОРРЕКТНО** — побайтово совпадает с ожидаемым/Edge-эталоном (все
6 панелей: половинчатая маска rect, круговая маска circle, составная "H"-маска,
luminance-полупрозрачность, alpha-полная-видимость, CSS `mask:` на path).
Это подтверждает: резолвинг `<mask>` (`box_tree/svg.rs::resolve_svg_mask`) и
эмиссия display-list-команд (`svg_text_decoration.rs::emit_svg_shape_masked`)
корректны — баг локализован именно в wgpu-исполнении.

Диагностическая подмена шейдера (`fs_alpha` временно возвращал `vec4(m.rgb, m.a)`
вместо вычисленной маски) дала недостоверный результат — снятый скриншот
оказался залит одним сплошным цветом на весь кадр (похоже на артефакт захвата
gdigrab при потере фокуса окна, см. известную ловушку в CLAUDE.md «Known
gotchas» — фоновые bash-команды в этой же сессии могли перехватывать фокус),
поэтому эта подмена не дала надёжного сигнала и была отменена без выводов.

## Подтверждено НЕ являющимся причиной (проверено вживую, с `eprintln!`-инструментацией, впоследствии убранной)

- **Не backend-специфично**: идентичный диф 10.02% и на Vulkan (`[probe] бэкенд
  выбран: Vulkan`), и с `WGPU_BACKEND=dx12` — значит логическая ошибка в общем
  коде плана рендера, а не квирк драйвера.
- **Display-list корректен**: `--dump-display-list` показывает точную,
  сбалансированную последовательность для каждой панели —
  `PushOpacity 1.000` → `FillRect/FillRoundedRect` (собственная заливка формы)
  → `PushMaskLayer (rect) mode=Luminance|Alpha` → `FillRect/FillRoundedRect`
  (содержимое маски, например `#ffffffff` белый прямоугольник) → `PopMaskLayer`
  → `PopOpacity`. Ровно то, что ожидает `PopMaskLayer`'s doc-comment
  (`display_list.rs:663-684`): контент элемента рисуется СНАЧАЛА, в уровне
  ниже, контент маски — между Push/Pop, в новом уровне.
- **`from_level` доходит корректным**: `eprintln!`-замер показал
  `MaskLayerComposite from_level=2` (ожидаемо: изоляция открывает уровень 1,
  `PushMaskLayer` — уровень 2), проходит guard `from_level < 2`.
  `self.layer_textures.len()=2` — обе текстуры аллоцированы (`ensure_layer_textures`,
  вызывается один раз перед циклом исполнения плана, `renderer.rs:3875`).
- **Draw-батчи для уровня маски (2) эмитятся с `ClearTransparent`**: замер
  показал `Draw target_level=2 ops_start=N ops_end=M load_op=ClearTransparent`
  для каждой панели, с непустым диапазоном ops — контент маски действительно
  рисуется в offscreen-слой перед композитом.
- **Индексация `parent_idx`/`mask_idx` совпадает с конвенцией остального кода**:
  `parent_idx = from_level - 2`, `mask_idx = from_level - 1` — та же формула,
  что уже рабочий путь `RenderPlanItem::Composite` (`renderer.rs:4442`,
  `comp.from_level - 2`) и рабочий путь `RenderPlanItem::MaskComposite`
  (`renderer.rs:4643`, `comp.from_level - 2`) используют для родителя.
- **BindGroupLayout совместим**: `mask_composite_bgl` (`construct.rs:742-772`) —
  0=texture, 1=texture, 2=sampler, ровно порядок, который ждёт
  `MASK_LAYER_SHADER_SRC` (`t_content`=0, `t_mask`=1, `s`=2).
- **Пайплайны не перепутаны**: `mask_layer_alpha_pipeline`/`mask_layer_luma_pipeline`
  используют `fs_alpha`/`fs_luma` в правильном порядке, оба возвращаются как
  `(alpha, luma)` кортеж, совпадающий с местом вызова
  (`MaskMode::Alpha => .0, MaskMode::Luminance => .1`, `renderer.rs:5484-5486`).
- **Никаких wgpu validation errors/паник в stderr** ни на одном прогоне —
  ошибка не всплывает как явный сбой, эффект просто отсутствует.

## Не проверено (следующие шаги для того, кто возьмёт баг)

- **GPU-readback** фактического содержимого `layer_textures[0]`/`[1]` после
  композита — самый прямой способ увидеть, что именно записано (требует
  async buffer mapping после `queue.submit`, не собрано в этой сессии).
  Живая визуальная GPU-отладка (RenderDoc/PIX) не проводилась.
- **Квад композита** (`transformed_grad_quad`/`grad_quad`,
  `paint_primitives.rs:881-903`) — геометрически выглядит корректным (2
  треугольника, покрывающих `rect`, `uv` в `[0,1]`), но не подтверждён
  экспериментально (например, вырожденный квад с нулевой площадью дал бы
  ровно наблюдаемый симптом — «эффект отсутствует, ошибок нет» — без
  видимой причины в коде, если баг в `dx,dy`/scroll-сдвиге или в
  `apply_affine_to_grad_verts`, применяемом лишний раз).
- **Копирование `parent → scratch`** (`copy_texture_to_texture`,
  `renderer.rs:5452-5458`) — не подтверждено, что скопированный кадр
  действительно содержит дорисованный `FillRect` уровня 1 к моменту копии
  (порядок команд в энкодере предполагает «да», но не проверено readback'ом).
- Стоит попробовать **минимальную репродукцию** — одна SVG-панель с одной
  маской на пустой странице — чтобы исключить любое (маловероятное)
  межпанельное взаимодействие через переиспользуемые `layer_textures`/`scratch_layer`.

## Область влияния

Затрагивает только путь SVG `<mask>` (LIB-9, `box_tree/svg.rs::resolve_svg_mask`
→ `svg_text_decoration.rs::emit_svg_shape_masked`) — единственный сегодняшний
эмиттер `PushMaskLayer`/`PopMaskLayer`. Существующие `mask-image`/`mask`
раст­ровые и градиентные маски (`PushMaskImage`/`PushMask*Gradient`/`PopMask`,
другой, рабочий путь) не затронуты — они используют другую пару команд с
другой (уже проверенной) композит-логикой (`MaskComposite`, не
`MaskLayerComposite`).

**CPU-снапшот-гейт (`SAVE_CPU_SNAPSHOTS=1 cargo test -p lumen-driver --features
cpu-render cases::snapshot_cpu`) не задет** — `cpu_raster.rs`'s `LayerComposite::MaskLayer`
(добавлено в LIB-9) рендерит корректно, подтверждено визуально.

`graphic_tests/156-svg-mask.html` зарегистрирован в `KNOWN_DEBTORS`
(`graphic_tests/run.py`) с baseline 10.02% до исправления этого бага.
