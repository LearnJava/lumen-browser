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

### BUG-003 · `background-color` на inline-элементах не отрисовывается

**Статус:** OPEN  
**Компонент:** `lumen-paint` — FillRect для inline-box  
**Обнаружен:** 2026-05-15  
**Скриншот:** `bugs/screenshots/` *(нет)*

**Описание:**  
`background-color` или `background` на `<span>` (inline) не рисуется.  
На блочных `<div>` — работает.

**Воспроизведение:**
```html
<span style="background: yellow;">жёлтый фон?</span>
```
**Ожидается:** жёлтый фон за текстом.  
**Факт:** фон не рисуется, текст без фона.

**Где смотреть:** `crates/engine/paint/src/` — рендеринг inline-box background.

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
