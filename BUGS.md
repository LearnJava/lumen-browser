# BUGS.md — Баг-трекер Lumen Browser

Этот файл — живой список известных багов и ограничений движка.  
Читается Claude автоматически через CLAUDE.md (см. ниже как добавлять баги).

**Как добавить баг:**
1. Скопируй скриншот в `graphic_tests/screenshots/bug-NNN-краткое-имя.png` (не коммитится, в `.gitignore`)
2. Добавь запись ниже в нужную секцию
3. Сообщи Claude: «посмотри BUGS.md, возьми баг-NNN»

**Источник багов:**
- Старые `BUG-001..019` — ручной анализ дампов `samples/test-*.html` (2026-05-15).
- `BUG-020..*` — блокирующий пайплайн `python graphic_tests/run.py` (см. CLAUDE.md → «Графические тесты»).

**Статусы:** `OPEN` · `IN PROGRESS` · `FIXED` · `WONTFIX (Phase N+)`

---

## Сводная таблица (grep-friendly)

```
BUG-002 | OPEN        | layout          | inline padding/border/margin stacks vertically instead of flowing
BUG-004 | OPEN        | layout          | height on inline elements ignored
BUG-005 | OPEN        | paint           | <img> inside <span> not rendered
BUG-006 | OPEN        | layout          | table layout not implemented (td/th render as blocks)
BUG-007 | OPEN        | layout          | <sub>/<sup>/<small> missing UA styles
BUG-008 | OPEN        | layout          | <del>/<ins>/<u>/<s> text-decoration missing UA styles
BUG-009 | OPEN        | layout          | <a> missing UA styles (no blue color, no underline)
BUG-010 | OPEN        | layout/paint    | <hr> renders nothing
BUG-011 | OPEN        | layout/paint    | list markers (bullet, numbers) not rendered
BUG-012 | OPEN        | layout          | <del>/<ins> break inline flow (each on new line)
BUG-013 | OPEN        | layout          | adjacent <span style="..."> stack vertically without separator
BUG-014 | OPEN        | image           | JPEG not decoded (PNG only)
BUG-015 | OPEN        | shell/paint     | broken <img> src shows no alt text
BUG-016 | OPEN        | css-parser/paint| border-style: only solid works; dashed=solid, double/groove/ridge=none
BUG-017 | OPEN        | layout/paint    | text-decoration-style ignored (all render as solid)
BUG-018 | OPEN        | layout          | text-decoration-color ignored (always inherits text color)
BUG-019 | OPEN        | css-parser/paint| outline not rendered at all
BUG-020 | OPEN        | layout/paint    | overflow:scroll/auto/hidden treated as visible
BUG-021 | OPEN        | html-parser     | HTML bgcolor attribute ignored
BUG-022 | OPEN        | css-parser      | Quirks-mode hashless hex colors not parsed
BUG-023 | FIXED       | paint           | opacity property completely ignored (renders at 100%)
BUG-024 | OPEN        | layout          | box-sizing: content-box — border не добавляется к внешнему размеру
BUG-025 | OPEN        | layout          | max-height не зажимает высоту (height > max-height не обрезается)
BUG-026 | OPEN        | layout/paint    | <img> CSS/HTML width+height не масштабирует изображение
BUG-027 | OPEN  [P1]  | layout          | block element explicit width ignored — body always sized to viewport width
BUG-028 | OPEN  [P3]  | shell           | relayout-on-resize + maximized window breaks all graphic tests (BUG-027 trigger)
BUG-001 | FIXED       | layout          | display:none on inline elements not working
BUG-003 | FIXED       | layout          | style="" attribute not processed by cascade
```

> Полные описания, воспроизведение и ссылки на код — в секциях ниже.
> `grep "OPEN" BUGS.md` — все открытые баги одной командой.

---


## Открытые баги

### BUG-002 · Inline-элементы с `padding`/`border`/`margin` стакаются вертикально

**Статус:** OPEN  
**Компонент:** `lumen-layout` — inline flow  
**Обнаружен:** 2026-05-15  
**Скриншот:** `bugs/screenshots/` *(нет)*

**Описание:**  
Span-элементы с любым `padding`, `border` или `margin` не текут горизонтально —  
каждый начинается с новой строки (ведут себя как блоки).  
Span без этих свойств (только `color`, `font-weight` и т.п.) работают правильно.

**Воспроизведение:**
```html
<p>
  <span style="padding: 2px 8px; border: 1px solid red;">span 1</span>
  <span style="padding: 2px 8px; border: 1px solid blue;">span 2</span>
</p>
```
**Ожидается:** span 1 и span 2 на одной строке.  
**Факт:** каждый span — на отдельной строке.

**Где смотреть:** `crates/engine/layout/src/inline.rs` или аналог — формирование inline line box при наличии box model на inline-элементе.

---

### BUG-004 · `height` на inline-элементах игнорируется

**Статус:** OPEN  
**Компонент:** `lumen-layout` — высота inline-box  
**Обнаружен:** 2026-05-15  
**Скриншот:** `bugs/screenshots/` *(нет)*

**Описание:**  
`height: 40px` на `<span>` не применяется — высота определяется строчным интервалом, а не `height`.  
(Это стандартное поведение CSS для `display:inline`, но в Phase 0 нет `display:inline-block` — альтернативы нет.)

**Воспроизведение:**
```html
<span style="height: 40px; background: red;">текст</span>
```
**Ожидается (для inline-block):** span высотой 40px.  
**Факт:** высота по line-height, `height` игнорируется.

**Заметка:** Исправляется добавлением `display:inline-block`. Блокирует демо высот в тестовых страницах.

