# BUG-1829 — фингерпринт-дивергенты toString-пробы BUG-1826: SAB без crossOriginIsolated, пустые прототипы WebGL/Crypto, нет RTCPeerConnection

**Статус:** OPEN
**Заведён:** 2026-11-19 (P6, toString-проба BUG-1826 срез 2 — `.tmp/stall/probe_native.py`)
**Компонент:** js (`crates/js/src` — шимы и нативы; v8_runtime — флаги изолята)

## Симптом

Проба 80 платформенных API на обоих движках (Lumen против CfT 157, одинаковая
машина) помимо главного дефекта BUG-1826 (71 функция отдают JS-исходник вместо
`[native code]`) нашла четыре структурные дивергенции, видимые анти-бот VM без
всякого `toString`:

1. **`SharedArrayBuffer`Exposed там, где Chrome его прячет.** В Lumen
   `typeof SharedArrayBuffer === 'function'` на обычной странице; в Chrome —
   `undefined` без `crossOriginIsolated` (Chrome 157 на той же странице).
   Гейтится ли в Lumen выдача под `cross_origin_isolated` (аргумент
   `install_dom`) — не проверено; сам глобал виден всегда.
2. **`WebGLRenderingContext.prototype.getParameter` отсутствует** (и в
   WebGL2-варианте): контекст `getContext('webgl')` в Lumen отвечает на
   `getParameter` (37446 → `"Generic GPU"`, 7936 → `"WebKit"`), но методы
   висят на инстансе, не на прототипе — `WebGLRenderingContext` как класс с
   прототипом не построен. Строки рендера синтетические (у Chrome на фоне —
   `null|WebKit`, на GPU-машине — ANGLE-строка); честная выдача — вопрос
   отдельной задачи.
3. **`Crypto.prototype` отсутствует**: `crypto.getRandomValues` работает,
   но интерфейсного класса нет (в Chrome `Crypto.prototype.getRandomValues`
   — функция). Тот же паттерн «методы на инстансе».
4. **`RTCPeerConnection` отсутствует** целиком (в Chrome — класс).
   WebRTC-фингерпринт (ICE-кандидаты) — один из базовых сигналов CF/капч.

Все четыре — прямой материал для скоринга анти-бот систем: отсутствие
интерфейса или лишний глобал отличают движок от Chrome детерминированно.

## Репро

```
py -3 .tmp/stall/probe_native.py both   # таблица LUMEN/CHROME по 80 API
```

## Что делать

- SAB: гейтить глобал (и `Atomics`-поведение) значением `cross_origin_isolated`
  контекста, как Chrome (проверить, что WPT-наборы Lumen не опираются на
  безусловный SAB).
- WebGL/Crypto: поднять интерфейсные классы с методами на прототипах (шимы
  уже умеют — тот же паттерн, что у `ReadableStream.prototype.pipeTo`).
- RTCPeerConnection: класс-заглушка с честным `createOffer`-фейлом или
  полноценный натив — по матрице WPT/фингерпринта; минимум — не
  `undefined`.

Связан с BUG-1826 (тот же потребитель — challenge-VM), но чинится независимо.
