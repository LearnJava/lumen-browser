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

## Известные расхождения (замер `ozon_probe` Lumen vs CfT 157, 2026-11-20)

- `fp`-payload `/abt/result`: Lumen ~60 KB JSON vs Chrome ~30 KB; `fp`-blob 55964 vs 25628 b64-символов. Внутри зашифрованного блоба (~OpenSSL `Salted__`) — точная причина не локализована; canvas dataURL у Lumen 49922 vs 35190 символов.
- `navigator.userAgentData.brands`: Lumen отдаёт 4 бренда включая `"Lumen"` и `"Google Chrome"`; CfT headless — `[Chromium, Not-A.Brand]`. Поле `formFactors` в `getHighEntropyValues` отсутствует, `uaFullVersion` = `157.0.0.0` вместо полного `157.0.8103.78`, `platformVersion` `10.0.0` vs `19.0.0`.
- `navigator.mediaDevices.enumerateDevices()`: Lumen отдаёт реальное железо ("Набор микрофонов (Realtek(R) Audio)"), Chrome — три устройства с пустыми label/deviceId.
- `RTCPeerConnection.prototype`: у Lumen-стаба нет `createDataChannel`, `createOffer` reject'ит `NotSupportedError`, отсутствуют `addStream`/`getConfiguration`/`ondatachannel`/`setConfiguration` и др.; `createOffer`/`createDataChannel` у Chrome работают.
- `matchMedia`: `(color-gamut:srgb)` false (у Chrome true), `(max-monochrome:0)` false (у Chrome true).
- `navigator.storage.estimate()`: нет `usageDetails`; `storage.persisted()` = true (Chrome false).
- `navigator.connection`: лишнее нестандартное поле `type:"wifi"`, `downlink:10` vs реальные ~1.45.
- WebGL `WEBGL_debug_renderer_info` UNMASKED_* отдаёт `WebKit | Generic GPU` вместо ANGLE-строки (расходится с `VENDOR`/`RENDERER`-параметрами).
- `performance.memory`: `totalJSHeapSize`/`usedJSHeapSize` — фиксированные правдоподобные значения вместо живых.
- IP-репутация: CfT 157 на том же IP тоже ловил 403/incident в части прогонов — возможен независимый сетевой фактор.

## План закрытия

1. Дожать перечисленные JS-расхождения (brands/formFactors, mediaDevices, RTC-стаб, matchMedia, storage, connection, WebGL UNMASKED).
2. Локализовать содержимое `fp`-блоба (перехват plaintext до шифрования или офлайн-расшифровка `Salted__`).
3. Compat-запись `ozon.ru → chrome_unbranded` добавлена в `compat_pack.json` (убирает токен `Lumen/x.y` и бренд `"Lumen"` из UA/Sec-CH-UA — self-identifying маркер для антибота).
4. Живой ретест после локальных проверок; при сохранении 403 на чистом Chrome с того же IP — закрыть как env/IP-репутацию.
