# BUG-1040 — Web Animations: именованные easing-ключевые слова аппроксимированы неверно, `transform: none` не сериализуется как матрица

**Статус:** OPEN
**Заведён:** 2026-09-08 (P2, WPT-RUN-7 срез 32 — перегенерация baseline `web-animations` после мержа BUG-530)
**Область:** `crates/js/src/shim/web_api_shim_tail_b.js` — `_wa_ease()` (именованные ключевые слова) и `_wa_lerp_transform()`/`_wa_compute_at_p()` (сериализация `transform`)
**Владелец:** P1/P3

## Как найдено

BUG-530 (влит в `main` 2026-09-08) впервые заставил `currentTime`-сеттер синхронно
применять интерполированный стиль (`_syncStyleAtCurrentTime`) — до этого сэмплирование
`anim.currentTime = t` вне RAF-цикла (штатный WPT-паттерн детерминированной выборки)
молча не красило кадр вовсе, поэтому многие тесты либо не проверяли то, что заявляли,
либо были синхронно заморожены на одном и том же (случайно совпадающем) значении с
обеих сторон сравнения. После мержа BUG-530 повторный `--update-expected` +
`--check` (дважды подряд, байт-в-байт идентичный результат) на `web-animations`
вскрыл 5 новых регрессий относительно baseline, записанного до мержа BUG-530 —
это не флак (тот же класс, что BUG-999/1003/1004/1005/1011/1038), а детерминированные,
воспроизводимые расхождения.

## Симптом 1 — именованные easing-ключевые слова (4 подтеста)

`animation-model/keyframe-effects/effect-value-transformed-distance.html`, четыре
подтеста «Linear-equivalent cubic-bezier keyframe easing applied to an effect with a
{ease, ease-in, ease-in-out, ease-out} does not alter the result» сравнивают ширину
двух анимаций с идентичным `timing.easing = <именованное ключевое слово>`, отличающихся
только тем, что у одной на первом кадре явно проставлен `easing:
'cubic-bezier(0,0,0,0)'` (математически тождественная функция) — по спеке результат
должен совпадать.

`_wa_ease()` (`web_api_shim_tail_b.js:3120`) реализует именованные ключевые слова как
грубые квадратичные приближения, а не через контрольные точки настоящих кривых
(CSS Easing Functions Level 1 §2.1):

```js
if (easing === 'ease-in')  return t * t;
if (easing === 'ease-out') return t * (2 - t);
if (easing === 'ease' || easing === 'ease-in-out') return t < 0.5 ? 2*t*t : -1+(4-2*t)*t;
```

Спековые контрольные точки: `ease` = `cubic-bezier(.25,.1,.25,1)`, `ease-in` =
`cubic-bezier(.42,0,1,1)`, `ease-out` = `cubic-bezier(0,0,.58,1)`, `ease-in-out` =
`cubic-bezier(.42,0,.58,1)` — ни одна не сводится к `t*t`/`t*(2-t)`/кусочной параболе.
Пример измеренного расхождения (`ease-in`, `sampleTime=250` из 1000): ожидание
`7.765881px` (настоящая cubic-bezier(.42,0,1,1) в точке 0.25 через решатель Ньютона
той же `_wa_ease()`, применённый по кубик-безье-ветке), фактически `6.25px`
(=100×0.25² — ветка `t*t`). Раньше это расхождение не проявлялось синхронно (BUG-530),
поэтому тест либо не запускался по существу, либо сравнивал одинаково замороженные
значения по обе стороны.

Кандидат-фикс: заменить хардкод в `_wa_ease` на реальные контрольные точки (свести
`ease`/`ease-in`/`ease-out`/`ease-in-out` к вызову той же cubic-bezier-ветки решателя
Ньютона с соответствующими `p1x,p1y,p2x,p2y`), а не отдельную аппроксимацию.

Второстепенная находка на том же файле: даже пара «идентичных по математике»
cubic-bezier-строк (`cubic-bezier(0,0,0,0)` против неявного `linear`) для `ease`/
`ease-in-out` расходится в шестом знаке (`12.500164px` vs `12.5px`) — решатель Ньютона
(8 итераций, `web_api_shim_tail_b.js:3133-3139`) не сходится к биту, что при
строковом `assert_equals` в WPT ломает точное совпадение. Отдельный, более мелкий
дефект (не приоритет по сравнению с симптомом 1) — увеличить число итераций/затравку
или сравнивать по эпсилону там, где это возможно.

## Симптом 2 — `transform: none` не сериализуется как матрица (1 подтест)

`animation-model/animation-types/accumulation-per-property-002.html`, подтест
`transform: none`:

```
assert_regexp_match: Actual value is not a matrix
  expected object "/^matrix(?:3d)*\(.+\)/" but got "none"
```

Спека (CSS Transforms / Web Animations «animation-types» accumulation) требует, что
`getComputedStyle().transform` для анимированного/накопленного значения сериализуется
в матричной форме (`matrix(...)`/`matrix3d(...)`), даже когда результат аккумуляции
математически равен тождественному преобразованию — ключевое слово `none` для
computed-value в контексте активной анимации не является валидной сериализацией.
Кандидат — `_wa_lerp_transform`/сериализатор transform в `_wa_compute_at_p`
(`web_api_shim_tail_b.js:3196+`): ветка `if (from === 'none' && to === 'none') return
'none';` (или аналогичный short-circuit для накопления с идентичным результатом)
возвращает буквальную строку `'none'` вместо матричной формы.

## Воспроизведение

```
LUMEN_PROFILE=dev-release tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py \
  --binary target/dev-release/lumen.exe --all --root web-animations --recursive --check
```

Оба симптома воспроизведены дважды подряд байт-в-байт идентично (2026-09-08, срез 32) —
не флак. Текущий `.ini`-baseline сужен `expected: FAIL` для всех 5 подтестов (P2,
методология WPT-RUN-7: движковый пробел = expected:FAIL + BUG-NNN, вендоренные тесты не
ослабляются).

## Что не проверялось

Не проверено на других категориях/тестах, использующих именованные easing-ключевые
слова синхронно вне RAF (симптом 1 может проявляться шире `web-animations`, включая
CSS Transitions/`transition-timing-function`, если там используется тот же `_wa_ease`
или его аналог — не искал). Не проверено, есть ли у `_wa_lerp_transform` тот же
короткий путь `'none'` для non-accumulation композиции (`replace`/`add`), только
`accumulate` подтверждён.