---

### BUG-005 · `<img>` внутри `<span>` не отрисовывается

**Статус:** OPEN  
**Компонент:** `lumen-paint` — DrawImage  
**Обнаружен:** 2026-05-15  
**Скриншот:** `bugs/screenshots/` *(нет)*

**Описание:**  
`<img>` рендерится только как прямой потомок блочного элемента (`<div>`).  
При вложении в `<span>` изображение не появляется.

**Воспроизведение:**
```html
<span><img src="photo.jpg" width="100" height="100"></span>  <!-- не работает -->
<div><img src="photo.jpg" width="100" height="100"></div>    <!-- работает -->
```

**Где смотреть:** `crates/engine/paint/src/` — поиск DrawImage команды для inline-контекста.

---

### BUG-006 · Таблицы не имеют табличного layout (`td`/`th` рендерятся как блоки)

**Статус:** OPEN  
**Компонент:** `lumen-layout` — table layout  
**Обнаружен:** 2026-05-15  
**Скриншот:** `bugs/screenshots/` *(нет)*

**Описание:**  
Элементы `<td>` и `<th>` рендерятся как обычные блочные `<div>` — каждая «ячейка» занимает всю строку.  
Горизонтального выравнивания колонок нет.

**Заметка:** Table layout — крупная фича, не входит в Phase 0. WONTFIX до Phase 1+.

**Подтверждено 2026-05-19** на `graphic_tests/20-quirks-bgcolor.html` (пайплайн): `<table>` с 10 `<td>` ячейками не рисуется ни одной ячейкой даже с bgcolor-атрибутами — в Lumen пустая страница, в Edge сетка 5×2.

---

### BUG-007 · `<sub>` / `<sup>` / `<small>` без UA-стилей

**Статус:** OPEN  
**Компонент:** `lumen-layout` — user-agent stylesheet  
**Обнаружен:** 2026-05-15 на `test-05-html-tags.html`  
**Скриншот:** `bugs/screenshots/` *(нет)*

**Описание:**  
- `<sub>` / `<sup>` — должны уменьшать font-size (≈0.83em) и смещать baseline (sub вниз, sup вверх). В Lumen рендерятся как обычный текст одного font-size, без смещения. В дампе test-05: `"— H 2 O (sub) — E=mc 2 (sup) —"` — цифры идут на baseline тем же размером.
- `<small>` — должен уменьшать font-size (≈0.83em). В Lumen: `"— small" 13.00` — тот же размер, что и окружающий текст.

**Где смотреть:** UA-stylesheet / встроенные пресентационные правила в `compute_style` (`default_display` рядом + аналог для font-size). Также `vertical-align: sub|super` поверх baseline.

---

### BUG-008 · `<del>`, `<ins>`, `<u>`, `<s>` — `text-decoration` не применяется

**Статус:** OPEN  
**Компонент:** `lumen-layout` — UA stylesheet  
**Обнаружен:** 2026-05-15 на `test-05-html-tags.html`

**Описание:**  
- `<del>`, `<s>` — должны быть `text-decoration: line-through`. В дампе нет признаков decoration (text-decoration_line у инлайн-фрагов остаётся `None`).
- `<ins>`, `<u>` — должны быть `text-decoration: underline`. То же — отрисованы без линии.

Эти 4 элемента не получают UA-стиль (HTML 4.01/HTML5 Rendering §15.3.7).

**Где смотреть:** `lumen-layout` UA stylesheet или `compute_style` defaults.

---

### BUG-009 · `<a>` без UA-стилей (нет синего цвета и underline)

**Статус:** OPEN  
**Компонент:** `lumen-layout` — UA stylesheet  
**Обнаружен:** 2026-05-15 на `test-05-html-tags.html`

**Описание:**  
`<a href="...">` без author-CSS должен по умолчанию быть синим (`:link { color: blue }`) и подчёркнутым.
В Lumen: `"ссылка <a>" 13.00 #2d3748ff` — наследованный цвет, без underline.

**Где смотреть:** UA stylesheet — добавить базовые правила для `a:link / a:visited`.

---

### BUG-010 · `<hr>` не рисует разделитель

**Статус:** OPEN  
**Компонент:** `lumen-layout` / `lumen-paint`  
**Обнаружен:** 2026-05-15 на `test-05-html-tags.html`

**Описание:**  
`<hr>` (даже без inline-style) не оставляет следов в display list — нет ни FillRect, ни DrawBorder, ни DrawLine.  
Должна быть тонкая серая линия по умолчанию (UA `border-top: 1px inset; height: 0`) или эквивалент.

**Где смотреть:** UA stylesheet + специальный `replaced-element-like` рендеринг для `<hr>` (минимум как блок с border-top на полной ширине).

---

### BUG-011 · List markers (`•` для `<ul>`, `1. 2. 3.` для `<ol>`) не рисуются

**Статус:** OPEN  
**Компонент:** `lumen-layout` / `lumen-paint` — markers/pseudo `::marker`  
**Обнаружен:** 2026-05-15 на `test-05-html-tags.html`

**Описание:**  
`<li>` рендерятся как обычные блочные строки текста без маркеров. В дампе:
```
DrawText (20.00, 318.25, ...) "Нейронные сети" 12.00
DrawText (20.00, 336.65, ...) "Машинное обучение" 12.00
```
— ни `•`, ни «1.», «2.», «3.».

CSS Lists L3: `list-style-type` / `::marker` / `marker-side: inside|outside`. У Lumen `list_style_type` есть в ComputedStyle, но генерация маркер-фрага в layout/paint, по-видимому, не реализована.

