# BUG-1791 — `navigator.getBattery` бросает в конструкторе: Battery Status API не реализован, ya.ru-плеер падает

**Статус:** OPEN
**Заведён:** 2026-11-06 (P3, находка compat-crawl 10 сайтов)
**Область:** js (`navigator` bindings, Battery Status API — W3C Battery Status / NS Battery)

## Симптом

Прогон `scripts/compat_crawl.py` (Lumen vs Chrome for Testing 153, `--maximized`, `LUMEN_NO_ADBLOCK=1`), отчёт `.tmp/compat-crawl/20261106-170917/`:

- `https://ya.ru` — JS-исключение `Battery Status API is not supported` (стабильно в обоих прогонах) + a11y-покрытие 36/63.
- Исключение летит из конструктора Яндекс.Плеер SDK: он вызывает `navigator.getBattery()` без try/catch — в Chrome API есть и возвращает `Promise<Battery>`, у нас getter/функция отсутствует или бросает синхронно.

## Что сделать

По спеке Battery Status API (W3C): `navigator.getBattery()` → `Promise<Battery>` с `charging`, `chargingTime`, `dischargingTime`, `level` и событиями `chargingchange`/`levelchange`/`chargingtimechange`/`dischargingtimechange`. На десктопе реалистичные константы: `charging: true`, `level: 1`, `chargingTime: 0`, `dischargingTime: Infinity` — детерминированные значения в духе ADR-007 (anti-fingerprint слой).

Минимум для разблокировки ya.ru: не бросать в конструкторе SDK. Полный вариант — живой `BatteryManager` с событиями (дешёво: состояние статично, события можно не файрить или файрить при реальном изменении питания, если бэкенд есть).

## Проверка

- `python scripts/compat_crawl.py` на ya.ru — exception уходит, a11y должен подрасти (плеер доинициализируется).
- Ручной прогон: `lumen --maximized https://ya.ru` с `LUMEN_NO_ADBLOCK=1`, в консоли нет `Battery Status API`.

## Связи

- Compat-crawl отчёт: `.tmp/compat-crawl/20261106-170917/summary.md` (не коммитится, `.tmp/`).
