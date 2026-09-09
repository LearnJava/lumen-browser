# BUG-926 — `<button>` и `<select>` без явной ширины схлопываются в 0 и не видны на странице

**Статус:** FIXED (P6, 2026-09-10, дорожка E2E — задача E2E-4, итерация 4)
**Заведён:** 2026-08-25 (P1, попутно к задаче IFC-1)
**Область:** layout (`crates/engine/layout/src/box_tree.rs` — `is_replaced` в `lay_out_inner`, затем `b.rect.width = pref_w.min(b.rect.width)` в ветке shrink-to-fit)
**Владелец:** P3

## Симптом

`<button>Btn</button>` и `<select><option>Opt</option></select>` без CSS-ширины
получают `width = 0`. Подпись есть в дереве боксов, но её `InlineRun` тоже
нулевой ширины, так что на экране от контрола остаётся вертикальная полоска из
двух рамок.

Замер `--dump-layout` (dev-release, 2026-08-25), страница
`<div><button>Btn</button></div><div><select><option>Opt</option></select></div><div><input size=4></div>`:

```
FormControl rect=(0.00,  0.00, 0.00, 23.00) bg=#efefefff display=inline-block h=21.00 bw=(1,1,1,1)
  InlineRun rect=(1.00, 1.00, 0.00, 19.20)          <- подпись «Btn», ширина 0
FormControl rect=(0.00, 23.00, 0.00, 23.00) bg=#ffffffff display=inline-block h=21.00 bw=(1,1,1,1)
  Skip      rect=(1.00, 24.00, 0.00,  0.00) display=none
FormControl rect=(0.00, 46.00, 176.00, 23.00) w=174.00 h=21.00   <- <input>: ширина есть
```

Контроль на той же странице: `<span style="display:inline-block">Btn</span>`
даёт `width = 23.12` — shrink-to-fit сам по себе работает, ломается он именно у
`BoxKind::FormControl`. `<input>` не задет, потому что UA-стиль выводит ему
явную `width` из атрибута `size` (в дампе `w=174.00`).

## Механизм

Две строки `lay_out_inner`, каждая по отдельности осмысленная:

1. `is_replaced` относит `BoxKind::FormControl` к замещаемым элементам
   (CSS 2.1 §10.3.2 — «auto-ширина = intrinsic, а не вся ширина контейнера»),
   и `b.rect.width` получает intrinsic-ширину, то есть **0**: у формы нет
   декодированных пикселей, из которых её взять.
2. Ниже ветка shrink-to-fit для atomic inline-level боксов пишет
   `b.rect.width = pref_w.min(b.rect.width)`. `preferred_inline_block_width`
   считает по подписи правильную ширину, но `min` с нулём из шага 1 её
   обнуляет.

То есть shrink-to-fit у формы вычисляется и тут же выбрасывается. `min` здесь
нужен для настоящих замещаемых элементов (картинка не должна раздуться шире
предложенного места), а у формы предыдущее значение — не «предложенное место»,
а нулевой intrinsic.

Это третий заход на ту же строку. [BUG-425](BUG-425-FIXED.md) (2026-07-31)
исключил из `is_replaced` формы с авторским `display: flex/grid` — ровно потому,
что они схлопывались в 0; UA-дефолт `inline-block` тогда не тронули.

## Что проверить при починке

- `<input type=checkbox|radio|range|color|file>` — у них intrinsic-размер
  действительно свой (виджет), подпись отсутствует, и `preferred_inline_block_width`
  вернёт по ним 0: они должны остаться на старом пути, иначе схлопнутся уже они.
- `<input type=submit|reset|button>` — подпись рисует
  `emit_input_value_text` из `FormControlKind::Input.value_text`, а не дочерний
  бокс, так что ширину по ней `preferred_inline_block_width` не увидит; нужна
  отдельная ветка (ср. `field_sizing_content_intrinsic`, который уже так делает
  для `field-sizing: content`).
- `<select>` — подпись тоже не бокс, а `FormControlKind::Select { selected_text }`
  (`<option>` по UA-стилю `display: none`), плюс место под стрелку.
- Правка двигает пиксели: полный графический прогон + регенерация CPU-снапшотов
  в том же коммите. Затронуты как минимум `34-forms` и любая страница с кнопкой.

## Починка (P6, 2026-09-10)

Ни одна из двух строк «Механизма» не убрана — обе по-прежнему верны для
настоящих замещаемых элементов. Добавлена третья ветка между ними: если
`is_replaced`-бокс — это form control, чья ширина выводится из содержимого,
то вместо intrinsic-нуля берётся эта выведенная ширина, а `min` ниже уже
не имеет нуля, который мог бы её съесть.

