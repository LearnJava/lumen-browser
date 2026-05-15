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

### BUG-003 · `background` из атрибута `style=""` не применяется

**Статус:** OPEN  
**Компонент:** `lumen-layout` / `lumen-paint` — применение inline-стилей  
**Обнаружен:** 2026-05-15, уточнён 2026-05-15 по скрину Lumen vs Chrome  
**Скриншот:** `bugs/screenshots/bug-003-inline-bg-vs-chrome.png` *(добавить)*

**Описание:**  
`background` / `background-color`, заданные через атрибут `style=""`, не применяются —
ни на inline `<span>`, ни на блочных `<div>`.  
При этом тот же `background` в CSS-классе (`<style>` блок) **работает**.

Сравнение на `test-03-colors.html`:
- Шапка `.hdr { background: #1e3a5f }` (CSS-класс) → тёмно-синий фон ✅
- `<div style="background: red;">` (inline style) → фон не рисуется ❌
- `<div style="background: #e53e3e;">` (hex, inline style) → фон не рисуется ❌
- `<div style="background: rgb(220,50,50);">` (rgb, inline style) → фон не рисуется ❌

**Воспроизведение:**
```html
<style>
  .from-class { background: green; }   /* работает */
</style>
<div class="from-class">зелёный</div>
<div style="background: red;">красный?</div>   <!-- фон не рисуется -->
```
**Ожидается:** оба div с фоном.  
**Факт:** только div с CSS-классом получает фон; inline style игнорируется.

**Где смотреть:** `crates/engine/layout/src/style.rs` или аналог —
применение inline `style=""` атрибута при cascade/computed values.
Вероятно, inline-стили не попадают в вычисленные свойства или игнорируются при FillRect.

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
