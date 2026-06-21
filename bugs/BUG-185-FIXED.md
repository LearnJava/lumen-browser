# BUG-185

**Статус:** FIXED 2026-06-21 (KNOWN_DEBTORS BUG-128)
**Компонент:** layout/paint
**Тест:** TEST-32 (3.75% → KNOWN_DEBTORS BUG-128)

## Описание

list markers: `::marker` box geometry, outside/inside, disc/decimal/alpha/roman.

## Корень и фикс

Две независимые геометрии `::marker` расходились с Edge:

1. **`::marker { content }` рисовал bullet вместо строки** (`paint/src/display_list.rs`,
   `emit_list_marker`). У маркера с `list-style-type: disc` + `::marker { content: "→ " }`
   поле `list_style_type` оставалось `Disc`, а текст «→ » клался в `text`. Painter
   матчил по `list_style_type` ПЕРВЫМ → рисовал диск и игнорировал строку. Фикс:
   армы `Disc`/`Circle`/`Square` получили guard `if text.is_empty()` — непустой `text`
   (counter-глиф ИЛИ `::marker { content }` override) всегда падает в text-ветку и
   побеждает bullet-форму (CSS Lists L3 §2.1 / Pseudo-elements L4 §14.2).

2. **Широкий маркер переполнялся в контент** (`layout/src/box_tree.rs`, marker
   positioning в `lay_out`). Бокс маркера был зафиксирован на `em * 1.5` (≈24px @16px).
   Длинный `@counter-style` prefix/suffix вроде «#1: » (≈28px) переполнял бокс и
   налезал на первое слово контента → «#1:One» вместо «#1: One». Фикс: для text-маркеров
   меряем ширину строки (`measure_text_w`) и берём `marker_w = (em*1.5).max(text_w)`;
   бокс растёт ВЛЕВО (`x = content_x - marker_w`), поэтому строка right-align'ится
   правым краем у контент-края (CSS Lists L3 §2.4). Узкие маркеры («1.», «a.», «i.»)
   ≤ дефолтного бокса — поведение не меняется (нет регрессий).

Скриншот: зелёные стрелки content-marker и «#1: One»/«#2: Two»/«#3: Three» совпадают
с Edge по геометрии.

## Остаток (KNOWN_DEBTORS BUG-128)

3.75% — целиком font-parity: Edge рендерит serif, Lumen — Inter sans по ВСЕЙ странице
(~50 текстовых строк списков/меток, rule 3). Дополнительно list-style-image data-URI
рисует disc вместо картинки (отдельная CSS-проводка, не геометрия маркера). TEST-32 →
KNOWN_DEBTORS (BUG-128, baseline 3.75%).

## Регресс-тесты

- `paint`: `marker_content_override_renders_text_not_bullet` (display_list.rs)
- `layout`: `wide_marker_box_grows_and_right_aligns_at_content_edge` (box_tree.rs)