**Где смотреть:** `lumen-layout::box_tree` — генерация anonymous marker-box для `display: list-item`.

**Подтверждено повторно** 2026-05-15 на `test-07-decorations.html`: ни `disc`/`square` (для `<ul>`), ни `upper-roman`/`lower-alpha`/`decimal-leading-zero` (для `<ol>`) не рисуются. Когда исправление подойдёт — нужно покрыть все эти типы. См. `bugs/screenshots/bug-019-outline.png` (нижняя секция «list-style-type» — текст без маркеров).

---

### BUG-012 · `<del>` и `<ins>` ломают inline-flow (каждый — на отдельной строке)

**Статус:** OPEN  
**Компонент:** `lumen-layout` — inline flow / default display  
**Обнаружен:** 2026-05-15 на `test-05-html-tags.html`

**Описание:**  
`<del>` и `<ins>` — inline-элементы по HTML5, но в Lumen внутри `<p>` каждый идёт на новой строке. Из дампа test-05:
```
DrawText (404.31, 53.50, ...) "—"            ← конец строки 1
DrawText (0.00, 72.35, ...) "зачёркнутый (del)"   ← новая строка
DrawText (0.00, 91.20, ...) "—"
DrawText (0.00, 110.05, ...) "подчёркнутый (ins)" ← новая строка
DrawText (0.00, 128.90, ...) "— H 2 O (sub) — E=mc 2 (sup) —"
```
`<strong>`, `<em>`, `<b>`, `<i>`, `<sub>`, `<sup>`, `<u>`, `<s>` — остаются inline; только `<del>` и `<ins>` ломают поток.

**Где смотреть:** `default_display` в `crates/engine/layout/src/style.rs` — для `del`/`ins` возможно стоит `Block` вместо `Inline`.

---

### BUG-013 · Несколько `<span style="...">` подряд без разделителя стакаются вертикально

**Статус:** OPEN  
**Компонент:** `lumen-layout` — inline flow при наличии inline-style-атрибута  
**Обнаружен:** 2026-05-15 на `test-05-html-tags.html` (секция «Семантика HTML5»)

**Описание:**  
Если в `<p>` несколько `<span style="...">` идут подряд, разделённые только whitespace (`\n  `), без визуального разделителя (`—`, `,`, чего-то ещё), то каждый span после первого рисуется на новой строке. С разделителем-текстом между ними — всё инлайн (test-01).

Из дампа test-05:
```
DrawText (0.00, 1033.00, ...) "Браузер корректно парсирует: <article>"
DrawText (0.00, 1051.85, ...) "<section>"
DrawText (0.00, 1070.70, ...) "<header>"
...
DrawText (0.00, 1146.10, ...) "<aside> — все рендерятся как блочные элементы."
```
— первый span (`<article>`) клеится к предыдущему тексту, остальные — каждый на новой строке. Последний (`<aside>`) — обратно inline вместе с trailing text. Без `style=""` атрибута spans стакались бы корректно.

**Гипотеза:** связано с BUG-003 — наличие `style`-атрибута на span как-то меняет inline-сегментацию (возможно, неизвестный атрибут заставляет считать узел не-inline-content, см. `is_inline_content` в `box_tree.rs:193`). Проверить можно сразу после фикса BUG-003.

---

### BUG-014 · JPEG не декодируется

**Статус:** OPEN — известное ограничение, не in roadmap для Phase 0  
**Компонент:** `lumen-image`  
**Обнаружен:** 2026-05-15 на `test-04-images.html`

**Описание:**  
`lumen-image` поддерживает только PNG (свой декодер). JPEG-файлы (`ai_wikipedia.jpg`) попадают в display list как DrawImage, но без декодированного содержимого — рисуется placeholder/ничего.

**Заметка:** по политике зависимостей JPEG-декодер — provisional accelerator (`zune-jpeg`), graduation criterion «никогда» — берётся готовый. Trait-anchor `ImageDecoder` ещё не выделен.

---

### BUG-015 · `<img>` с несуществующим `src` не показывает alt-текст

**Статус:** OPEN  
**Компонент:** `lumen-shell` / `lumen-paint` — fallback при ошибке загрузки  
**Обнаружен:** 2026-05-15 на `test-04-images.html` (последний блок)

**Описание:**  
`<img src="images/nonexistent.png" alt="[файл не найден — alt-текст]">` — должен показать alt-текст внутри прямоугольника `width × height`. В Lumen — пустое место (DrawImage эмитится, но изображение не зарегистрировано, ничего не рисуется), alt не отображается.

**Где смотреть:** `lumen-shell` (decode images перед resumed) и `lumen-paint` — для DrawImage с unregistered src нужно рендерить fallback (alt-текст или плейсхолдер).

---

### BUG-016 · `border-style` поддерживает только `solid` (dashed/dotted рисуются как solid, double/groove/ridge — не рисуются вовсе)

**Статус:** OPEN  
**Компонент:** `lumen-layout` (cascade) / `lumen-paint` (stroke)  
**Обнаружен:** 2026-05-15 на `test-07-decorations.html`  
**Скриншот:** `bugs/screenshots/bug-016-border-style.png`

**Описание:**  
Из всех значений `border-style` корректно работает только `solid`. `dashed` и `dotted` отрисовываются как сплошная линия (стиль игнорируется, fallback к solid). `double`, `groove`, `ridge` приводят к **полному отсутствию рамки** — рамки не рисуются вовсе (вероятно, стиль не парсится и итоговый `border-style: none`).

