# BUGS.md — Баг-трекер Lumen Browser

Этот файл — живой список известных багов и ограничений движка.  
Читается Claude автоматически через CLAUDE.md (см. ниже как добавлять баги).

**Как добавить баг:**
1. Скопируй скриншот в `bugs/screenshots/bug-NNN-краткое-имя.png`
2. Добавь запись ниже в нужную секцию
3. Сообщи Claude: «посмотри BUGS.md, возьми баг-NNN»

**Статусы:** `OPEN` · `IN PROGRESS` · `FIXED` · `WONTFIX (Phase N+)`

---

## Открытые баги

### BUG-001 · `display:none` на inline-элементах не скрывает контент

**Статус:** OPEN  
**Компонент:** `lumen-layout` — обработка `display:none`  
**Обнаружен:** 2026-05-15 (тестовые страницы Phase 0)  
**Скриншот:** `bugs/screenshots/` *(нет)*

**Описание:**  
`display:none` на `<span>` (inline-элемент) не скрывает содержимое — текст остаётся видимым.  
На блочных `<div>` работает корректно.

**Воспроизведение:**
```html
<p>
  Видимый текст.
  <span style="display: none;">ЭТОТ ТЕКСТ ДОЛЖЕН БЫТЬ СКРЫТ</span>
  Снова видимый.
</p>
```
**Ожидается:** span не виден, второй текст идёт сразу после первого.  
**Факт:** span виден, `display:none` проигнорирован.

**Где смотреть:** `crates/engine/layout/src/` — вычисление `display` у inline-box.

---

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

### BUG-003 · Атрибут `style=""` не обрабатывается полностью (root cause для BUG-001 и многих визуальных проблем test-01…06)

**Статус:** OPEN — root cause  
**Компонент:** `lumen-layout::style::compute_style` — каскад не читает inline `style`-атрибут  
**Обнаружен:** 2026-05-15  
**Скриншот:** `bugs/screenshots/bug-003-chrome.png` + `bugs/screenshots/bug-003-lumen.png` *(сохранить вручную)*

**Описание (после уточнения 2026-05-15 по test-01…06):**  
Атрибут HTML `style="..."` не подключён к каскаду CSS. Ни одно свойство, заданное через inline-атрибут, не применяется:
`background`, `color`, `font-weight`, `font-style`, `text-decoration`, `text-transform`, `letter-spacing`, `word-spacing`,
`text-align`, `line-height`, `font-size`, `margin`, `padding`, `border`, `width`, `height`, `display`, `overflow`,
`object-fit`, `float`, `vertical-align`, `box-sizing` и т.д.  
При этом эти же свойства, заданные через CSS-классы или селекторы в `<style>` блоке, **работают**.

**Корневая причина:**  
`compute_style` (`crates/engine/layout/src/style.rs:2449`) собирает declarations только из `sheet.rules` + `sheet.media_rules` + `apply_image_presentational_hints` для `<img>`. Шага «прочитать `node.get_attr("style")`, спарсить declaration-list, добавить в каскад» нет вообще. Грэп по `crates/`: ни `get_attr("style")`, ни `parse_inline_style` не встречаются.  
По CSS Cascade L4 §6.4.3 inline `style` — отдельный origin со specificity (1,0,0,0), идущий сразу после `!important author`. Эта ветка не реализована.

**Воспроизведение (минимальное):**
```html
<style>.k { background: green; }</style>
<div class="k">зелёный</div>            <!-- работает -->
<div style="background: red;">красный?</div>   <!-- фон не рисуется -->
<span style="color: #e53e3e;">красный?</span>  <!-- цвет не меняется -->
```
**Ожидается:** оба div с фоном; span — красным.  
**Факт:** inline style полностью игнорируется.

**Производные симптомы (после фикса должны исчезнуть):**
- BUG-001 «`display:none` на span не скрывает» — это `style="display:none"` через атрибут.
- На test-01 — все секции font-weight/font-style/decoration/color/text-transform/letter-spacing/word-spacing/text-align/line-height/font-size инлайном не работают.
- На test-02 — никакая box-model (border, padding, margin, width, box-sizing) инлайном не применяется.
- На test-03 — ни одна цветная плашка `<div style="background: ...">` не получает фон.
- На test-04 — все `display: inline-block` (для горизонтальной раскладки картинок) превращаются в block, объекты стакаются вертикально; `object-fit` не применяется.
- На test-05 — `float: left` для трёх колонок списков не работает (`float` сам ограничен Phase 0, но через inline он бы и так не дошёл); `<hr style="...">` не получает inline border.
- На test-06 — все боксы без inline-фонов/бордеров/размеров, секция «наследование CSS» не показывает разных цветов.

**Что фиксить:**
1. В `lumen-css-parser` — публичная функция `parse_inline_style(input: &str) -> Vec<Declaration>` (парсит declaration-list без `{}`).
2. В `compute_style` после `apply_image_presentational_hints` (style.rs:2447) — прочитать `node.get_attr("style")`, спарсить, добавить в `matched` со specificity (1,0,0,0) и `rule_idx` после всех stylesheet-правил (чтобы при равной specificity inline побеждал).
3. Юнит-тест на cascade с inline-стилем (inline побеждает class).
4. После фикса — пересмотреть BUG-001 (станет следствием) и заметить в snapshot-тестах.

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

## Исправленные баги

*(пусто)*

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
