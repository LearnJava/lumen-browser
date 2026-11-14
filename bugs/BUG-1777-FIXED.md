# BUG-1777 — html/: 6 регрессий WPT 10-22→10-27 без бисекта (reftest dir-shadow-41, draw-element-image, 4 теста −1…−3 подтеста)

**Статус:** FIXED 2026-11-14 (последний остаток — reftest `draw-element-image/css-backdrop-filter-and-mask` — снят срезом 2 BUG-1798)
**Заведён:** 2026-10-28 (P2, WPT-DROP-1027-S3)
**Область:** layout / paint / shell (окна ниже; виновники не найдены)

## Симптом и окна

Все шесть id стабильно хуже `d4114145a` (WPT-RUN-16) в обоих перепрогонах и на `main` (`12ff9f37d`)
воспроизводятся с теми же числами, что на `a76e4c7b9`. Окно = между соседними бинарями сетки
`.tmp/bisect-bin` (хронологически: ok…bad); порядковые номера — позиции в
`git log --first-parent d4114145a..a76e4c7b9` (1 = `a76e4c7b9`, 144 = самый старый).

| id | old | new / main | окно (ok…bad) | позиции |
|---|---|---|---|---|
| `/html/dom/elements/global-attributes/dir-shadow-41.html` | reftest PASS | FAIL | `bf47a575d`…`417f685ab` | 25–42 |
| `/html/canvas/element/manual/draw-element-image/css-backdrop-filter-and-mask.tentative.html` | reftest PASS | FAIL | `417f685ab`…`a76e4c7b9` | 1–25 |
| `/html/rendering/non-replaced-elements/tables/table-ua-stylesheet.html` | 2/5 | 1/5 | `417f685ab`…`a76e4c7b9` | 1–25 |
| `/html/semantics/disabled-elements/event-propagate-disabled-keyboard.tentative.html` | 29/29 | 28/30 | `90cf87014`…`38f31ef3c` | 91–112 |
| `/html/semantics/forms/the-label-element/proxy-modifier-click-to-associated-element.tentative.html` | 2/4 | 1/4 | `38f31ef3c`…`e3536bfb3` | 75–91 |
| `/html/semantics/interactive-elements/the-dialog-element/abspos-dialog-layout.html` | 6/10 | 4/10 | `old`…`da258ae4f` | 128–144 |

Список merge-коммитов окна: `git log --first-parent --oneline <ok>..<bad>`. Для `abspos-dialog-layout`
подтесты «Dialog should not still be centered when removed…» (expected 381 got 820) и
«recentered if showModal() after removing open» (expected −4604 got 0) — геометрия
`showModal()`/`abspos`; для `proxy-modifier-click` и `event-propagate-disabled` — события клика/
клавиатуры на disabled/label (shell/js input). Следующий шаг — бисект по окну (≤ 4 сборки на id).

## Прогресс

**2026-11-12 (P3): `abspos-dialog-layout` 4/10 → 10/10.** Закрыт без бисекта — трассировочный
probe-пейдж выявил корни, бисект по окну не понадобился:

1. `CSSStyleDeclaration.setProperty` превращал `val` в `String(val)` — `style.height = null`
   записывал литеральную строку `"null"` вместо удаления объявления (CSSOM: аргумент
   `[LegacyNullToEmptyString]`, `null` → `""` = remove). Высота `20000px` из подтеста «tall
   dialog» протекала через `reset()` во все последующие подтесты.
2. Открытый `<dialog>` не имел UA `display: block` (спека блокифицирует его через UA
   `position: absolute`, которого в движке нет) — элемент падал в inline-дефолт и его текст
   сливался в inline-run родителя; `collect_layout_rects` публиковал под nid диалога прямоугольник
   родителя (10000px body, top 820 родителя).
3. Центрирование модального диалога существовало только на paint-стороне
   (`build_dialog_overlay`) — `getBoundingClientRect` отвечал статической позицией. Теперь
   шим для диалога из `_lumen_modal_dialog_nids` отвечает той же геометрией, что рисует оверлей
   (`vp/2 − w/2`, по вертикали с clamp на верх вьюпорта для диалога выше него — HTML5 §4.11.7),
   а `build_dialog_overlay` сведён к одной формуле `(vp_h/2 − h/2).max(0) − r.y + scroll_y`.
4. Удаление диалога из документа не снимало `data-lumen-modal`/стек модальных nid — по HTML LS
   «top layer» membership сбрасывается при отключении от документа. Обёртка `_lumen_remove_child`
   (`web_api_shim_mid_b4.js`) снимает сентинел и pop-ает стек. `shortcut`: вложенный модальный
   диалог в удаляемом поддереве не чистится (нативный detach из `innerHTML=""` тоже) — пока
   ни один путь этого не требует.

Попутно: удалён устаревший `tests/wpt/metadata/.../table-ua-stylesheet.html.ini` (три
`expected: FAIL` остались от состояния до фикса `7f2b54104`; подтесты проходят 5/5), и
схлопнуты двойные `if` в `lumen/click.rs:619` (clippy collapsible_if, висел на main после
`4f3ad6c59`).