**Воспроизведение:**
```html
<div style="border: 3px dashed #c00; padding: 8px;">dashed</div>   <!-- рисуется как solid -->
<div style="border: 3px dotted #c00; padding: 8px;">dotted</div>   <!-- рисуется как solid -->
<div style="border: 6px double #c00; padding: 8px;">double</div>   <!-- рамки нет -->
<div style="border: 6px groove #c00; padding: 8px;">groove</div>   <!-- рамки нет -->
<div style="border: 6px ridge  #c00; padding: 8px;">ridge</div>    <!-- рамки нет -->
```

**Ожидается:** все 5 стилей визуально различимы (штрихи, точки, двойная линия, 3D-эффекты).  
**Факт:** dashed/dotted = solid, double/groove/ridge = no border.

**Гипотеза:** есть две разные ветви проблемы — (1) CSS-парсер принимает значения `dashed`/`dotted`, но paint в `display_list` всегда эмитит solid; (2) `double`/`groove`/`ridge` парсер не принимает и computed `border-style` остаётся `none`.

**Где смотреть:** `lumen-css-parser` (longhand `border-*-style`), `lumen-layout::style` (`BorderStyle` enum, если есть), `lumen-paint::display_list` (stroke command, варианты стиля).

---

### BUG-017 · `text-decoration-style` игнорируется (double/dotted/dashed/wavy рисуются как solid)

**Статус:** OPEN  
**Компонент:** `lumen-layout` (cascade) / `lumen-paint` (underline stroke)  
**Обнаружен:** 2026-05-15 на `test-07-decorations.html`  
**Скриншот:** `bugs/screenshots/bug-017-text-decoration-style.png`

**Описание:**  
Все варианты `text-decoration-style` (`double`, `dotted`, `dashed`, `wavy`) отрисовываются как обычное `solid`-подчёркивание. Различить визуально нельзя.

**Воспроизведение:**
```html
<p style="text-decoration: underline double #c00;">двойная</p>
<p style="text-decoration: underline dotted #c00;">точечная</p>
<p style="text-decoration: underline dashed #c00;">штриховая</p>
<p style="text-decoration: underline wavy   #c00;">волнистая</p>
```
**Ожидается:** четыре визуально разных underline-стиля.  
**Факт:** все четыре идентичны solid-варианту.

**Где смотреть:** `lumen-css-parser` (поддерживает ли он longhand `text-decoration-style`), `lumen-layout::style` (хранится ли стиль в `ComputedStyle`), `lumen-paint` (FillRect для подчёркивания — нужно расширить до stroke с pattern или wavy curve).

---

### BUG-018 · `text-decoration-color` игнорируется (underline всегда цвета текста)

**Статус:** OPEN  
**Компонент:** `lumen-layout` (cascade)  
**Обнаружен:** 2026-05-15 на `test-07-decorations.html`  
**Скриншот:** `bugs/screenshots/bug-018-text-decoration-color.png`

**Описание:**  
`text-decoration-color` не применяется. Подчёркивание всегда рисуется цветом `color` текущего элемента.

**Воспроизведение:**
```html
<p style="text-decoration: underline solid #0a0; color: #222;">зелёное подчёркивание под чёрным текстом</p>
```
**Ожидается:** чёрный текст с зелёной чертой снизу.  
**Факт:** чёрный текст с чёрной чертой.

**Где смотреть:** `lumen-css-parser` (поддерживает ли longhand `text-decoration-color`), `lumen-layout::style` (поле `text_decoration_color`), `lumen-paint` (использовать его при эмитировании underline вместо `color`).

---

### BUG-019 · `outline` не отрисовывается

**Статус:** OPEN  
**Компонент:** `lumen-css-parser` / `lumen-paint`  
**Обнаружен:** 2026-05-15 на `test-07-decorations.html`  
**Скриншот:** `bugs/screenshots/bug-019-outline.png`

**Описание:**  
Свойство `outline` (любого стиля и ширины) полностью игнорируется. Обводка не появляется. В отличие от `border`, `outline` не должен влиять на layout — но в Lumen его вообще нет ни в paint, ни в layout.

**Воспроизведение:**
```html
<div style="outline: 3px solid #07c; padding: 8px;">обводка снаружи</div>
<div style="outline: 3px dashed #07c; padding: 8px;">пунктирная обводка</div>
```
**Ожидается:** синяя/синяя пунктирная рамка снаружи блока, не сдвигающая соседей.  
**Факт:** обводка отсутствует.

**Где смотреть:** `lumen-css-parser` (longhand `outline-style/-width/-color/-offset`), `lumen-layout::style` (`outline_*` поля), `lumen-paint::display_list` (отдельный proceedingstep после стандартного painter — отрисовать вне border-box, не учитывая в layout).

**Подтверждено 2026-05-19** на `graphic_tests/16-outline.html` (пайплайн): 11 боксов с разными `outline` (1/2/4 px, solid/double/dashed, цветные) — ни одна обводка не видна. (Но коммиты `5a527a3`/`2e683a2` про outline-dash-dot указывают, что часть кода в paint реализована — возможно, проблема в том, что в `compute_style` outline не доходит из cascade.)

---

### BUG-020 · `overflow: scroll/auto/hidden` не реализован (обрабатывается как `visible`)

**Статус:** OPEN
**Компонент:** `lumen-layout` (overflow handling) / `lumen-paint` (clipping)
**Обнаружен:** 2026-05-19 на `graphic_tests/14-overflow.html` (пайплайн)
**Скриншот:** `graphic_tests/screenshots/14-*.png` *(не в репо)*

