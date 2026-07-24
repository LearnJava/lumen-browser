# BUG-333: `height: var(--custom-prop)` резолвится в 0 на flex-строках хрома (CC-1 смоук)

**Статус:** OPEN
**Компонент:** layout (флекс-резолюция `height` через `var()`, вероятно `crates/engine/layout/src/box_tree.rs`)
**Найден:** P1, CC-1 (`docs/tasks/p1-css-chrome.md`), скрин-сверка `about:chrome-preview` с эталоном `docs/design/lumen-v3_3.html` 2026-07-24

## Симптом

В сайдбаре хрома (`.sb-tabs` — список вкладок под `.sb-workspaces`) все строки `.tab-row`
рендерятся с нулевой высотой: текст заголовков соседних вкладок налезает друг на друга
(скриншот `about:chrome-preview`, область сайдбара y≈180–220 при viewport 1024×720).

`--dump-layout about:chrome-preview` подтверждает — каждая `.tab-row` имеет `h=0.00`:

```
Block rect=(8.00, 191.92, 223.00, 0.00) bg=#f0f2f5ff position=relative display=flex h=0.00 ...
```

при CSS-правиле:

```css
:root{ --tab-h:28px; --toolbar-h:36px; }
.tab-row{
  display:flex; align-items:center; gap:6px; height:var(--tab-h); padding:0 6px 0 8px;
  border-radius:var(--radius-sm); position:relative; border-left:2px solid transparent; cursor:pointer;
}
```

Контрольный пример на той же странице — `.toolbar{ height:var(--toolbar-h); ... }` — резолвится
**корректно** (`h=36.00` в дампе), т.е. `var()` в `height` в принципе работает; ломается только
в контексте `.tab-row`.

## Воспроизведение

1. `cargo run -p lumen-shell -- --dump-layout about:chrome-preview` (ветка `p1-cc-1-chrome-assets-smoke`,
   нужен `assets/chrome/chrome.html`, генерируется `scripts/gen_chrome_assets.py`).
2. В выводе найти блоки `bg=#f0f2f5ff position=relative display=flex h=0.00 ... bs=(none,none,none,solid)`
   в секции сайдбара (после `ВКЛАДКИ · ЛИЧНОЕ`) — это `.tab-row`, все с `h=0.00`.

Изолированный минимальный репро **не собрался** за разумное время диагностики — три попытки:

- Тот же CSS для `.tab-row`/`.sb-tabs`/`.sidebar` (без остальных ~530 строк файла) на 3 строках —
  **работает корректно** (`h=28.00`).
- Реальная разметка (`sed -n '628,693p' assets/chrome/chrome.html`, все 10 `.tab-row` с
  `data-ws`/`.active`/`.child`/`.sleeping`/`.tree-line`) + написанный вручную аналогичный CSS —
  **тоже работает корректно**.
- CSS-строки 13–140 исходного `assets/chrome/chrome.html` (`:root` + база `body` + `.sidebar`/
  `.sb-profile`/`.sb-workspaces`/`.tab-row` включительно) + **дословная** разметка `<aside
  class="sidebar">` (строки 589–702) — **воспроизводится** (`h=0.00`).
- Попытка добавить к упрощённому репро «дробный остаток» соседей `flex:1` (имитация того, что
  `.sb-tabs` в реальной странице получает `flex:1` от родителя высотой `511.08px`, не целым
  числом, из-за предшествующих `.sb-profile`/`.sb-divider`/`.sb-workspaces`) — **не** воспроизвела
  баг в одиночку.

Т.е. триггер требует полного каскада (не одного правила `.tab-row` — что-то в CSS-строках 13–140
в сочетании с точной вложенностью реальной разметки `.sidebar > .sb-profile + .sb-divider +
.sb-workspaces + .sb-tabs`). Самый надёжный воспроизводимый кейс на сегодня — дословное извлечение
`sed -n '1,12p;13,140p;589,702p' assets/chrome/chrome.html` в отдельный `<html>` (обёрнутый
`</style></head><body>...</body></html>`).

## Не диагностировано

Точная строка/функция в `box_tree.rs`, отвечающая за резолюцию `height` для flex-item, не
локализована — вне бюджета времени CC-1 (задача — смоук-каталог расхождений, не фикс движка).
Кандидат для расследования: резолюция `height:<length>` (через `var()`) на flex-item, чей
flex-контейнер (`.sb-tabs`) сам получает дробный `flex:1`-остаток (`511.08px`) от **своего**
родителя (`.sidebar`, `display:flex; flex-direction:column`) — возможно, взаимодействие
дробного available-space с явной `height` дочернего flex-item несколько уровней глубины.

## Что закрыло бы

Правильная резолюция `height: var(--x)` (для любых `.tab-row`) → строки не должны схлопываться;
после фикса — визуальная проверка `about:chrome-preview` (сайдбар со списком вкладок должен
совпадать с эталоном `docs/design/lumen-v3_3.html`).
