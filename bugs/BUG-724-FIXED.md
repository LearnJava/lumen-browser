# BUG-724: пустое значение border-шортката роняет поток вёрстки

**Статус:** FIXED 2026-08-09
**Компонент:** layout (`crates/engine/layout/src/style.rs` — `expand_border_4`)
**Найден:** P3 при разборе [BUG-703](BUG-703-FIXED.md), 2026-08-09

## Симптом

```
thread 'lumen-engine' panicked at crates\engine\layout\src\style.rs:22443:26:
index out of bounds: the len is 0 but the index is 0

thread 'main' panicked at crates\shell\src\main.rs:13494:46:
called `Result::unwrap()` on an `Err` value: PoisonError { .. }
```

Одна невалидная декларация убивает окно целиком: падает поток вёрстки, а следом
`main` — на отравленном мьютексе общего документа.

## Причина

`expand_border_4` разворачивает 1-4 токена в четвёрку (T,R,B,L). Ветка `_`
(«4 и больше») индексировала `parts[0]`, но получала управление и при
`parts.len() == 0` — то есть на пустом или состоящем из пробелов значении
(`border-radius: ;`).

Такое значение — не экзотика: React-приложения пишут пустышки в inline-стиль
пачками для «неустановленных» пропсов. На `tbank.ru` это буквально каждый
компонент:

```html
<div style="width: 212px; height: ; background-color: ; background-image: ; color: ; cursor: ">
```

Панику не видели раньше только потому, что до [BUG-486](BUG-486-FIXED.md)
страница не доходила до рендера собственного контента.

## Фикс

Явная ветка `0 => [val; 4]`: четыре пустых токена, которые не парсит ни один из
потребителей (`resolve_box_length`, `parse_css_color_legacy`,
`parse_radius_length` возвращают `None`, `parse_border_style_kw` — `None`-стиль,
ровно как на любом другом нераспознанном ключевом слове). Невалидное объявление
игнорируется, как и требует CSS, вместо падения.

Тест: `style::tests::empty_border_shorthand_is_ignored_not_panic` — покрывает и
`expand_border_4("")`/`("   ")` напрямую, и сквозной `compute_style` на
`style="border-radius: ; border-width: ; border-color: ;"`.

## Смежное

Класс «невалидное значение свойства роняет движок вместо игнорирования» стоит
проверить и в остальных разворачивающих хелперах `style.rs` — здесь он найден
случайно, живой страницей, а не аудитом.