**Описание:**
В Edge на странице 14 видны вложенные элементы с разными значениями `overflow`: scrollbar-полоски внутри отдельных боксов с прокручиваемым контентом, обрезка по border-box при `overflow: hidden`. В Lumen — простой бокс без внутренних scrollbar-ов и без клиппинга, любой `overflow` ведёт себя как `visible`.

**Воспроизведение:**
```html
<div style="width: 200px; height: 100px; overflow: scroll; background: #f00;">
  <div style="width: 300px; height: 200px; background: #00f;"></div>
</div>
```
**Ожидается:** 200×100 красный бокс со scrollbar-ами, внутри частично виден синий 300×200.
**Факт:** красный бокс растягивается до 300×200, синий рисуется целиком, scrollbar-ов нет.

**Где смотреть:** `lumen-layout` (учёт `overflow` в установке размеров parent-box-а), `lumen-paint` (clip-команды + scrollbar-overlay по аналогии с глобальным `lumen-shell::scrollbar`).

---

### BUG-021 · HTML-атрибут `bgcolor` не применяется (на `<body>` и `<td>`)

**Статус:** OPEN
**Компонент:** `lumen-html-parser` (presentational hints) / `lumen-layout` (cascade)
**Обнаружен:** 2026-05-19 на `graphic_tests/20-quirks-bgcolor.html` (пайплайн)
**Скриншот:** `graphic_tests/screenshots/20-*.png` *(не в репо)*

**Описание:**
HTML-атрибут `bgcolor` — presentational hint, по HTML5 §15 обязан транслироваться в правило с UA-specificity. В Edge `<body bgcolor="#1a2030">` даёт тёмно-синий фон страницы; `<td bgcolor="red">` — красные ячейки. В Lumen оба игнорируются: body белый, ячейки пустые.

**Воспроизведение:**
```html
<body bgcolor="#1a2030">
  <table>
    <tr><td bgcolor="red">A</td><td bgcolor="#3182ce">B</td></tr>
  </table>
</body>
```

**Где смотреть:** `lumen-html-parser` / `lumen-layout` — добавить presentational-hints для `<body bgcolor>`, `<table bgcolor>`, `<tr bgcolor>`, `<td bgcolor>`, `<th bgcolor>` по HTML LS «mapped attributes» (рядом с уже реализованными `<img width|height>`).

**Связано:** при фиксе BUG-006 (table layout) `td bgcolor` начнёт работать автоматически — но `body bgcolor` независим, его можно сделать в первую очередь.

---

### BUG-022 · CSS hashless hex colors (Quirks-mode) не парсятся

**Статус:** OPEN
**Компонент:** `lumen-css-parser`
**Обнаружен:** 2026-05-19 на `graphic_tests/20-quirks-bgcolor.html` (пайплайн)

**Описание:**
В quirks-mode (HTML5 §13.2.5.1) CSS-парсер должен принимать hex-цвета без `#` в значениях presentational-hint атрибутов и в некоторых CSS-контекстах: `bgcolor="44aa66"`, `color="ff0000"` и т.п. трактуются как `#44aa66` / `#ff0000`. В Lumen такие значения игнорируются.

**Воспроизведение:**
```html
<!-- DOCTYPE отсутствует → Quirks mode -->
<html>
  <body>
    <div style="background: ff4444;"></div>     <!-- ожидается красный -->
    <td bgcolor="44aa66">cell</td>              <!-- ожидается зелёный -->
  </body>
</html>
```
**Ожидается:** Edge применяет `ff4444` и `44aa66` как `#ff4444` / `#44aa66`.
**Факт:** в Lumen ни одна из этих нотаций не работает.

**Где смотреть:** `lumen-css-parser` (color parser — добавить fallback на 3/6 hex digits без `#` если `DocumentMode::Quirks`), `lumen-html-parser` (legacy color parsing для presentational hints).

**Severity:** LOW (легаси). Phase 0 quirks-coverage не приоритетна, но trait-anchor под этот случай уже частично есть (`DocumentMode::Quirks` в DOM).

---

### BUG-023 · `opacity` полностью игнорируется (рендерится как 1.0)

**Статус:** FIXED 2026-05-19 (ветка `offscreen-opacity-1b4`, коммит `356ba0d`)
**Компонент:** `lumen-paint` (display list / compositor)
**Обнаружен:** 2026-05-19 на `graphic_tests/13-visibility-opacity.html` (пайплайн, 13.75%)

**Фикс:** P2 реализовал off-screen opacity layer rendering: поддерево с `opacity < 1.0` рисуется в отдельную RGBA-текстуру (OffscreenLayer), затем composit-ится с заданным alpha через `COMPOSITE_SHADER`. `PushLayer`/`PopLayer` в display list + render plan pipeline.

---

### BUG-024 · `box-sizing: content-box` — `border` не добавляется к внешнему размеру

**Статус:** OPEN
**Компонент:** `lumen-layout` — box model
**Обнаружен:** 2026-05-19 на `graphic_tests/07-box-sizing.html` (пайплайн, 11.23%)
**Скриншот:** `graphic_tests/screenshots/07-*.png` *(не в репо)*

**Описание:**
Для `box-sizing: content-box` CSS-спека определяет: `outer_width = width + padding_l + padding_r + border_l + border_r`. В Lumen border не добавляется к внешнему размеру, из-за чего `content-box` боксы компактнее Edge на `2 × border_width` по каждому измерению.

Это вызывает смещение боксов ниже по странице (суммируется на каждом элементе), что даёт визуальную дельту 11.23%.

