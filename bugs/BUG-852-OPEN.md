# BUG-852 — `content-visibility`/`contain-intrinsic-size` не видны в `getComputedStyle`, а `contentvisibilityautostatechange` не доставляется никогда: шелл считает диффы и складывает их в очередь без потребителя

**Статус:** OPEN
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
   [BUG-839](BUG-839-OPEN.md).

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
