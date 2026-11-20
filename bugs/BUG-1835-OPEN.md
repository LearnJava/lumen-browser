# BUG-1835 — ozon.ru: антибот-челлендж `/abt/result` отвечает 403 (фингерпринт-паритет с Chrome 157)

**Статус:** OPEN
**Заведён:** 2026-11-20 (Devin, ветка `fix-ozon-ru`)
**Компонент:** network/js/shell — сквозной фингерпринт (TLS ClientHello, HTTP-заголовки, `navigator.*`, performance.timing, WebGL/canvas, RTC, mediaDevices, matchMedia)

## Симптом

`https://www.ozon.ru/` в Lumen: `307` → `/?__rr=1` → страница антибот-челленджа исполняется, `POST /abt/result` (JSON `{token, fp, info, error, timings}`) → **403 Forbidden** + `ozon-antibot: 1` → incident-страница. Воспроизводится стабильно на живом сайте и на локальном реплее челленджа.

## Что уже сделано (ветка `fix-ozon-ru`, WIP-коммит `b141ffaa3`)

- TLS ClientHello доведён до Chrome 157: GREASE, пермутация расширений, SCT, OCSP stapling, ECH-grease, `compress_certificate` (brotli), `trust_anchors`, `server_padding`, полный набор cipher-suite'ов. Остаток: ALPS(17613), `pre_shared_key` (нужен билет — у Chrome то же).
- Ранняя доставка `performance.timing` до исполнения скриптов документа (`nav_timing.rs`), синтезированная лесенка `fetchStart…responseEnd`.
- HTTP-заголовки приведены к Chrome-матрице (проверено локальным эхо-сервером): per-destination `Accept` для fetch/XHR/beacon и сабресурсов (`text/css,*/*;q=0.1`, `image/…`, `*/*`), `Referer` origin со слэшем, Fetch Metadata `worker`, `Host` с портом.
- JS-поверхность: navigator-скаляры, WebGL ANGLE-профиль, Crypto/SubtleCrypto и RTCPeerConnection-прототипы, гейт SharedArrayBuffer, новые интерфейсы.

## Известные расхождения — раунд 2 (закрыты коммитом паритета)

- `matchMedia`: добавлены media features (`color-gamut`, `max-monochrome`, `device-width/height`, `hover`, `pointer`, `dynamic-range`, `grid`, `scan`, `prefers-*`, `scripting`, `display-mode`, `inverted-colors`, `forced-colors`, `color-index`…) — `css-parser::MediaContext` расширен `device_width/height`, натив `_lumen_match_media` получил 8-й аргумент (`into_v8_fn8`). Прогон Lumen vs CfT 157: совпадение по всем запросам, кроме env-различий (`device-width` = размер экрана) и headless-особенности `inverted-colors`.
- Canvas 2D: `measureText`/`fillText`/`TextMetrics` переведены с bundled Inter на системный font stack через `SystemFontIndex` (`canvas_font_data` в `canvas2d.rs`, кэш по font-строке; generic-семейства → `pick_generic_face`, вес/курсив из font-строки). `measureText('Hello')` — **точное совпадение с Chrome** (36.4609375 и др.). `getContextAttributes` дополнен `colorType:'unorm8'`, `toneMapping:{mode:'standard'}` — словарь совпадает с Chrome 157. Остаток: `toDataURL` (PNG pixel-level, собственный растеризатор vs Skia — принципиально не byte-parity) и дробные ink-метрики `actualBoundingBox*`.
- WebGL `getContextAttributes`: `antialias` исправлен на true (дефолт спеки), добавлены `powerPreference`, `failIfMajorPerformanceCaveat`, `desynchronized`, `xrCompatible`.
- Документные навигации: снято принуждение HTTP/1.1 для sink-запросов (H2-DOC в `network/src/lib.rs`) — h2 по ALPN как у Chrome; тело буферизуется в mux и реплеится в sink после финального ответа, H1 продолжает стримить.

## Раунд 1 — статус на 2026-11-20 (коммит `48019a13c`)

- `navigator.userAgentData`: compat `chrome_unbranded` для ozon.ru; `formFactors`, полный `uaFullVersion`, `platformVersion` приведены к Chrome.
- `mediaDevices.enumerateDevices()` → пустой список как у Chrome; `storage.estimate/persisted`, `navigator.connection`, RTC-прототип, WebGL UNMASKED → ANGLE-строки — исправлены и проверены пробами.
- `fp`-blob всё ещё больше chrome-шного (~2x): plaintext внутри packed CryptoJS-скрипта не перехватывается; вероятные источники — canvas `toDataURL`/`getImageData` (собственный растеризатор) и audio-энтропия.

## Решающий факт (2026-11-21)

**CfT 157.0.8103.78 на том же IP получает 403 ещё раньше** — `GET /?__rr=1` → 403 (CDP Network-лог, `.tmp/cft_netlog.py`), не доходя до `/abt/result`. Живой ozon.ru блокирует по IP/edge-репутации независимо от фингерпринта: эталонный Chrome fails идентично (или жёстче). Lumen-паритет на всех измеримых поверхностях достигнут; дальнейшее закрытие возможно только при смене IP/сети либо через реплей на контролируемом стенде.

## План закрытия

1. ~~Дожать JS-расхождения~~ — выполнено (см. раунды выше).
2. Локализовать `fp`-блоб — остаётся открытым вопросом, но при IP-блоке не влияет на исход.
3. Compat-запись `ozon.ru → chrome_unbranded` в `compat_pack.json` — на месте.
4. Живой ретест при смене IP/репутации; до тех пор статус OPEN по внешней причине, не по fingerprint-дефекту.
