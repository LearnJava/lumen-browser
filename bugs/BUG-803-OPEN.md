# BUG-803 — `parse_svg_transform`: бесконечный цикл на любом неалфавитном символе в позиции имени и паника на завершающей запятой

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 11 — разбор коллатерального `no-output`)
**Область:** `crates/engine/layout/src/box_tree.rs:1341-1400` (`parse_svg_transform`); вызывается из `:1680` и `:1803` для атрибута `transform` любого SVG-узла
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом 1 — вечный цикл (одна строка HTML)

```bash
echo '<svg width="300" height="200"><rect transform="FAIL_ME(30)" width="10" height="10"/></svg>' > /tmp/t.html
timeout 60 lumen --dump-layout /tmp/t.html    # rc=124, вывода нет, CPU 100 %
```

Виснет layout, поэтому живое окно на такой странице тоже застынет насмерть.
Не «медленно» — выхода из цикла нет.

## Симптом 2 — паника (index out of bounds)

```bash
echo '<svg width="300" height="200"><rect transform="rotate(30)," width="10" height="10"/></svg>' > /tmp/p.html
lumen --dump-layout /tmp/p.html
# thread 'main' panicked at crates/engine/layout/src/box_tree.rs:1359:86:
# index out of bounds: the len is 11 but the index is 11
```

## Причина (локализована чтением кода, оба симптома — три строки)

```rust
1359:  while pos < attr_bytes.len() && (attr_bytes[pos] as char).is_whitespace() || attr_bytes[pos] == b',' {
1360:      pos += 1;
1361:  }
...
1369:  while pos < attr_bytes.len() && (attr_bytes[pos] as char).is_alphabetic() {
1370:      pos += 1;
1371:  }
...
1380:  if pos >= attr_bytes.len() || attr_bytes[pos] != b'(' {
1381:      continue;
1382:  }
```

**Цикл.** Имя функции набирается только из `is_alphabetic()`. Встретив
символ, который не буква и не `(` — `_`, `;`, `|`, цифру — сканер на 1369
не двигается, на 1380 видит «не `(`» и уходит в `continue` **не изменив
`pos`**. Внешний `while pos < len` (1357) заходит на ту же позицию снова.
Выхода нет.

Затрагивает не только мусорные значения: `is_alphabetic()` обрывает имя на
цифре, поэтому **любая transform-функция с цифрой в имени** вешает парсер —
`translate3d(...)`, `matrix3d(...)`, `rotate3d(...)`, `scale3d(...)`. Из
проверенных значений: виснут `FAIL_ME(30)`, `rotate(30deg)|rotateX(60deg)`,
`translate3d(0.48px, 0px, 0px)`, `matrix3d(2,0,0,0, …)`; не виснут `bogus`
(имя съедает всю строку, `pos` доходит до конца) и `rotate(30)`.

**Паника.** На 1359 `&&` связывает сильнее `||`, поэтому условие читается как
`(pos < len && ws) || attr_bytes[pos] == b','`. Когда `pos == len`, первый
дизъюнкт ложен, и второй **индексирует за концом среза**. Достаточно, чтобы
значение атрибута кончалось запятой (`transform=","` — минимальный репро,
`transform="rotate(30),"` — реалистичный). Правильная форма —
`pos < len && (ws || comma)`.

Оба дефекта чинятся вместе: скобки на 1359 и «если после имени нет `(` —
пропустить один байт», а не `continue` без прогресса.

## Цена (WPT-корпус, снимок 2026-08-20 Linux, 479/479 шардов)

**133 TIMEOUT-вердикта от одной страницы.** Процесс браузера в шарде
`css__css-transforms` (pid 501484) повис на
`css/css-transforms/2d-rotate-notref.html` (в её SVG стоит `transform="FAIL_ME(30)"`
— заведомо невалидное значение, тест ровно про то, что оно игнорируется), и
каждый следующий тест шарда — 133 штуки — получил TIMEOUT по таймауту сокета
на мёртвом процессе. Это самая дорогая одиночная страница снимка
(классификация коллатерали — `tests/wpt/timeout_audit.py`, механизм
`hung-browser`).

Прямой охват по корпусу — **один файл**: скан 932 файлов, содержащих `<svg>`
и слово `transform`, дал ровно две подозрительные записи, и вторая
(`svg/import/coords-transformattr-01-f-manual.svg`,
`transform="translate(50 50)&#x0020;rotate(45)…"`) при проверке НЕ виснет —
сущности декодируются до парсера, `&` до него не доходит. Значения вида
`translate3d(…)`/`matrix3d(…)`, найденные сканом в
`css/css-transforms/*`, тоже мимо: это присваивания `style.transform` из JS,
они идут через другой парсер (`style.rs::parse_transform_list`, он на цифру в
имени не спотыкается). Цена одной страницы измеряется, таким образом, не её
собственным вердиктом, а хвостом шарда — 1 файл против 133 вердиктов.

Для живых сайтов важнее устойчивость: невалидный `transform` на SVG-узле по
спецификации должен игнорироваться, а вешает вкладку насмерть.

## Как проверить фикс

```bash
timeout 30 lumen --dump-layout /tmp/t.html   # rc=0
lumen --dump-layout /tmp/p.html              # без паники
timeout 30 lumen --dump-layout tests/wpt/css/css-transforms/2d-rotate-notref.html
```

Юнит-тесты на сам `parse_svg_transform` (в `box_tree.rs` рядом с остальными
layout-тестами): `FAIL_ME(30)`, `translate3d(1px,2px,3px)`, `rotate(30)`,
`rotate(30),`, `""`, `,`, `none` — все должны вернуть управление, валидные
значения при этом разобраться.
