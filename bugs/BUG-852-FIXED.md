# BUG-852 — `content-visibility`/`contain-intrinsic-size` не видны в `getComputedStyle`, а `contentvisibilityautostatechange` не доставляется никогда: шелл считает диффы и складывает их в очередь без потребителя

**Статус:** FIXED 2026-08-25
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 24 — живой замер, маркер `content-visibility-state-event`)
**Область:** `crates/engine/layout/src/selector_query.rs:688` (`computed_style_to_map` — 88 свойств, обоих в списке нет), `crates/shell/src/main.rs:22950` (`take_cv_events` — единственное упоминание, вызывающих нет), `crates/shell/src/main.rs:6504` («Phase 2: P3 доставляет как `contentvisibilityautostatechange` в JS» — доставки не существует)
**Владелец:** P1/P3 (layout + шелл). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Свойства применяются к раскладке, но не существуют для CSSOM, а событие о
смене skipped-состояния не приходит:

```js
el.style.contentVisibility = 'auto';          // inline-стиль читается: "auto"
getComputedStyle(el).getPropertyValue('content-visibility');    // ""
getComputedStyle(el).getPropertyValue('contain-intrinsic-size'); // ""
'oncontentvisibilityautostatechange' in el;                      // false
el.addEventListener('contentvisibilityautostatechange', …);      // не срабатывает
```

## Прямое измерение

`tests/wpt/verify_frame_load_media_gaps.py --variant cv-auto-state`
(2026-08-22, dev-release, Linux, коммит `c583a90b4`, `--seconds 5`, страница
жива — 9 тиков). Элемент с `contain-intrinsic-size: auto 1px;
content-visibility: auto` под распоркой в 2000 px:

| ожидалось | получено |
|---|---|
| `oncontentvisibilityautostatechange` есть | `cv-support onevent=false` |
| `cv-statechange skipped=…` | тишина |
| непустые вычисленные значения | `prop-cv="" prop-cis="" camel-cv=""`, при `inline="auto"` |

`offsetHeight === 1` — то есть размерная подстановка из
`contain-intrinsic-size` работает, дефект только в наблюдаемости.

## Причина (локализована чтением кода)

1. **CSSOM.** `getComputedStyle` отвечает из снимка, который строит
   `computed_style_to_map` (`selector_query.rs:688`, 88 `insert`-ов). Ни
   `content-visibility`, ни `contain-intrinsic-size`/`-width`/`-height` туда
   не кладутся, хотя парсинг и поля `ComputedStyle` есть
   (`style.rs:16617`/`:16640`, `CSS-SPECS.md` отмечает оба как реализованные —
   речь именно о сериализации в снимок).
2. **Событие.** Шелл честно считает дифф skipped-состояния между проходами
   (`collect_cv_skipped`, `main.rs:6517`) и складывает `ContentVisibilityChange`
   в `cv_events` (`main.rs:7869`), а `take_cv_events` (`main.rs:22950`) во
   всём workspace не вызывается ни разу. Диспатча
   `contentvisibilityautostatechange` не существует нигде — `grep` по
   `crates/` даёт только три комментария «Phase 2: P3 доставляет…». Та же
   форма, что у [BUG-809](BUG-809-OPEN.md) (`deliver_layout_shift`) и
   [BUG-839](BUG-839-FIXED.md).

## Масштаб

Маркер `content-visibility-state-event` в `tests/wpt/timeout_audit.py` — **1
id** остатка снимка WPT-RUN-5
(`css/css-contain/content-visibility/content-visibility-auto-state-changed-first-observation.html`,
оба подтеста ждут события), рядом ещё два id того же каталога висят на
`ResizeObserver` ([BUG-661](BUG-661-FIXED.md)). Отсутствие свойств в снимке
шире: любой тест, читающий `getComputedStyle(el).contentVisibility`, получает
`""` вместо `visible`.

## Направление починки (не предписание)