**Воспроизведение:**
```html
<div style="width:200px; height:48px; padding:8px; border:8px solid gray; box-sizing:content-box;"></div>
<!-- Ожидается outer width = 200+16+16 = 232px, outer height = 80px -->
<!-- Факт: Lumen рисует что-то меньше (border не учтён) -->
```

**Где смотреть:** `crates/engine/layout/src/box_tree.rs` — функция `lay_out()` / вычисление `rect.width` / `rect.height` для `content-box`. Искать место где `border_*_width` прибавляется к content-width/height (для `content-box` должно, для `border-box` — нет).

---

### BUG-025 · `max-height` не зажимает высоту блока

**Статус:** OPEN
**Компонент:** `lumen-layout` — block height clamping
**Обнаружен:** 2026-05-19 на `graphic_tests/11-min-max-height.html` (пайплайн, 15.64%)
**Скриншот:** `graphic_tests/screenshots/11-*.png` *(не в репо)*

**Описание:**
При `height: 160px; max-height: 80px` блок должен отрисовываться высотой 80px. В Lumen — 160px (max-height игнорируется). Симметричная проблема с `min-height` также возможна, но менее явна из-за совпадения визуального эффекта.

**Воспроизведение:**
```html
<div style="height: 160px; max-height: 80px; background: green;"></div>
<!-- Edge: box 80px high. Lumen: box 160px high -->
```

**Где смотреть:** `crates/engine/layout/src/box_tree.rs` — после вычисления `height` в `lay_out()`, найти где применяются `min_height` / `max_height` (через `resolve_or_zero`). Проверить что `max_height` из `ComputedStyle` корректно разрешается и применяется как `height = height.min(max_h)`.

---

### BUG-026 · `<img>` не масштабируется по CSS/HTML `width`/`height`

**Статус:** OPEN
**Компонент:** `lumen-layout` / `lumen-paint`
**Обнаружен:** 2026-05-19 на `graphic_tests/18-images.html` (пайплайн, 10.29%)
**Скриншот:** `graphic_tests/screenshots/18-*.png` *(не в репо)*

**Описание:**
`<img width="200" height="150">` и `<img style="width:300px; height:225px;">` рендерятся меньше чем в Edge — предположительно по натуральному размеру файла, без CSS-масштабирования. В Edge изображения отображаются в указанных CSS/HTML-размерах.

**Воспроизведение:**
```html
<img src="photo.png" width="300" height="225" alt="">
<!-- Edge: 300×225. Lumen: меньше (натуральный размер файла?) -->
```

**Где смотреть:** `crates/engine/layout/src/box_tree.rs` — presentational hints `<img width|height>` (уже частично есть) и CSS `width`/`height` на `<img>`-элементе. Также `lumen-paint` — команда `DrawImage` должна использовать layout-rect, не натуральный размер текстуры.

---

### BUG-027 · Block-элемент не уважает явный `width` — тело страницы растягивается до viewport

**Статус:** OPEN
**Компонент:** `lumen-layout` — block width computation [P1]
**Обнаружен:** 2026-05-19, прогон 3 (после relayout-on-resize, см. BUG-028)
**Скриншот:** `graphic_tests/screenshots/02-lumen.png` *(не в репо)*

**Описание:**
В CSS `width: 1024px` на `<body>` должен ограничивать ширину body ровно 1024px независимо от viewport. В Lumen block-элемент всегда берёт 100% ширины содержащего блока. Пока viewport был жёстко 1024px, баг не проявлялся. После релауота с viewport = физический размер максимизированного окна (~1920px) body растянулся до 1920px.

**Доказательство:** В `graphic_tests/screenshots/02-lumen.png` видно окно ~1920×820. Body занимает всю ширину — 12 боксов шириной 140px в один ряд (12×156=1872=1920-2×24). В Edge те же 18 боксов — 6 в ряд (body=1024px, content=976px, 6×156=936<976).

**Воспроизведение:**
```html
<body style="width: 400px; background: red;">
  <!-- Ожидается: красная полоска 400px шириной -->
  <!-- Факт при viewport > 400px: красный фон на полную ширину viewport -->
</body>
```

**Где смотреть:** `crates/engine/layout/src/box_tree.rs` — вычисление ширины block-box. После resolve_length для `width` следует проверить: если `width` задан явно (не `auto`), использовать это значение; если `auto` — использовать available_width из containing block. Сейчас `auto` и `<length>` обрабатываются одинаково (берётся available_width).

**Масштаб:** Все тесты 02–06, 08, 12, 15–16, 18–19, 21 регрессировали из-за этого бага в прогоне 3.

---

### BUG-028 · `relayout-on-resize` + `.with_maximized(true)` ломает все graphic tests

**Статус:** OPEN
**Компонент:** `lumen-shell` — `Lumen::relayout()` [P3]
**Обнаружен:** 2026-05-19, прогон 3
**Связан:** BUG-027 (trigger)

**Описание:**
Коммит `e484d37` (3A.1 relayout-on-resize) добавил `self.relayout()` в `WindowEvent::Resized`. Окно открывается с `.with_maximized(true)`, поэтому при старте winit сразу стреляет `Resized` с размером максимизированного окна (~1920×1040). `relayout()` вызывает `r.viewport_size()` → новый viewport → пересчёт layout с неправильными размерами (см. BUG-027).

До этого коммита viewport всегда был 1024×720 (жёстко в `parse_and_layout`). Теперь он берётся из физического размера surface.

**Симптом:** В прогоне 3 регрессировали 12 тестов из 17 ранее проходивших (02–06, 08, 12, 15–16, 18–19, 21).

