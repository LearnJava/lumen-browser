# BUG-1095 — SMIL `begin` в форме syncbase/event (`id.end`, `id.begin+2s`, `anim.beginEvent`) не запускает анимацию вовсе — страница виснет до `TIMEOUT`, а не проваливается с `FAIL`

**Статус:** OPEN
**Тип:** осознанно суженный остаток [GAP-SMIL](../ROADMAP.md) (закрыт как `done`, [BUG-806-FIXED](BUG-806-FIXED.md)) — задокументированное «вне скоупа» на практике даёт не «часть подтестов красная», а harness-`TIMEOUT` для всего файла.
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 53, `svg`)
**Область:** js — `crates/js/src/svg.rs::_lumen_smil_parse_begin_offset` (`crates/js/src/svg.rs:1013-1020`) — любая форма `begin`, кроме голого числа/`indefinite`/пустого, парсится в `null`, что рантайм трактует как «жди только явного `beginElement()`»; исходный SMIL-таймер — `crates/shell/src/lumen/smil.rs`
**Владелец:** P3 (тот же владелец, что закрывал GAP-SMIL).

## Симптом

38 из 41 «нехороших» top-level результатов прогона `svg` — `TIMEOUT`, не `FAIL` (harness ждёт событие, которое никогда не придёт, до истечения тайм-аута раннера — самая дорогая категория провала, ~4-6с на файл вместо обычных <1с):

```
begin-attribute-mutation.html, begin-event.svg, beginelement-instance-time-1.html,
beginevents-1.html, correct-events-for-short-animations-with-syncbases.html,
cyclic-syncbase-2.html, cyclic-syncbase-events.html, event-listeners.html,
eventbase-non-svg-element.html, first-interval-in-the-past-contribute.html,
interval-restart-events.html, invalid-attribute-type-does-not-block-begin-event.html,
repeatcount-attribute-mutation.html, repeatcount-numeric-limit.tentative.html,
repeatn-remove-add-animation.html, scripted/eventbase-after-removal.html,
scripted/keypoints-attribute-trailing-semi.html, scripted/keytimes-attribute-trailing-semi.html,
scripted/syncbase-after-removal.html, seeking-events-{1..8}.html, slider-switch.html,
syncbase-escaped-dots.html, embedded/image-crossorigin.sub.html,
interact/image-load-error-events.html, interact/script-load-error-events.html,
interact/scripted/defer-01.svg, linking/scripted/a.rel-noopener-policy.html,
linking/scripted/a.rel-noreferrer-policy.html, pservers/pattern-with-invalid-base-cloned-thcrash.html,
struct/scripted/autofocus-attribute.svg, struct/scripted/use-load-error-events.tentative.html
```

Подтверждено чтением: `cyclic-syncbase-2.html` использует `begin="c.end; b.begin"` (syncbase-цепочка), `begin-event.svg` — `begin="anim.beginEvent"` (event-based), `seeking-events-*.html` — числовой `begin` (`"5s"`/`"9s"`), но ждут `beginEvent`/`endEvent` реального другого элемента цепочки, которая не запускается без syncbase-поддержки. `_lumen_smil_parse_begin_offset` (`svg.rs:1013-1020`) возвращает `null` для любой из этих форм — по коду-комментарию это осознанно («только `beginElement()`/`beginElementAt()` может стартовать»), но раз ничто в тесте не зовёт `beginElement()` вручную, анимация не стартует НИКОГДА, `beginEvent` не диспатчится НИКОГДА, а тест ждёт этого события через `add_completion_callback`/`event_test`-подобный хелпер — итог не «саб-тест красный», а весь harness висит до тайм-аута раннера.

## Отличие от GAP-SMIL/BUG-806

`ROADMAP.md`/`BUG-806-FIXED.md` фиксируют это как сознательно суженный скоуп («вне скоупа: syncbase/event/repeat-формы begin») — премиса верна для страниц, где отсутствие фичи просто не даёт анимации случиться (обычный `FAIL` на конкретных assert'ах). Здесь другое: тест **структурно зависит** от того, что событие рано или поздно случится (иначе он не тест на функцию — он тест самого события), поэтому отсутствие поддержки превращает `FAIL` в `TIMEOUT` — вайп файла целиком (0 подтестов вместо частичного прохода) и самую дорогую по времени раннера категорию провала. Заводится отдельным номером, а не довеском к BUG-806-FIXED, потому что тот уже закрыт и практическая цена («сколько файлов реально виснет из-за этого решения») раньше не была измерена.

## Ожидание

Минимально — синхронный (не полный async event-driven) разбор `id.begin`/`id.end`±offset (syncbase) и `id.eventname`±offset (event-based `begin`) с построением зависимостей между анимационными элементами и стартом по цепочке, как того требует SMIL3 §Timing. Полная реализация — тот же объём работы, что и был сознательно вынесен GAP-SMIL за скоуп; даже частичная (например, syncbase без event-based) убрала бы часть из 38 TIMEOUT.

## Связанное

- [BUG-806-FIXED](BUG-806-FIXED.md) / `ROADMAP.md` GAP-SMIL — откуда взято решение о скоупе.
- `docs/tasks/p2-test-track.md#test-3-срез-53-2026-09-22`.

## Не проверялось

- 3 оставшихся `ERROR` (не `TIMEOUT`) top-level результата (`restart-never-and-begin-click.html`, `outer-svg-intrinsic-size-002.html`, `SVGAnimatedEnumeration-initial-values.html`) — 0/0 подтестов, детальный блок отчёта пуст (нет саб-тестовой таблицы), причина не разобрана в этом срезе.
