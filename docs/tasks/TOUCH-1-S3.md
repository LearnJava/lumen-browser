# TOUCH-1-S3 — JS-диспатч касаний: `pointerType` и `_lumen_dispatch_touch_event`

Срез 3 из TOUCH-1. Владелец — P1. Крейт — `lumen-js`. Шим и тесты.

## Цель

1. `_lumen_dispatch_pointer_event` получает необязательные `pointerId`, `pointerType`, `isPrimary`, `width`, `height`, `pressure`. Значения по умолчанию — `1` / `'mouse'` / `true` и нынешние. Все существующие вызовы не меняются.
2. Новая функция `_lumen_dispatch_touch_event(start_nid, type, touches, changedTouches, targetTouches, mod)`.
   - Массивы — дескрипторы `{identifier, target_nid, clientX, clientY, radiusX, radiusY, force}`.
   - Из них собираются `Touch` / `TouchList` / `TouchEvent` с `isTrusted: true`, `bubbles: true`.
   - `cancelable` по Touch Events L2 §5: `touchcancel` — false, остальные — true.
   - Возвращает результат `dispatchEvent`, то есть отменён ли default action.
3. Не включать `ontouch*` и не менять `maxTouchPoints`: это решение пользователя (TOUCH-1-S5), см. комментарий в шиме :191-196.

## Точки входа

- [web_api_shim_mid.js:1471](../../crates/js/src/shim/web_api_shim_mid.js#L1471), жёсткое `pointerType: 'mouse'` на :1482.
- `Touch` / `TouchList` / `TouchEvent` — `web_api_shim_mid.js:190-384`.

## Не трогать

- Shell (S4/S5).
- Вызовы на :1407, :1430, :1531, :1563 — только если параметры добавляются туда без изменения поведения.

## Готово, когда

Новые V8-тесты:

- рядом с [v8_event_classes.rs:229](../../crates/js/src/dom/tests/v8_event_classes.rs#L229): `pointerType === 'touch'`, `pointerId === 2`, старый вызов по-прежнему даёт `'mouse'`;
- в `crates/js/src/dom/tests/v8_bug688_touch_events.rs`: слушатель на предке получает `touchstart` с правильными `touches.length`, `changedTouches[0].identifier`, `target`; `preventDefault()` возвращает «отменён»; `touchcancel` не cancelable.

Существующие тесты событий зелёные.

## Гейт

```
cargo clippy -p lumen-js --features v8-backend --all-targets -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/dump_golden.py
```

## Зависимости

Нет. Независим от S4.
