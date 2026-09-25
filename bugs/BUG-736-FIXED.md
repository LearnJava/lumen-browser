# BUG-736: `<img>` как flex-элемент не ужимается по контейнеру и растягивается по поперечной оси

**Статус:** FIXED 2026-09-14
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs` — `lay_out_flex`,
размер flex-элемента)
**Найден:** P3 при разборе [BUG-733](BUG-733-FIXED.md), 2026-08-09

## Симптом

Картинка внутри flex-контейнера игнорирует и размер контейнера, и собственное
соотношение сторон: по главной оси вылезает наружу сырым intrinsic-размером, по
поперечной — растягивается до высоты контейнера (`align-items: stretch`), теряя
пропорции.

Сверка с headless Edge, картинка 852×725:

| Разметка | Edge | Lumen |
|---|---|---|
| `.row{display:flex;width:600px;height:300px}` + `<img>` + `<div>x</div>` | 353×300 | 592×725 |
| `.col{display:flex;flex-direction:column;width:200px;height:300px}` + `<img>` | 200×170 | 852×300 |

Edge в обоих случаях ужимает картинку до свободного места и выводит вторую
сторону из соотношения (353 = 300 × 852/725; 170 = 200 × 725/852). Lumen в
строке берёт остаток контейнера по ширине и сохраняет полную intrinsic-высоту,
в колонке — полную intrinsic-ширину и растянутую высоту.

## Что это НЕ

Не регресс [BUG-734](BUG-734-FIXED.md): дампы `--dump-layout` этой страницы на
baseline и на фиксе совпадают до цифры. 734 задел только не-flex путь.

Не обязательно то же самое, что пункты 1 и 2 [BUG-733](BUG-733-FIXED.md)
(вертикальная навигация и схлопнувшаяся CTA-кнопка на `tbank.ru`): там подпись
обратная — flex-элементы получаются **уже** нужного (кнопка 32 px при
контейнере 1104 px) или **шире** (пункт списка на всю строку), то есть речь
скорее о нераспределении места между элементами, а не о замещаемых элементах.
Проверять отдельно, не сваливать в одну заявку без доказательства.

## Как воспроизводить

`.tmp/b733_flex.html` в ветке `p3-bug-733` (страница печатает оба rect в `<pre>`
на `load`, поэтому один и тот же файл читается и `--dump-layout` у Lumen, и
`--dump-dom` у headless Edge).

## Направление фикса

CSS Flexbox L1 §9.2 и §9.4: flex base size замещаемого элемента берётся из его
intrinsic-размера, а `stretch` по поперечной оси **не применяется** к элементу с
intrinsic-соотношением, если его поперечный размер `auto` и главный определён
(§4.5 + CSS Sizing L4 §4.1). Соотношение теперь доступно как
`style.aspect_ratio` (заполняется в ветке `is_image_element`, см.
[BUG-734](BUG-734-FIXED.md)) — отдельный канал заводить не нужно.

## Фикс (2026-09-14)

Presentational-hint `width`/`height` у `<img>` (декодированный intrinsic
размер или content-атрибуты) помечается новыми полями `ComputedStyle`
`width_is_intrinsic_hint`/`height_is_intrinsic_hint`, чтобы flex-раскладка
отличала «автор задал размер» от «размер — заглушка под intrinsic». Флаг
сбрасывается любой авторской записью в `width`/`height` (явное значение,
`initial`/`inherit`/`unset`).

Flex-basis растянутого по cross-оси replaced item теперь считается по CSS
Flexbox L1 §9.2/§4.5 — transferred size через `aspect_ratio` — вместо сырого
intrinsic-пикселя; чинит и row-, и column-направление (`flex.rs`,
`build_flex_init`). В column-направлении item с intrinsic-hint дополнительно
лэйаутится с явным `UsedSizeOverride` (`probe_width`), чтобы его
`aspect_ratio`-derived высота считалась от реального используемого, а не от
сырого intrinsic, ширины.

Новые тесты: `box_tree::tests::intrinsic_and_wrap::bug736_row_flex_replaced_item_uses_transferred_size`,
`bug736_column_flex_replaced_item_stretches_and_derives_height`.

Гейт: `cargo clippy --workspace --all-targets -- -D warnings` чисто;
`lumen-layout --lib` (3957 тестов) и `--all-targets` (77) зелёные.
`scripts/scoped-test.sh` — единственный красный `cpu_snapshots_match_references`
(те же 7 файлов, что и в [BUG-1048](BUG-1048-FIXED.md)) — предсуществующий
дрейф, не регрессия (правка не трогает paint); `lumen-network` — известный
сломанный гейт [BUG-805](BUG-805-OPEN.md).
