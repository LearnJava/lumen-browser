# BUG-439 — activation behavior не реализовано: клик, синтезированный в JS, не отправляет форму и не активирует ссылку

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `_lumen_dispatch_rich` / живой `dispatchEvent`), shell (вся активация живёт в `Lumen::handle_click_at`, JS-слой в неё не входит)
**Найден:** 2026-07-29, P1, при разборе [BUG-437](BUG-437-FIXED.md) — третий пункт исходной заявки, подтверждён отдельным прогоном

## Симптом

Клик, целиком сформированный в JS, доставляется слушателям, но не запускает
*activation behavior* элемента (HTML LS §6.10 «activation behaviour»):

```js
document.getElementById('btn').dispatchEvent(
    new MouseEvent('click', {bubbles: true, cancelable: true}))
```

по `<button type="submit">` внутри `<form>`:

- `dispatchEvent` возвращает `true`, обработчик `click` на кнопке **срабатывает**;
- событие `submit` на форме **не диспатчится**, отправка не начинается,
  `location.href` не меняется.

Замерено на живом окне (`--mcp-live-port`) после починки BUG-437: `#log` получает
`"clicked"` (сработал обработчик клика) и остаётся им — `"submitted"` не наступает.

## Причина (по коду, не проверено правкой)

Активация в браузере целиком реализована в шелле: `Lumen::handle_click_at`
классифицирует попадание (`forms::classify_click`) и уже сам выполняет
отправку формы / переход по `<a href>` / переключение чекбокса. JS-слой в эту
цепочку не входит вообще: `_lumen_dispatch_rich` только обходит реестр
`_lumen_listeners` и возвращает `!defaultPrevented`. Поэтому любое событие,
родившееся в JS, не имеет пути к шелловской активации.

То же по построению касается `<a href>` (синтетический клик по ссылке не
навигирует) и чекбоксов — проверялась только форма.

## Ожидалось

`dispatchEvent` события `click` с `cancelable: true`, не отменённого
обработчиками, выполняет activation behavior целевого элемента: для
`<button type=submit>` — отправку формы (через тот же путь, что и нативный клик,
включая шаг 11 с событием `submit`), для `<a href>` — навигацию.

## Смежное

- [BUG-437](BUG-437-FIXED.md) — нативный клик по submit-кнопке (починен: шаг 11 теперь выполняется).
- [BUG-383](BUG-383-OPEN.md) — в шиме нет `element.click()`, `form.submit()`, `form.requestSubmit()`.
- [BUG-360](BUG-360-OPEN.md) — живой диспатч читает только `addEventListener`, `on<type>`-атрибуты мертвы.

## Обновление 2026-07-29 (BUG-383)

Машинерия появилась: [BUG-383](BUG-383-FIXED.md) добавил
`_lumen_run_activation_behavior(nid, el)` в шим — поведение активации для
checkbox/radio (`input`+`change`), `<input type=submit|image>` и
`<button type=submit>` (через `form.requestSubmit()`), `type=reset`,
`<a href>`/`<area>`, `<summary>` и `<label>`. Но вызывает её только
`HTMLElement.prototype.click()`; `dispatchEvent(new MouseEvent('click', …))`
по-прежнему проходит мимо, потому что `_lumen_dispatch_rich` — это чистая
доставка события и шага «run activation behavior» в нём нет. Остаток бага
сузился до одного места: после доставки `click` через живой `dispatchEvent`
нужно вызвать ту же `_lumen_run_activation_behavior`, если событие не было
отменено и `isTrusted === false`.