Свойства — добавить в `computed_style_to_map` рядом с `contain`. Событие —
дренировать `cv_events` там же, где шелл дренирует прочие очереди в JS, и
диспатчить `ContentVisibilityAutoStateChange` с полем `skipped`.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_frame_load_media_gaps.py
   --variant cv-auto-state` — ожидаются `cv-support onevent=true`,
   `cv-statechange skipped=…` и непустые `prop-cv`/`prop-cis`.
2. WPT: `run_report.py --all --root css/css-contain/content-visibility`.

---

## Исправлено (P1, 2026-08-25, двумя коммитами)

Заявка называла **два** дефекта; их оказалось **шесть**, и четыре из четырёх
лишних нашёл замер, а не чтение названных функций.

### Часть 1 — CSSOM (коммит `87b86c322`)

`computed_style_to_map` не содержал не двух имён, а **пяти**: `contain` и оба
лонгханда `contain-intrinsic-width`/`-height` нашлись тем же замером
(вариант пробы `cv-computed`, добавлен ДО правки). Третий дефект — разбор:
`parse_contain_intrinsic_one` съедал префикс `auto` и возвращал только длину,
так что `contain-intrinsic-size: auto 1px` читался бы обратно как `1px`. Флаг
теперь хранится отдельно от длины, по одному на ось; раскладка не изменилась
(последнего запомненного размера у движка нет, `auto` для неё и правда ничего
не значит). `contain` сериализуется набором ключевых слов, а не
`strict`/`content` — CSS Containment L3 §3 определяет их как раскрывающиеся.

### Часть 2 — событие (этот коммит)

**Прямая починка по «направлению» из заявки дала бы неверные события.** Дренаж
`cv_events` — последний шаг; до него стояло то, чего заявка не видела: **само
состояние считалось неправильно**. `collect_cv_skipped` называл бокс
пропущенным по признаку «список детей пуст», а это несовпадение в обе стороны.
Пустой `<div style="content-visibility:auto">` — ровно то, что строит
`content-visibility-auto-state-changed-first-observation.html`, — выглядел
пропущенным, где бы он ни стоял; а раскладка про него вообще не спрашивала,
потому что `cv_should_skip` вызывается только при `!children.is_empty()`
(скрывать нечего). Правило релевантности поэтому вынесено из thread-local
пути раскладки в чистую функцию `lumen_layout::cv_is_skipped(relevant, top_y,
scroll_y, viewport_h)`: одно правило, два вызывающих — раскладка для боксов с
детьми и шелл для **всех** `auto`-боксов. Две копии разошлись бы.

**Семантика диффа тоже не могла остаться прежней.** «Появился узел ⇒
`skipped: true`, исчез ⇒ `skipped: false`» не выражает того, что требует
CSS Contain L2 §4.1 и на чём стоит `first-observation.html`: первое наблюдение
стреляет в **обе** стороны — `skipped: false` для элемента во вьюпорте не менее
обязателен, чем `skipped: true` для элемента под ним. Теперь база диффа —
карта `NodeId → skipped` **каждого** `auto`-узла (`Lumen::cv_auto_state`,
отдельно от `cv_skipped`, который остался только ratchet-у: «узла в карте нет»
и «узел не пропущен» — разные вещи, и на первом держится событие первого
наблюдения). Узел, покинувший дерево, не порождает ничего — отсоединённый
элемент молчит (`content-visibility-auto-state-changed-removed.html`).

**Точка выдачи одна на все источники** — шаг 1.65 `RedrawRequested`, внутри
которого §4.1 и определяет релевантность. `refresh_cv_state` зовётся из
четырёх мест (загрузка, релейаут, восстановление вкладки, ratchet при скролле),
и **в двух из них JS-контекста ещё нет**, поэтому «диспатчить там, где считаем»
не работает: события копятся в очереди и уходят первым кадром, на котором
контекст появился. Иначе страница, объявившая `content-visibility: auto` в
разметке, теряла бы ровно своё первое наблюдение.

**Четвёртый и пятый дефекты, которых заявка не называла.** IDL-аксессора
`oncontentvisibilityautostatechange` не было (контент-атрибутная половина уже
работала — `_lumen_is_on_attr_name` принимает любой `on*`), а спрашивает
`'oncontentvisibilityautostatechange' in el` именно про него; имя добавлено в
`_LUMEN_EVENT_HANDLER_ATTRS`. И у события не было `target`: `_lumen_dispatch`
его не ставит ([BUG-873](BUG-873-OPEN.md)), а страница, слушающая несколько
элементов одним слушателем, различить их больше нечем — заполняется так же,
как в `_lumen_details_fire_toggle` ([BUG-851](BUG-851-FIXED.md)).

Класс события — `ContentVisibilityAutoStateChangeEvent` с readonly `skipped`
(WebIDL `boolean`, дефолт `false`). Мёртвый `take_cv_events` удалён вместе с
комментарием «Phase 2: P3 доставляет…», который теперь неправда.

### Замер после

Проба (`verify_frame_load_media_gaps.py`, dev-release, Windows, страница жива —
7 тиков) — все три варианта зелёные:

| вариант | получено |
|---|---|
| `cv-first-observation` | `cvfo-support onevent=true`, `cvfo-event top n=1 skipped=false`, `cvfo-event bottom n=2 skipped=true`, `total=2`, после `remove()` — по-прежнему `total=2` |
| `cv-auto-state` | `cv-support onevent=true`, `cv-statechange skipped=true`, `prop-cv="auto" prop-cis="auto 1px" camel-cv="auto"` |
| `cv-computed` | `cvc-attr-fired div`, `cvc-attr-fired svg`, все пять имён непустые |

WPT — **A/B по всей категории** `css/css-contain/content-visibility`
(`run_report.py --all --recursive`, 190 тестов + 295 подтестов; «до» — тот же
слот с отложенной правкой и пересобранным бинарником, то есть ровно `main`).
Считать по одному id было бы недостаточно: правка меняет не только доставку,
но и содержимое `cv_skipped`, а его читает ratchet `maybe_expand_cv_relevant`,
то есть потенциально саму отрисовку.

| | main | +правка |
|---|---|---|
| подтестов пройдено | 117/295 | **122/295** |
| тестов OK | 64 | **65** |
| тестов TIMEOUT | 6 | **4** |
| тестов ERROR | 7 | 8 |
| reftest FAIL | 72 | 72 — **ни один не сдвинулся** |

Совпадение 72/72 по reftest-ам и есть ответ на вопрос про ratchet: отрисовка не
изменилась. Поимённо сдвинулись только шесть проверок вверх (`…-first-observation.html`
TIMEOUT → OK, обе его пары подтестов + три подтеста
`…-auto-state-changed.html` — все UNEXPECTED-PASS) и одна вниз по
классификации: `…-removed.html` TIMEOUT → ERROR.

Считать «Unexpected results» этих прогонов регрессом нельзя: baseline
`tests/wpt/metadata/` покрывает 75 из 233 файлов категории, поэтому непокрытый
reftest-FAIL там числится «неожиданным» и на `main` тоже (107 против 117 —
это +6 улучшений и +4 ставших видимыми провала, а не десять поломок).

### Остатки (не этот баг)

* `/common/rendering-utils.js` **не вендорен**, поэтому `waitForAtLeastOneFrame`
  — `ReferenceError`, и это единственное, обо что спотыкаются
  `…-removed.html` целиком (отсюда TIMEOUT → ERROR) и два подтеста
  «content attribute test» (`div` и `svg`) в `…-auto-state-changed.html`.
  Событие там уже сработало: первый `await` в `…-removed.html` — ожидание
  `contentvisibilityautostatechange`, и он разрешился, иначе до
  `waitForAtLeastOneFrame` дело бы не дошло. Дыра вендоринга, не движка —
  P2/WPT-VENDOR.
* Два подтеста `…-auto-state-changed.html` («fires when skipped» / «fires when
  not skipped») остаются TIMEOUT/NOTRUN. Они ведут страницу через
  `middle.scrollIntoView()` по распоркам `.spacer { height: 3000px }` **без
  фона**, то есть зависят от того, прокручивается ли такая страница вообще;
  механизм здесь не замерялся, и записывать его по аналогии с готчей про
  нерисующую распорку в `CLAUDE.md` до замера не следует.
* `contentvisibilityautostatechange` доставляется раз в кадр, а не задачей на
  каждый переход: две смены состояния между кадрами схлопываются в одну.