* `form_control_fit_content_width` (`box_tree/intrinsic.rs`) — новая функция,
  `Option<f32>`; `None` означает «этот контрол остаётся на старом пути».
  * `<button>` — обычный shrink-to-fit поддерева (`preferred_inline_block_width`,
    подпись живёт в дочернем `InlineRun`); пустая кнопка получает ширину
    собственных padding + border, а не ноль.
  * `<select>`/`<selectlist>` — подпись не бокс вовсе (`<option>` по UA-стилю
    `display: none`), поэтому `selected_text` **измеряется** здесь, тем же
    шрифтом и теми же внутренними отступами, какими его рисует paint;
    `appearance: none` снимает стрелку в paint, значит и колонка под неё
    здесь не резервируется.
  * Все остальные `FormControlKind` — `None`. У checkbox/radio/range/progress/
    meter есть настоящий intrinsic-размер виджета и нет подписи, а текстовым
    контролам UA выдаёт явную `width` (174px) до того, как эта ветка вообще
    будет опрошена, — поэтому предупреждение «что проверить» ниже про
    `<input type=submit|reset|button>` оказалось неактуальным: у них тоже есть
    UA-ширина, и отдельная ветка им не понадобилась.
* Место вызова — `box_tree/layout_dispatch.rs`, ветка `b.rect.width = if is_replaced`.
  Результат клампится доступным inline-размером: `fit-content` — это
  `min(max-content, available)`, тем же правилом, что уже применяет
  `Length::FitContent` строкой ниже.
* Числа виджета `<select>` (внутренний отступ 4px, шрифт `clamp(10,14)`, ширина
  колонки стрелки) переехали из тела `emit_select_indicator` в константу и две
  функции в `box_tree/svg.rs`, откуда их читают ОБА крейта. Две копии одного
  числа — это ровно то, из-за чего измеренный бокс и нарисованный виджет могли
  бы разъехаться.

### Замер

`--dump-layout`, dev-release, та же страница, что в «Симптоме»:

| Элемент | До | После |
|---|---|---|
| `<button>Btn</button>` | `w=0.00` | `w=25.12` (23.12 подпись + 2 рамки; контроль `<span display:inline-block>` даёт 23.12) |
| `<select><option>Opt</option></select>` | `w=0.00` | `w=53.00` |
| `<input size=4>` | `w=176.00` | `w=176.00` — не задет |
| `<button style="padding:4px 12px">clicked 0</button>` | `w=0.00`, подпись по слову на строку | `w=84.20`, подпись одной строкой |

7 юнит-тестов в `box_tree/tests/intrinsic_and_wrap.rs` (`bug926_*`): подпись,
author-padding, явная `width` по-прежнему побеждает, кламп доступной шириной,
`<select>` с стрелкой и без неё, неизменность checkbox/`<input>`.

### Гейты

* `cargo test -p lumen-layout` 3950/3950, `-p lumen-paint` 1047/1047.
* `cargo clippy -p lumen-layout -p lumen-paint --all-targets -- -D warnings` чист.
* Полный `graphic_tests/run.py --continue-on-fail` — 13/156 FAIL, ровно те же
  вердикты, что у прогона до правки (2026-09-08), кроме двух улучшений не по
  этой ветке (TEST-06 FAIL→PASS, TEST-61 3.06% против baseline 10.59%).
  `34-forms` — единственная страница корпуса с `<button>`/`<select>` — не
  сдвинулась (DEBTOR 2.99% против baseline 3.02%), потому что задаёт им явную
  CSS-ширину.
* CPU-эталоны: 7 несовпадений **побайтово те же** с правкой и без неё
  (`git stash` A/B) — предсуществующий дрейф [BUG-1008](BUG-1008-OPEN.md),
  к этой ветке отношения не имеет, поэтому не перегенерированы.
* `dump_golden.py` — те же 4/12, что на `main`; ни на одной из 12 страниц
  набора нет form control вовсе.

### Что это разблокировало

Живая проба стенда [`samples/e2e4-hydration`](../samples/e2e4-hydration/README.md)
(React 18.3.1, живое окно, `--maximized`): `btn rect w=0` → `w=70.53`, MCP-клик
попадает **в саму кнопку** (`target=btn/BUTTON`, было `target=app/DIV`), React
зовёт `onClick`, счётчик растёт `clicked 0` → `clicked 1`. Попутно перестал
воспроизводиться и репро [BUG-1044](BUG-1044-OPEN.md): клик по инлайновой
`<button type=submit>` теперь отправляет форму (`⊢ form post /login
body=user=admin&pass=secret`, сервер получил `POST /login`) — измерение
дописано в его карточку, остаток бага сузился до «честного отказа».

## Связанное

- [BUG-425](BUG-425-FIXED.md) — тот же `is_replaced`, ветка `display: flex/grid`.
- Замер P6 2026-09-09 (E2E-4, стенд [`samples/e2e4-hydration`](../samples/e2e4-hydration/README.md)):
  на этом баге стоит интерактивность гидрированного React-приложения. Дамп
  `--dump-layout`: `<button>clicked 0</button>` → `FormControl rect w=0.00`,
  `padding:4px 12px` ширину не спасает (высота 23→31 растёт, ширина остаётся 0),
  подпись переносится по одному слову на строку; `<input type=submit>` рядом — 176px.
  Дальше по цепочке MCP-клик по такой кнопке уходит в родителя
  ([BUG-1044](BUG-1044-OPEN.md)): слушатель на самой кнопке молчит, а на `#app`,
  `#root` и `document` срабатывает с `target=app/DIV`, поэтому React-обработчик
  `onClick` не вызывается ни разу и приложение читается как «не гидрировалось».
- IFC-1 (ROADMAP.md) — базовая линия у форм; найден при её проверке. Выравнивание
  кнопки по строке уже правильное, видна она от этого не становится.
