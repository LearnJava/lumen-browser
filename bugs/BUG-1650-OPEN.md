# BUG-1650 — один и тот же `.woff2` Google Fonts запрашивается 4–8 раз (вариативный шрифт × веса)

**Статус:** OPEN
**Заведён:** 2026-10-19 (Chat, повторная проверка после [BUG-1634](BUG-1634-FIXED.md))
**Область:** shell (`crates/shell/src/page_load.rs` — `requested_web_fonts`, `spawn_web_font_fetches`/`take_needed_web_fonts`)

## Симптом

`target/dev-release/lumen.exe` (сборка `main` 2026-10-19, `6003be54f`), `--maximized --proxy http://127.0.0.1:12334`
(системный прокси Windows), `LUMEN_NO_ADBLOCK=1`, `--mcp-live-port`, ~80 с:

- главная youtube: 153 запроса к `fonts.gstatic.com`, разных URL — 118;
- `/results?search_query=test`: 80 запросов, разных — 45; Roboto — 44 запроса, отдельные URL по 4–8 раз.

Пример (`.tmp/yt-probe.log`, поиск): `…/roboto/v51/KFO7CnqEu92Fr1ME7kSn66aGLdTylUAMaxKUBGEe.woff2` —
`→ GET` в строках 73, 82, 91, 100 (подряд, до первого ответа), затем ещё раз пачкой со строки 208.

## Вероятная причина

CSS `fonts.googleapis.com/css2?family=Roboto:wght@300;400;500;700` у Roboto v51 отдаёт **вариативный** шрифт:
четыре блока `@font-face` (wght 300/400/500/700) на каждый поднабор `unicode-range` ссылаются на **один и тот же
URL**. Дедупликация `requested_web_fonts` идёт по (семейство, вес, стиль, URL) — поэтому один файл качается
на каждый вес (×4). Вторая пачка (×8 итого) — вероятно, повторная проверка `deferred_web_fonts` в
`apply_relayout_result` или второй лист стилей с тем же `@font-face`; проверить.

В [BUG-1634](BUG-1634-FIXED.md) повторы отмечены как «не воспроизведены» — вживую воспроизводятся.

## Что сделать

1. Сетевую загрузку дедуплицировать по URL: один fetch на URL, результат раздаётся всем блокам
   `@font-face`, которые на него ссылаются (каждый регистрирует свой дескриптор веса/стиля поверх тех же байт).
2. Найти источник второй пачки и убрать.
3. Тест: фикстура с четырьмя `@font-face` (разные `font-weight`, один `src: url(x.woff2)`) — ровно один запрос.

Попутно (из «Не вошло» BUG-1634): фреймовый путь (`frames.rs`) и headless (`dump_mode.rs`) по-прежнему
грузят все поднаборы `unicode-range` — перенести туда тот же отбор `take_needed_web_fonts`.

**Готово, когда:** на youtube каждый URL `fonts.gstatic.com` запрошен один раз; тест зелёный.
