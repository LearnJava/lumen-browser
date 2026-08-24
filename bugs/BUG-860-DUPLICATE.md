> **Дубль.** Тот же механизм двумя днями раньше описан в
> [BUG-808](BUG-808-FIXED.md) (заведён 2026-08-21, WPT-RUN-6 срез 15):
> прототип `Animation` не подключён к `EventTarget`. По конвенции выживает
> первый по дате — этот файл оставлен только как след замера среза 25,
> измерения и разбор починки перенесены в BUG-808. Починено 2026-08-24 (P1).

# BUG-860 — `Animation` не EventTarget: `addEventListener`/`removeEventListener` у анимации отсутствуют, есть только `on*`-свойства

**Статус:** DUPLICATE → [BUG-808](BUG-808-FIXED.md) (закрыт 2026-08-24)
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 25 — живой замер, маркер `wa-finish`)
**Область:** `crates/js/src/dom.rs` — объект, возвращаемый `element.animate()` (Web Animations shim): есть `onfinish`/`oncancel`/`onremove`, `cancel`, `finish`, `reverse`, `timeline`, `ready`, `finished`, но нет `addEventListener`/`removeEventListener`/`dispatchEvent`
**Владелец:** —. Заведён P2 в ходе WPT-задачи; чинился как BUG-808.
**Родственный:** [BUG-704](BUG-704-OPEN.md) (`persist`/`commitStyles` отсутствуют) — тот же объект, соседняя дыра.

## Симптом

```js
var anim = el.animate([{opacity:1},{opacity:0}], 300);
anim.addEventListener('finish', handler);
// TypeError: anim.addEventListener is not a function
```

`Animation` по спеке (Web Animations §5.3) наследует `EventTarget`, и WPT
подключается к `finish`/`cancel`/`remove` обоими способами вперемешку.
Дополнительный вред: исключение выбрасывается **на первой же строке**
подготовки теста, поэтому файл не доходит и до тех проверок, которые
движок бы прошёл — так этот баг маскирует состояние всего семейства.

## Прямое измерение

`tests/wpt/verify_focus_mutation_animation_gaps.py --variant wa-finish`
(2026-08-23, dev-release, Linux, `main` = `530d0a444`, `--seconds 5`,
страница жива — 9 тиков):

```
wa-eventtarget addEventListener=undefined cancel=function finish=function
               reverse=function timeline=DocumentTimeline
```

Что при этом **работает** (важно, чтобы не чинили лишнего): `onfinish`
приходит (`wa-onfinish t=303.9`), `anim.finished` резолвится
(`wa-finished-promise`), `anim.ready` резолвится, `playState` меняется
`running → finished`, повторный `currentTime = 0` + `play()` даёт второй
`onfinish`. Первый прогон этой же пробы (до правки самой пробы) потерял всё
перечисленное именно потому, что упёрся в `addEventListener` и умер —
ровно то, что происходит с тестами.

## Масштаб

Механизм `animation-not-eventtarget` в `tests/wpt/timeout_audit.py` — часть
кластера `web-animations`/`scroll-animations` в остатке снимка WPT-RUN-5
(7 id вместе с [BUG-861](BUG-861-OPEN.md) и [BUG-704](BUG-704-OPEN.md)):
`web-animations/interfaces/Animation/onfinish.html`, `onremove.html`,
`persist.html`, `web-animations/animation-model/keyframe-effects/effect-value-replaced-animations.html`,
`web-animations/interfaces/Animatable/getAnimations-iframe.html`,
`web-animations/timing-model/animations/start-time-compat.html`,
`scroll-animations/scroll-timelines/updating-the-finished-state.html`.

## Направление починки (не предписание)

Дать объекту анимации общий `EventTarget`-базис (в шиме уже есть
`EventTarget.prototype`, который используется, например, контейнером
service worker'а) и диспатчить существующие `finish`/`cancel`/`remove`
через него, оставив `on*` как рефлексию.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_focus_mutation_animation_gaps.py
   --variant wa-finish` — ожидается `wa-finish-listener` рядом с `wa-onfinish`.
2. WPT: `run_report.py --all --root web-animations/interfaces/Animation`.
