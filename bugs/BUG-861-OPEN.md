# BUG-861 — перемотка завершённой анимации назад не возвращает её в работу: второй `finish` не приходит, `finished` не заменяется

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 25 — живой замер, маркер `wa-seek-refire`)
**Область:** `crates/js/src/dom.rs` — Web Animations shim: сеттер `currentTime` не выполняет «update the finished state» (Web Animations §4.4.11), `playState` остаётся `finished`, а `finished`-промис не пересоздаётся
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
var anim = el.animate(null, 1);
anim.onfinish = () => {
  anim.currentTime = 0;      // спека: анимация снова «running»,
  anim.onfinish = () => t.done();   // и по завершении finish приходит второй раз
};
// второй onfinish не приходит никогда
```

Спека требует при любой смене `currentTime` прогонять процедуру обновления
завершённого состояния: если текущее время ушло от конца, анимация перестаёт
быть завершённой, `finished`-промис **заменяется** новым, и следующий приход
в конец даёт новое событие `finish`.

## Прямое измерение

`tests/wpt/verify_focus_mutation_animation_gaps.py --variant wa-seek-refire`
(2026-08-23, dev-release, Linux, `main` = `530d0a444`, `--seconds 5`,
страница жива — 9 тиков; код варианта дословно повторяет зависший подтест):

| шаг | ожидалось | получено |
|---|---|---|
| `animate(null, 1)` | объект `Animation` | ✔ (`ready`/`finished` — объекты) |
| первый `finish` | есть | ✔ `ws2-finish n=1 t=140.6 playState=finished` |
| `currentTime = 0` | `playState → running`, `finished` заменён | `playState=finished`, `finished-replaced=false` |
| второй `finish` | есть | **нет** (`ws2-checked n=1`) |

Контраст, отделяющий этот дефект от «событий вообще нет»: в варианте
`wa-finish` та же перемотка **с явным `play()`** второй `onfinish` даёт. То
есть отсутствует именно неявный переход из завершённого состояния при
одной лишь смене времени.

## Масштаб

Механизм `animation-finished-state` в `tests/wpt/timeout_audit.py` — **2 id**
остатка снимка WPT-RUN-5 с одинаковым именем зависшего подтеста
`Animation finish event is fired again after seeking back to start`:
`web-animations/timing-model/animations/updating-the-finished-state.html` и
`scroll-animations/scroll-timelines/updating-the-finished-state.html`. В том
же файле висит и соседний подтест про замену `finished`-промиса
(`Animation finished promise is replaced after replaying from start`).

## Направление починки (не предписание)

Вынести «update the finished state» в один хелпер и звать его из сеттеров
`currentTime`/`startTime`/`playbackRate` и из `play()`/`pause()`/`reverse()`,
пересоздавая `finished`-промис при уходе из завершённого состояния.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_focus_mutation_animation_gaps.py
   --variant wa-seek-refire` — ожидается `ws2-finish n=2`.
2. WPT: `run_report.py --all --root web-animations/timing-model/animations`.