**2026-11-12 (P3, позже): `event-propagate-disabled-keyboard` 28/30 → 30/30 и
`proxy-modifier-click-to-associated-element` 1/4 → 4/4.** Не регресс окна — тест стал честным:
`1e7913d580` (p1-shadow-parts) сделал `attachShadow` настоящим (фрагмент переносится детьми,
`ShadowRoot.getElementById` — scoped), до этого shadow-механика была инертной и тестовые
fallback-и проходили мимо. Три корня, найдены пробами, бисект не понадобился:

1. Путь события не пересекал границу shadow-дерева: `_lumen_event_path` останавливался на
   shadow root (родитель null) — bubbling-события из shadow-контента не доходили до
   light-DOM предков, document, window. Теперь подъём продолжается от shadow root к host
   (тот же обход, что у `_lumen_resource_is_connected`).
2. `document.activeElement` при фокусе внутри shadow-дерева возвращал внутренний элемент, а
   не top-level host (delegatesFocus-подтест падал на assert ещё до key-событий). Теперь
   подъём до корня дерева и переход к host, вложенность до 16 границ.
3. Per-frame `_lumen_focus_fixup` сбрасывал фокус shadow-узла: `_lumen_focus_not_rendered`
   в подъёме предков спрашивал `display` у самого shadow root — а тот не публикует
   computed style вовсе (BoxKind::Skip), читался как «not rendered» → `_lumen_focus_drop()`
   каждый кадр. Теперь shadow root в подъёме пропускается (переход к host).
   Диагностика: shell-side eprintln показал `focus_reqs [Some(17)] → … → [None]` — фокус
   применялся и тут же сбрасывался фикса́пом.

`proxy-modifier-click` пал тем же кликом-целью: JS-клик целился в блок-владелец InlineRun
(`hit_result.node` = `<div>`-обёртка), поэтому label-forwarding (HTML LS §4.10.4) не
срабатывал ни для одного label (проба: gBCR label корректны и различны, elementFromPoint
верен, но replay-клик даёт `click` на `<div>` без mousedown). Фикс: цель события = родительский
ЭЛЕМЕНТ инлайн-текста (`hit_result.source_node` → ближайший элемент-предок), как требует DOM;
`target_node` используется и для диспетчера клика, и для label-проверки. Побочно это же даёт
корректный `event.target` для кликов по инлайн-элементам (`<span>` внутри `<p>` — span, а не p;
предки по-прежнему получают событие bubbling-ом).

**2026-11-13 (P3): `dir-shadow-41` и `dir-shadow-42` PASS.** Два корня (пробами, без бисекта;
42 раньше не числился в баге — падал так же, 282px):

1. Направленность `<slot dir=auto>` без сильных символов в assigned nodes: брали
   directionality host'а, по HTML LS §3.2.6.1 auto directionality пуст → `ltr`
   (`dir-shadow-42`: host `rtl`, ожидание `ltr`). Исправлено в обоих путях — `:dir()`
   (`matching/forms.rs::slot_element_directionality`) и presentational hint
   `direction` (`style/presentational.rs::apply_dir_presentational_hint`);
   `slot_host_directionality`/`slot_auto_directionality` удалены как лишние.
2. Геометрия: `display:contents` (слот) на уровне детей блока строил собственный
   InlineRun со СВОИМ стилем, и его `direction` задавал направление строки
   (`dispatch_box.rs::para_dir`) вместо направления блока-хозяина — пустой span
   прилипал к левому (rtl-див) / правому (ltr-див) краю. По CSS Display L3 §3.1
   contents прозрачен и на этом уровне: его flat-дети вливаются в inline-контекст
   блока (`inline_build.rs::produces_inline_segments`). Исключение — UA-слот
   `<select>`/`<details>` (`Document::ua_slot_role`): боксовый путь держит в дереве
   Skip-боксы его display:none-контролов (`<option>` под `<select>` — тесты
   `state_pseudo_classes` на :checked/:disabled), инлайн-прозрачность их выбрасывает.
   Лишние `element_directionality`/`is_tel_input`/`input_type` presentational.rs
   удалены (дубль forms.rs).

Тесты `state_pseudo_classes::dir_shadow_slot_*`; полный lumen-layout — множество
падений совпадает с чистым main (17, таблицы/BUG-1784/1785 и пр.).

**2026-11-14 (P3, срез 2 BUG-1798): `css-backdrop-filter-and-mask` PASS** — байт-в-байт с референсом
(fuzzy теста `3900px@1` больше не задействован). Корень остаточного 200 px@2 был не в маске и не в
element-image-конвейере, а в f32-округлении выборки при 1:1-блите в
`Context2D::draw_image_cropped` (`crates/engine/canvas/src/lib.rs`): `(dy/dest_h)·sh` в f32 давало
`52.999996` для строк 53 и 100-строчного блита, `floor` брал предыдущую строку источника —
canvas-битмап дублировал строки 53 и 59 растра элемента. Починка — snap `+1e-3` до `floor` (реальные
дробные выборки сдвигаются не более чем на тысячную пикселя, NEAREST этого не различает); регресс-тест
`draw_image_cropped_1to1_preserves_every_row`. Попутно тест закрыл оба REGRESS из BUG-1806
(`18-images`, `61-view-transitions` — снова 0.00 %).

## Воспроизведение

`tests/wpt/run_smoke.py --binary <lumen.exe> <id>`; reftest — без `LUMEN_NO_PAINT`. Бинари
сетки лежат в `.claude/worktrees/p2-work/.tmp/bisect-bin/<sha>/lumen.exe`.