**Временный фикс [P3]:** Убрать `.with_maximized(true)` из `resumed()` — окно откроется 1024×720, resize не стреляет, relayout не запускается. Постоянный фикс — после исправления BUG-027 (layout уважает explicit width) relayout будет давать правильный результат.

**Где смотреть:** `crates/shell/src/main.rs:1033` — `.with_maximized(true)`.

---

## Исправленные баги

### BUG-001 · `display:none` на inline-элементах не скрывает контент

**Статус:** FIXED 2026-05-15 (как следствие BUG-003).  
Был частным проявлением BUG-003: `style="display:none"` через inline-атрибут не доходил до каскада. После подключения inline-style работает на любых элементах. Юнит-тест: `inline_style_display_none_hides_element`.

### BUG-003 · Атрибут `style=""` не обрабатывался каскадом

**Статус:** FIXED 2026-05-15 (ветка `inline-style-attr`).  
**Корневая причина:** `compute_style` собирал declarations только из `sheet.rules` + `sheet.media_rules` + presentational hints для `<img>`. Шага «прочитать `node.get_attr("style")`, спарсить, добавить в каскад» не было.

**Фикс:**
1. `lumen-css-parser::parse_inline_style(input)` — публичная функция, парсит declaration-list без `{}` (внутри использует существующий `parse_declaration_block`).
2. `compute_style` (`crates/engine/layout/src/style.rs:2449`) — читает `node.get_attr("style")`, парсит, добавляет в `matched`. Sort-key расширен новым bit `is_inline`: `(important, is_inline, specificity, rule_idx, decl_idx)` — внутри одного origin/importance inline всегда побеждает любой селектор (CSS Cascade L4 §6.4.3 «Element-Attached Styles»). `!important inline` побеждает `!important class`; `!important class` побеждает normal `inline`.
3. 7 юнит-тестов в `css-parser` + 8 в `layout`.

**Сразу починилось:** BUG-001 (см. выше), а также на test-01…06 — все inline `color`, `font-weight`, `font-style`, `text-decoration`, `background`, `border`, `padding`, `margin`, `width`, `height`, `display:none`, `text-align`, `line-height` и пр. начали работать. Подтверждено повторным дампом `--dump-display-list samples/test-06-layout.html`: фоны Блок 2/3 (`#f0fff4`, `#faf5ff`), цвета spans (`#e53e3e`, `#38a169`, `#3182ce`, `#805ad5`, `#d69e2e`), bold/italic, underline (FillRect) — всё в выводе.

**Остались независимые баги:** BUG-002 (inline-spans с padding/border ломают inline-flow — отдельная проблема inline layout), BUG-004 (`height` на inline без `display: inline-block`), BUG-013 (несколько `<span style="">` подряд без разделителя стакаются вертикально — отдельная проблема inline-сегментации).

**Прогон 2026-05-19 на `graphic_tests/02-04` и `09-12` показал:** пустые `<div style="background:red; width:140; height:80">` с `display: inline-block` рендерятся как невидимая или 0×0 область, поэтому inline-style backgrounds/widths визуально отсутствуют. Это **не регрессия BUG-003** — корень в Phase 0 ограничении `display:inline-block` (см. таблицу «Ограничения Phase 0»): div-ы с `display:inline-block` коллапсят до `inline` → пустой контент = 0×0 box, фон применяется к нулевой площади. Подтверждение: тесты 00/01 (без inline-block) проходят pixel-perfect (0.00% diff).

---

## Ограничения Phase 0 (не баги — запланировано позже)

| Фича | Фаза |
|---|---|
| `display:inline-block` | Phase 1 |
| `float` | Phase 1 |
| `position:absolute/fixed/relative` | Phase 1 |
| `flexbox` (`display:flex`) | Phase 1 |
| `grid` | Phase 2 |
| `border-radius` | Phase 1 |
| `box-shadow` | Phase 1 |
| CSS-градиенты | Phase 1 |
| CSS-анимации | Phase 2 |
| Table layout | Phase 1 |
| HiDPI / DPR-масштабирование | Phase 1 |

---

## Прогон 2026-05-19 (graphic_tests pipeline)

Первый прогон через `python graphic_tests/run.py` после внедрения магента-маркера и динамической калибровки. Пайплайн блокирующий: первый fail = стоп.

```
[00] PASS    0.00%   calibration
[01] PASS    0.00%   sanity
[02] FAIL   29.30%   color-named     ← пайплайн ОСТАНОВЛЕН (порог 5%)
Пропущено 18 тестов.
```

**Что подтвердилось:**
- Базовая геометрия Lumen pixel-perfect (тесты 00/01 → 0.00% diff). Прошлый «сдвиг квадрата на 8/16 px» был артефактом жёсткого crop offset, не Lumen.
- Магента-маркер (block + width/height + background-color из class CSS) рендерится корректно.

**Что блокирует продолжение:**
- BUG-001/BUG-002/BUG-013 (inline-spans/inline-style) + Phase 1 ограничение на `display:inline-block` — тест 02 (color-named) состоит из 18 пустых div-ов с `display:inline-block` и inline-style backgrounds, ни один не виден. Пока не реализован `display:inline-block` или не предложен альтернативный тестовый стиль (например, `display:block` + явный `width`/`margin`), большинство тестов 02–20 будет fail-иться.

**Новые баги, обнаруженные этим прогоном:** BUG-020 (overflow), BUG-021 (HTML bgcolor attr), BUG-022 (hashless hex colors). Подтверждены: BUG-006 (table layout) на тесте 20, BUG-019 (outline) на тесте 16.

---

## Прогон 2026-05-19 (второй, --continue-on-fail, полный)

