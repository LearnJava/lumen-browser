# BUG-1217 — `repeat(N, minmax(0, 1fr))` даёт нулевые треки: `grid-column: span K` схлопывается до K−1 промежутков

**Статус:** FIXED 2026-10-07
**Компонент:** layout (grid track sizing — разбор/резолв `minmax(0, 1fr)` внутри `repeat()`)
**Найден:** 2026-09-29, живая проверка локального стенда bankruptcy-platform (Next.js + Tailwind), сборка `main` 22a782d55

## Симптом

Tailwind пишет `grid-cols-N` как `grid-template-columns: repeat(N, minmax(0, 1fr))`. В Lumen все треки получают ширину 0,
элемент со `span K` занимает ровно `(K−1) × gap`. На дашборде `grid grid-cols-12 gap-6` (контейнер 1612 px):
`col-span-7` → **144 px** (=6×24), `col-span-5` → **96 px** (=4×24) вместо ~930 и ~660 px. Блок «Ближайшие заседания»
сжат в полосу, «Мои задачи» обрезан. Та же причина, вероятно, у `/documents` (три плитки налезают друг на друга) и
части `/profile`. Затронут любой сайт на Tailwind.

## Минимальный repro (без JS, без сети)

```html
<!doctype html><style>
.g{display:grid;grid-template-columns:repeat(12,minmax(0,1fr));gap:24px;width:1612px}
.p{grid-column:span 7 / span 7}
</style><div class="g"><div class="p">C</div></div>
```

`lumen --viewport 1920x945 --dump-layout repro.html` → ширина `.p` = `144.00`.

Контроль: тот же файл с `repeat(12, 1fr)` даёт `930.33` (верно). Значит, ломает именно `minmax(0, 1fr)`.
Форма записи `grid-column` не влияет: `span 7`, `span 7 / span 7`, `1 / span 7`, `span 7 / auto` дают 144.

## Ожидание

Треки `minmax(0, 1fr)` делят свободное место поровну (для `repeat(12, …)` при 1612 px и gap 24 — по ≈112.3 px),
`span 7` = 7×112.3 + 6×24 ≈ 930.

## Замечание

`getComputedStyle(grid)` в живой странице отдаёт пустые `gap` и `gridTemplateColumns` — отдельная проблема рядом:
при починке проверить, что резолвленные значения возвращаются.

## Исправление

В `grid.rs` колонки `minmax(<min>, Nfr)` не считались flex-треками (размер = min = 0). Добавлен `GridTrackSize::flex_factor()` и `find_fr_size` (CSS Grid L1 §12.7.1): flex-треки делят остаток, трек с долей ниже базы замораживается. Строки (rows) не затронуты. Остаток: `getComputedStyle(grid).gap/gridTemplateColumns` пустые — отдельно проверить.
