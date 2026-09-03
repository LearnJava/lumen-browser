# BUG-975: программная запись скролла (`scrollTo`/`scrollBy`/`scrollLeft=`/`scrollTop=`) не видна синхронному чтению в том же тике

**Статус:** OPEN
**Компонент:** js (`crates/js/src/v8_runtime/install/platform.rs::install_scroll_state`, нативы `_lumen_request_scroll`/`_lumen_get_scroll_state`)
**Найден:** P3, 2026-09-04, при доследовании остатка [BUG-504](bugs/BUG-504-OPEN.md) (файл `overflow-clip-clamps-and-ignores-scroll-offsets-vertical-rl.html`) уже после посадки CSSOM-4.

## Симптом

Любая программная запись скролла контейнера, синхронно прочитанная в том
же скрипте без ожидания следующего тика, отдаёт СТАРОЕ значение, а не
только что установленное:

```js
var s = document.getElementById('s'); // overflow: auto, реальный скролл-контейнер
s.scrollTo(20, 30);
console.log(s.scrollLeft, s.scrollTop); // печатает 0 0, ожидается 20 30
```

Живая проба (`--mcp-live-port`, `.claude/worktrees/p3-work/.tmp/scrolltest.py`,
не привязана к CSSOM-4/BUG-504 конкретно) подтверждает это на голом
`overflow: auto`-контейнере без всякого `clip`/`hidden`/`writing-mode`.
Тот же механизм у `scrollBy()`, прямого присваивания `scrollLeft =`/
`scrollTop =`.

## Причина

`_lumen_request_scroll(nid, x, y)` (`install_scroll_state`,
`crates/js/src/v8_runtime/install/platform.rs`) только кладёт запрос в
очередь `pending_scrolls`:

```rust
reg!(scope, ctx, store, "_lumen_request_scroll", move |nid: u32, x: f32, y: f32| {
    ps.lock().unwrap().push((nid, x as f32, y as f32));
});
```

Очередь дренируется шеллом асинхронно (`take_scroll_requests()`, следующий
тик событийного цикла), который затем зовёт `lumen_layout::set_scroll_position`
на живом дереве и только после этого обновляет JS-видимый кэш
`scroll_states` через `update_scroll_states`. `_lumen_get_scroll_state`
(тот же файл) читает исключительно этот кэш — запрос, ещё сидящий в
`pending_scrolls`, для него не существует.

CSSOM-4 (BUG-493) закрыла симметричный разрыв для стилевых/геометрических
чтений (`getComputedStyle`/`getBoundingClientRect`) — синхронный флаш
(`FlushHandles::maybe_flush`) форсирует пересчёт layout прямо в нативе.
BUG-504 part 10 (тот же коммит, что заводит этот баг) распространила
`maybe_flush` и на `_lumen_get_scroll_state`, но флаш триггерится только
на `dom_dirty`/`never_flushed` — программный скролл сам по себе не трогает
DOM/стиль, поэтому флаш не срабатывает вовсе, и `pending_scrolls` эта
цепочка не читает ни при каких условиях.

## Масштаб

Как минимум весь `tests/wpt/css/cssom-view/elementScroll.html` (`.ini`
сейчас списывает провал на другую причину, [BUG-475](bugs/BUG-475-FIXED.md),
закрытый 2026-09-02 — `.ini` не перепроверен свежим `run_report.py` после
того фикса, поэтому неизвестно, какая доля восьми `expected: FAIL`
объясняется этим багом, а не пересчитана заново) и второй блок
`overflow-clip-clamps-and-ignores-scroll-offsets-vertical-rl.html`
(первые два `assert_equals` теста — `scrollTo(-40, 50)` → синхронное
чтение — падают здесь, а не на переходе `overflow: clip`, который
[BUG-504](bugs/BUG-504-OPEN.md) part 10 как раз закрывает). Вероятно
задевает любой WPT-файл с синхронным `scrollTo`/`scrollBy`-раунд-трипом —
не пересчитано отдельным прогоном категории `cssom-view`.

## Предлагаемая правка

Не точечный фикс — `_lumen_request_scroll` нужно либо (a) синхронно
применять запрос к какому-то представлению, которое `_lumen_get_scroll_state`
тоже видит (симметрично `maybe_flush`, но без полного релэйаута — клэмп
скролла не требует пересчёта стиля, только знания `padding_box`/
`scrollable_extent`, которых у голого кэша `[f32;4]` нет), либо (b)
оптимистично обновлять сам кэш `scroll_states` в момент постановки в
очередь, оставляя дренаж `pending_scrolls` на шелл как есть (шелл потом
применит то же значение повторно — идемпотентно, если контейнер не успел
измениться между двумя событиями). Вариант (b) проще и дешевле, но
клэмпинг (`overflow: clip` → 0, отрицательный диапазон в RTL/vertical-rl,
maximum-scroll-limit) в кэше без живого layout посчитать нельзя правильно —
нужно решение, каким приближением клэмпить оптимистичное значение, прежде
чем шелл его перепосчитает по-настоящему.

## Смежное

[BUG-965](bugs/BUG-965-OPEN.md) — не то же самое: там headless-драйвер
(`InProcessSession`, `--mcp-port`) вообще никогда не зовёт
`update_scroll_states` ни на каком тике, поэтому `scrollLeft` там 0 всегда,
даже спустя сколько угодно тиков. Здесь — живое окно (`--mcp-live-port`,
шелл), где `update_scroll_states` штатно зовётся каждый relayout, но
конкретно ЭТОТ тик (тот же синхронный скрипт, что сделал запрос) его ещё
не увидел.

## Repro

```bash
python tests/wpt/verify_bug504_vertical_rl_clip.py --binary <АБСОЛЮТНЫЙ путь к lumen.exe>
```

первые два `[ФЕЙЛ]` (`after_scrollTo_hidden`) — этот баг, не BUG-504.