После влития `length-type-cascade` (P1) и фикса `fix-paint-length-type` (paint сборка).
Пайплайн `--continue-on-fail` — все 22 теста, без остановки.

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: PASS  1.95%   color-named
TEST-03: PASS  1.95%   color-formats
TEST-04: PASS  1.95%   color-alpha
TEST-05: PASS  3.52%   border-width
TEST-06: PASS  4.84%   border-sides
TEST-07: FAIL 11.23%   box-sizing        ← BUG-024
TEST-08: PASS  3.52%   padding
TEST-09: PASS  1.95%   margin
TEST-10: PASS  3.52%   min-max-width
TEST-11: FAIL 15.64%   min-max-height    ← BUG-025
TEST-12: PASS  1.95%   display
TEST-13: FAIL 13.75%   visibility-opacity← BUG-023 (opacity)
TEST-14: FAIL 10.07%   overflow          ← BUG-020 (known)
TEST-15: PASS  3.01%   box-shadow
TEST-16: PASS  3.52%   outline
TEST-17: PASS  3.52%   calc
TEST-18: FAIL 10.29%   images            ← BUG-026
TEST-19: PASS  4.98%   object-fit
TEST-20: FAIL 30.56%   quirks-bgcolor    ← BUG-006/021/022 (known)
TEST-21: FAIL  5.01%   border-style      ← BUG-016 partial (double still wrong)
```

**Что прошло vs прошлого прогона:** тест 02 и другие (02–06, 08–10, 12, 15–17, 19) — PASS, потому что тесты были переписаны с `display:inline-block` на `display:block` (worktrees P1 ранее).

**Примечание:** в `02-color-named.html` и ряде других по факту осталось `display: inline-block`. Тесты проходили случайно — viewport=1024px совпадал с explicit `body { width: 1024px }` (скрытый BUG-027). После `relayout-on-resize` (прогон 3) баг проявился.

**Новые баги:** BUG-023 (opacity), BUG-024 (box-sizing), BUG-025 (max-height), BUG-026 (img scaling).

**Обнаруженный build-break:** После слияния `length-type-cascade` у `lumen-paint` не компилировались 5 мест в `display_list.rs` (padding/outline_offset → Length, арифметика с f32). Исправлено веткой `fix-paint-length-type`, влито в main.

---

## Прогон 2026-05-19 (третий, --continue-on-fail, после relayout-on-resize)

После влития `relayout-p3-clean` (P3, 3A.1 relayout-on-resize). Пайплайн `--continue-on-fail`.

```
TEST-00: PASS  0.00%   calibration
TEST-01: PASS  0.00%   sanity
TEST-02: FAIL 22.04%   color-named         ← РЕГРЕССИЯ (был 1.95%) — BUG-027/028
TEST-03: FAIL 32.12%   color-formats       ← РЕГРЕССИЯ (был 1.95%) — BUG-027/028
TEST-04: FAIL 15.67%   color-alpha         ← РЕГРЕССИЯ (был 1.95%) — BUG-027/028
TEST-05: FAIL 13.67%   border-width        ← РЕГРЕССИЯ (был 3.52%) — BUG-027/028
TEST-06: FAIL 23.12%   border-sides        ← РЕГРЕССИЯ (был 4.84%) — BUG-027/028
TEST-07: FAIL  8.60%   box-sizing          ← BUG-024 (улучшение с 11.23%)
TEST-08: FAIL 11.35%   padding             ← РЕГРЕССИЯ (был 3.52%) — BUG-027/028
TEST-09: PASS  1.95%   margin
TEST-10: PASS  3.52%   min-max-width
TEST-11: FAIL 15.90%   min-max-height      ← BUG-025 (≈ то же)
TEST-12: FAIL 13.76%   display             ← РЕГРЕССИЯ (был 1.95%) — BUG-027/028
TEST-13: FAIL 13.67%   visibility-opacity  ← BUG-023 частично исправлен (был 13.75%)
TEST-14: FAIL 20.39%   overflow            ← BUG-020 + BUG-027 (хуже: был 10.07%)
TEST-15: FAIL  6.44%   box-shadow          ← РЕГРЕССИЯ (был 3.01%) — BUG-027/028
TEST-16: FAIL 20.37%   outline             ← РЕГРЕССИЯ (был 3.52%) — BUG-027/028
TEST-17: PASS  3.52%   calc
TEST-18: FAIL 31.73%   images              ← BUG-026 + BUG-027 (хуже: был 10.29%)
TEST-19: FAIL 22.53%   object-fit          ← РЕГРЕССИЯ (был 4.98%) — BUG-027/028
TEST-20: FAIL 30.62%   quirks-bgcolor      ← BUG-006/021/022 (≈ то же)
TEST-21: FAIL 19.07%   border-style        ← BUG-016 + BUG-027 (хуже: был 5.01%)
```

**Корень проблемы:** BUG-027 + BUG-028. Окно открывается максимизированным (~1920×1040), relayout пересчитывает layout с viewport ~1920px. В Lumen block-элемент игнорирует explicit `width` в px и берёт 100% от viewport → body становится 1920px вместо 1024px. Все тесты с explicit body width рассыпаются.

**Что не затронуто:** TEST-09 (margin), TEST-10 (min-max-width), TEST-17 (calc) — проходят, т.к. их layout не зависит от body width.

**Новые баги:** BUG-027 (layout: explicit block width ignored), BUG-028 (shell: relayout-on-resize + maximize trigger).

**Build-break при pull:** `DrawLayerSnapshot` вариант не был покрыт в `content_height_of()` в `main.rs`. Исправлено одной строкой в `main.rs:1652`.
