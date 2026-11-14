# BUG-1791 — `navigator.getBattery` бросает в конструкторе: Battery Status API не реализован, ya.ru-плеер падает

**Статус:** FIXED 2026-11-14
**Заведён:** 2026-11-06 (P3, находка compat-crawl 10 сайтов)
**Закрыт:** 2026-11-14 (P3)
**Область:** js (`navigator` bindings, Battery Status API — W3C Battery Status / NS Battery)

## Симптом

Прогон `scripts/compat_crawl.py` (Lumen vs Chrome for Testing 153, `--maximized`, `LUMEN_NO_ADBLOCK=1`), отчёт `.tmp/compat-crawl/20261106-170917/`:

- `https://ya.ru` — JS-исключение `Battery Status API is not supported` (стабильно в обоих прогонах) + a11y-покрытие 36/63.
- Исключение летит из конструктора Яндекс.Плеер SDK: он вызывает `navigator.getBattery()` без try/catch — в Chrome API есть и возвращает `Promise<Battery>`, у нас getter/функция отсутствует или бросает синхронно.

## Что было

`crates/js/src/battery_bindings.rs` — disable-стаб по ADR-007 Layer 4 (9D.4): `navigator.getBattery` перезаписывался функцией, возвращающей `Promise.reject(new Error('Battery Status API is not supported'))`. API высокоэнтропийный фингерпринт-источник, поэтому стаб и стоял — но rejected-промис без обработчика в конструкторе SDK убивал инициализацию плеера на ya.ru.

## Фикс

Стаб заменён детерминированным `BatteryManager` (реальное состояние питания не читается никогда — в духе ADR-007):

- `navigator.getBattery()` → `Promise<BatteryManager>`, промис один на навигатор (`[SameObject]`, Battery Status §5.2 — WPT `battery-promise` проверяет идентичность).
- `BatteryManager` — интерфейс на глобале, `Illegal constructor`, прототип от шимовского `EventTarget`; `charging: true`, `chargingTime: 0`, `dischargingTime: Infinity`, `level: 1` — readonly prototype-геттеры с бренд-чеком; `Symbol.toStringTag`.
- События `chargingchange`/`chargingtimechange`/`dischargingtimechange`/`levelchange` и атрибуты `on*` существуют, но не файрятся — состояние константно (иначе это была бы ложь о железе).
- Окно-scope, как у стаба: в `WorkerNavigator` `getBattery` по-прежнему нет (`shortcut:` воркеры — при первом живом пользователе).
- Ставится там же в `install_dom` (до finalize-прохода BUG-624, который уносит функцию на `Navigator.prototype`).

## Проверка

- `compat-crawl` ya.ru (`--env LUMEN_NO_ADBLOCK=1`, 2026-11-14): `lumen=OK`, в консоли ни одной JS-ошибки и ни одного упоминания Battery (до фикса — стабильный `Uncaught Error: Battery Status API is not supported`); страница 200, скрипты и лента грузятся. Плечо Chrome упало само (REF-FAIL, антибот) — a11y-сравнение этого прогона не считалось; пункт «a11y подрастёт» не перепроверен.
- 9 юнит-тестов `battery_bindings` (форма API, идентичность промиса, константы, readonly, Illegal constructor, EventTarget).

## Связи

- Compat-crawl отчёт: `.tmp/compat-crawl/20261106-170917/summary.md` (не коммитится, `.tmp/`); прогон проверки — `.tmp/compat-crawl/bug1791-verify/`.
