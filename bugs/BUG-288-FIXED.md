# BUG-288 — TEST-14 (overflow): scrollbar на overflow-axis-coerced контейнере не совпадает с overlay-scrollbar Edge

**Статус:** FIXED (закрыто ревизией) 2026-09-01
**Компонент:** paint (`emit_scrollbars`, `display_list.rs`) / test-triage
**Найден:** 2026-07-16, при триаже BUG-287 (первый из 33 нетриаженных тестов)

## Симптом

`graphic_tests/14-overflow.html` (без текста, только графика — 4 демо
`overflow`/`overflow-x`/`overflow-y`) — TEST-14 = 1.63% (порог 0.5%), был чист
(0.03%) с момента BUG-020 (2026-05-26).

## Диагностика

`--dump-display-list` на 14-overflow.html:

```
FillRect (25.00, 165.00, 160.00, 100.00) #2d3748ff     ; .ct фон (третье демо)
PushScrollLayer clip=(25.00,165.00,160.00,100.00) scroll=(0.00,0.00)
FillRect (25.00, 165.00, 220.00, 140.00) #38a169d9      ; .ch (child) клипован
PopScrollLayer
DrawScrollbar vertical track=(173,165,12,100) thumb=(175,165,8,71)
```

Третье демо — `overflow-x: hidden; overflow-y: visible`. По BUG-020 (CSS
Overflow L3 §2.1) один явно-неvisible axis коэрсит второй `visible` axis в
`auto`: результат `(overflow-x=Hidden, overflow-y=Auto)`. Юнит-тест
`style::tests::overflow_axis_coercion_visible_plus_hidden` подтверждён зелёным
на HEAD — коэрсия сама по себе не сломана.

Клип применяется верно (`PushScrollLayer` на 160×100, как и должно быть).
Расхождение — в `DrawScrollbar`: `emit_scrollbars` (BUG-220, 2026-06-24,
`display_list.rs`) рисует статический scrollbar для ЛЮБОГО `overflow: auto`/
`scroll` axis с переполнением контента, независимо от того, как контейнер
попал в `auto` (явно в CSS или через коэрсию BUG-020). Четвёртое демо
(`overflow-x: visible; overflow-y: hidden` → коэрсия в `(Auto, Hidden)`)
получает горизонтальный scrollbar тем же путём.

BUG-020 landed 2026-05-26, TEST-14 был чист. BUG-220 landed 2026-06-24 —
scrollbar стал рисоваться и на ordered (stacking-context) пути, включая эти
coerced-auto контейнеры. Никто не прогонял TEST-14 методично между этими
датами, поэтому регрессия не была замечена до полного прогона BUG-287
(2026-07-15).

## Почему это DEBTOR, а не дефект

`bugs/BUG-220-FIXED.md` уже документирует тот же класс на TEST-83: «Edge на
Windows показывает overlay-scrollbar, который скрыт в статическом
скриншоте» — рисование scrollbar статически (а не overlay/auto-hide) было
осознанным решением ради консистентности между `walk` и `box_layer_ops`
путями. TEST-14 не содержит текста (в отличие от большинства тестов),
поэтому весь 1.63% diff — это ИСКЛЮЧИТЕЛЬНО два нарисованных scrollbar-виджета
(vertical на демо 3, horizontal на демо 4), которых в headless-скриншоте Edge
не видно. Геометрия клипа, цвета, позиционирование — всё верно и совпадает
с Edge.

Совпасть с Edge можно только подавив рисование scrollbar на auto-coerced
контейнерах — это либо (а) специальный кейс, ломающий консистентность с
BUG-220, либо (б) требует полной эмуляции overlay-scrollbar (invisible до
hover/scroll) — вне scope точечного фикса, отдельная фича уровня P1.

## Резолюция

`14` добавлен в `KNOWN_DEBTORS` (`graphic_tests/run.py`) с baseline 1.63%,
маркер `BUG-288`. Порог 0.5% не менялся (правило проекта) — ratchet
через отдельный allow-list, тот же механизм, что TEST-83/BUG-220.

## Связь с BUG-287

Один из 33 нетриаженных тестов BUG-287. Диагностика показала, что диапазон
коммитов, который BUG-287 подозревал (`afdc823d..7e4f2a97`), для ЭТОГО теста
неверен — регрессия TEST-14 внесена BUG-220 (2026-06-24), задолго до
`afdc823d` (2026-07-15). См. обновление в `bugs/BUG-287-FIXED.md`.

## Закрытие (P3-ревизия, 2026-09-01)

Долг уже снят побочно коммитом `55aec8b0a` (2026-07-24, «CC-CSS-2:
overflow:auto — вложенный скролл + BUG-335 (PushScrollLayer clip-transform)»,
[BUG-335](BUG-335-FIXED.md)): `PushScrollLayer` в wgpu `renderer.rs` не
применял `apply_transform_to_clip()` к своему `clip_rect` перед пересечением
с `clip_stack` (в отличие от `PushClipRect`/`PushClipRoundedRect`/
`PushClipPath`, уже пофикшенных BUG-276) — под живым chrome-transform это
давало неверный scissor и портило рендер scroll-контейнеров шире одного
теста. Фикс заодно поправил TEST-14: коммит-сообщение прямо фиксирует
«TEST-14 1.63%→0.20% (долг снят)», и запись `'14'` тем же коммитом удалена
из `KNOWN_DEBTORS` (`graphic_tests/run.py`). `BUGS.md`/этот файл/
`STATUS-P3.md` обновлены не были — расхождение статуса провисело полтора
месяца до этой ревизии.

Перепроверено живым прогоном: `python graphic_tests/run.py --only 14
--no-cache` дал **PASS 0.20%** три раза из четырёх подряд (один выброс —
`FAIL 100.00%`, диагностирован как gdigrab-глитч захвата фокуса — тот же
класс, что общий гочта «gdigrab captures the whole desktop»; повторный
прогон сразу после снова дал 0.20%, дефект не воспроизвёлся). Diff-картинка
на 0.20% визуально пуста — курсор и системные артефакты захвата, никакой
геометрии теста. Точечного P3-фикса не требовалось — код не менялся.

Класс «статический scrollbar Lumen vs overlay-scrollbar Edge» сам по себе
не закрыт — он остаётся живым остатком у [TEST-83/BUG-220](BUG-220-FIXED.md)
и у TEST-149 (`KNOWN_DEBTORS['149']`, помечен `BUG-335`, см. этот файл).
Закрыт только конкретный экземпляр (TEST-14, axis-coercion сценарий),
который раньше был заведён под этим номером.
