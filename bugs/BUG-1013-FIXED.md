# BUG-1013 — `FontFace.load()` блокировал JS-поток на весь сетевой round-trip шрифта

**Статус:** FIXED 2026-09-06
**Крейт:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_font_face_try_one_source`;
`crates/js/src/shim/web_api_shim_mid_b.js` — `_lumen_fetch`)
**Найден:** P1, 2026-09-06, при разборе прогона корпуса top-100 foreign (`google.com`,
`ready_s = 139.5`, CPU всего 6.2 с — то есть ожидание, а не вычисления)

---

## Симптом

`google.com` в живом окне: белый экран, `broken_render`, первый непустой кадр
на **139.5 с** (`[bench] first non-empty frame: 139551ms`). Страница при этом
полностью распарсена и свёрстана (518 DOM-узлов, 253 бокса) — отрисовано 0 единиц.

## Механизм

Инлайновый скрипт в `<head>` самого google.com (до `<title>`):

```js
var w = ["Google Sans", [400, 500, 700]];
… document.fonts.load(c + " 10pt " + b).catch(function(){})
```

Цепочка, по которой это уходило в сеть **синхронно**:

1. `FontFaceSet.prototype.load` — находит члены сета по family, форсирует `.load()` каждому;
2. `FontFace.prototype.load` → `_lumen_font_face_try_sources` → `_lumen_font_face_try_one_source`;
3. там был **`fetch(src.value)` без `AbortSignal`**;
4. в `_lumen_fetch` выбор транспорта был `useAsync = fetchSignal && !fetchSignal.aborted && !(_timeoutMs > 0)`
   — то есть воркер-поток брался, только если вызывающий передал сигнал. Без сигнала —
   `_lumen_fetch_sync`;
5. `_lumen_fetch_sync` (`crates/js/src/v8_runtime/install/net.rs`) — блокирующий
   `provider.fetch_request` прямо на JS-потоке, **без таймаута**;
6. всё это внутри фазы `run-scripts` загрузочного конвейера
   (`crates/shell/src/page_pipeline.rs`), то есть до `layout` и `paint`.

Итог: пока `fonts.gstatic.com` не ответит, движок стоит целиком. Хост с этой локации
периодически подвисает на ~135 с (Chromium в одном прогоне из 14 упёрся ровно в ту же
стену — 136 с, но отрисовал сразу фолбэк-шрифтами и этого не показал).
Google Sans 400/500/700 × 2 `unicode-range` = до 6 таких запросов подряд.

**Почему всплыло именно между прогонами 09-04 и 09-06:** до FONTLOAD-1/3/4 `document.fonts`
был пуст, `FontFaceSet.load` не находил ни одного члена и резолвился мгновенно. Как только
сет стал заполняться из CSS-`@font-face`, каждый вызов пошёл в сеть.

Загрузка при этом ещё и **дублировалась**: те же url() уже качает фоновый поток шелла
(`crates/shell/src/page_load.rs`, FOUT-подмена через `LoadEvent::FontLoaded`) — на рендеринг
синхронный fetch не влиял вовсе, только держал кадр.

## Проба (стенд, локальный сервер отдаёт woff2 через 8 с)

`lumen --trace-nav`, три страницы с одним и тем же `@font-face`:

| страница | `run-scripts` | `navigation` |
|---|---|---|
| только `@font-face url()`, без скрипта | 0.0 мс | 20.7 мс |
| то же + `document.fonts.load()` | **8122 мс** | **8147 мс** |
| `fetch(url, {signal})` — асинхронный транспорт | 110 мс | 129 мс |

Разница между строками 1 и 2 — ровно один вызов `document.fonts.load`.
Строка 3 показывала, что рабочий асинхронный путь в шиме уже есть.

Асимметрии headless/live нет — headless блокировался так же; предыдущему
прогону `--trace-nav` просто повезло с быстрым gstatic.

## Фикс

1. `_lumen_fetch` получил явный внутренний opt-in `init._lumenAsync`: воркер-путь
   (`_lumen_fetch_async_*`) теперь выбирается либо по живому не-таймаутному сигналу,
   как раньше, либо по этому флагу. Три обращения к `fetchSignal` внутри асинхронной
   ветки обёрнуты проверкой на его наличие — раньше они полагались на `catch`,
   глотающий `TypeError` на `undefined`.
2. `_lumen_font_face_try_one_source` зовёт `fetch(src.value, { _lumenAsync: true })`.

Флаг сделан **точечным opt-in, а не поведением по умолчанию**: остальные вызывающие
`fetch()` в движке рассчитывают, что ответ уже в руках в момент создания промиса, а
headless-режимы (`--screenshot`/`--trace-nav`/`--dump-*`) вообще не прокачивают таймеры —
асинхронный промис там просто никогда не дорезолвится. Для шрифтов это безопасно:
рендерингом занимается фоновый загрузчик шелла, а не этот промис.

## Проверка

- Новый регресс-тест `font_face_load_does_not_block_the_js_thread`
  (`crates/js/src/dom/tests/v8_fontface_shadow_custom.rs`): провайдер стоит 1500 мс,
  ассерт — что `.load()` вернул управление меньше чем за половину этого срока, и что
  промис затем резолвится с прокачкой `_lumen_tick_timers`/`_lumen_drain_microtasks`.
  A/B подтверждён: с откаченным фиксом тест падает с
  `FontFace.load() parked the JS thread for 1.5115901s`.
- Стенд выше на пересобранном бинаре: `run-scripts` 8122 мс → **104 мс**,
  `navigation` 8147 мс → **126 мс**.
- `cargo test -p lumen-js --features v8-backend` — 3515/3516; единственный провал
  `native_binding_panic_does_not_abort_process` — предсуществующий
  [BUG-997](BUG-997-OPEN.md), не связан.
- `cargo clippy -p lumen-js --features v8-backend --all-targets -- -D warnings` — чисто.

## Оставшийся объём (не этим багом)

- **Дедлайна у web-шрифта по-прежнему нет.** Chromium подменяет фолбэком по
  `font-display`-таймеру (~3 с); у нас промис просто висит, сколько нужно хосту.
- **`_lumen_fetch_sync` остался без таймаута вовсе** — любой синхронный `fetch()` со
  страницы всё ещё может встать намертво и повесить конвейер. Это отдельный, более
  широкий предохранитель.
- **Первый кадр по-прежнему ждёт конца всего конвейера** — инкрементальный рендер
  (как у Chromium) снял бы весь класс «белый экран из-за одного медленного ресурса».
- Побочно замечено в `_lumen_font_face_try_sources`: при провале последнего источника
  `err || lastErr` всегда выбирает обобщённое `NetworkError: No valid font sources`,
  заслоняя настоящую причину (например `SyntaxError` от невалидных байт). На поведение
  страницы не влияет, но диагностику портит.
