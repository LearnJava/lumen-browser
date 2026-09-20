# BUG-941 — у канвы нет флага origin-clean: cross-origin картинка рисуется и читается обратно, `getImageData`/`toDataURL` не бросают `SecurityError` никогда

**Статус:** OPEN (ДОРАБОТКА → [GAP-CANVASORIGIN](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — модели origin-clean в движке нет вовсе (`grep` по `crates/js` за `origin.clean`/`SecurityError` в `canvas2d.rs`/`offscreen_canvas.rs` даёт ноль), и она требует не одной правки, а сквозного состояния: режим запроса по атрибуту `crossorigin`, результат CORS-проверки ответа, распространение флага через `drawImage`/`createPattern`/`ImageBitmap`/`transferToImageBitmap` и проверка на трёх читающих членах. Ведётся как задача `GAP-CANVASORIGIN` в [ROADMAP.md](../ROADMAP.md); P3 как баг не берёт.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 30 — живой замер, вариант `canvas-taint-crossorigin`)
**Область:** js (`crates/js/src/canvas2d.rs` — `getImageData`/`toDataURL`/`drawImage`; `crates/js/src/offscreen_canvas.rs`), network (режим запроса и CORS-ответ до канвы не доходят — ср. [BUG-859](BUG-859-OPEN.md): исходящий запрос не несёт даже `Origin`)
**Владелец:** дорожка `GAP-CANVASORIGIN`.

## Симптом

Канва, в которую нарисовали картинку с ЧУЖОГО источника, читается обратно
полностью: `getImageData` возвращает пиксели, `toDataURL` — data-URL. По HTML
LS §4.12.5.1.2 такая канва перестаёт быть origin-clean и оба члена обязаны
бросить `SecurityError`.

Проверки нет ни в какой форме: атрибут `crossorigin` отражается
(`corsed.crossOrigin === "anonymous"`), но ни на что не влияет — CORS-ответа
никто не спрашивает, и результат одинаков с ним и без него.

## Прямое измерение

`tests/wpt/verify_replaced_content_gaps.py --variant canvas-taint-crossorigin`
(2026-09-01, dev-release, Linux, `main` = `287562e61`). Второй origin — второй
порт собственного сервера пробы; алиасы `www1.`, которыми это делает сам
прогон WPT, на машине не разрешаются (`WPT-RUN-10`), и мерить надо было бы
их, а не предмет:

```
crossOrigin-reflects = anonymous
same-draw = ok    same-read = 0,127,1,255      ← своя картинка, читается
cross-draw = ok   cross-read = 0,0,1,255       ← ЧУЖАЯ картинка, читается
cross-toDataURL = data:image/png;base64,       ← и сериализуется
[server saw: GET /media/1x1-green.png?taint=same,
             GET [alt]/images/black-rectangle.png?taint=cors,
             GET [alt]/images/black-rectangle.png?taint=cross]
```

Картинки написаны парсером намеренно: на момент этого среза скриптовая в стор
битмапов канвы не попадала вовсе ([BUG-938](BUG-938-FIXED.md), исправлено
2026-09-20), рисовала пусто, и через неё вопрос о загрязнении задать было
нельзя — первый замер этого среза так и вышел бессодержательным.

## Кого это держит

`svg/embedded/image-crossorigin.sub.html` — 4 сабтеста, две пары «можно
прочитать» / «нельзя прочитать», то есть файл проверяет обе стороны флага;
`html/canvas/element/manual/drawing-images-to-the-canvas/drawimage_svg_image_with_foreign_object_does_not_taint.html`
— отрицательная сторона (SVG с `<foreignObject>` НЕ должен загрязнять).
Оба из остатка WPT-RUN-5. Больший счёт — в невендоренных категориях
`html/canvas/*`, где это одно из основных правил.

## Почему это заявка на безопасность, а не на совместимость

Origin-clean — единственное, что мешает странице прочитать пиксели чужого
документа через канву (классический пример — приватная картинка, отданная по
cookie пользователя). Сегодня такая проверка отсутствует, поэтому в этой
части движок разрешает больше, чем любой браузер, а не просто отвечает не то.

## Срез 1 (2026-09-17, P1)

Origin-clean флаг заведён на `Context2D` (`crates/engine/canvas/src/lib.rs`:
`origin_clean`/`is_origin_clean()`/`taint()`, монотонный на весь срок жизни
канвы). Заражение проведено через все перечисленные в объёме пути:
`drawImage`/`drawImage` с обрезкой/`createPattern` (обе стороны —
`fillStyle`/`strokeStyle`) на элементной канве и на `OffscreenCanvas`,
`ImageBitmap` (`image_bitmap_from_img_nid_native` переносит флаг источника).
Все три читающих члена из объёма enforced: `getImageData` и
`toDataURL`/`toBlob` бросают `SecurityError` (`crates/js/src/shim/web_api_shim_mid.js`
для элементной канвы, `OffscreenCanvasRenderingContext2D.getImageData` в
`crates/js/src/offscreen_canvas.rs` — `convertToBlob` там пока не
реализован, гейтить нечего).

Кросс-origin детектится простым сравнением origin резолвленного `<img src>` с
`base.origin()` (`crates/shell/src/subresources.rs::fetch_and_decode_images`,
новый параметр `cross_origin_urls`) — **не** настоящей CORS-проверкой ответа:
`Origin` всё ещё не уходит ни на один сабресурс (BUG-859), поэтому
`crossorigin="anonymous"` по-прежнему ничего не меняет и не отличается от
его отсутствия. Консервативно и безопасно (чужой origin красит независимо от
намерения автора), но не полная модель — остаётся вторым срезом.

Не покрыто: запрос по атрибуту `crossorigin`/реальный CORS-хендшейк ответа
(BUG-859), поэтому статус задачи в ROADMAP остаётся `planned`, а не `done`.

## Срез 2 (2026-09-17, P1)

`<img crossorigin>` на cross-origin URL теперь идёт через реальный CORS-протокол
(Fetch §3-§4): `Origin`-header, проверка ответных `Access-Control-Allow-Origin`/
`-Allow-Credentials`. Использована уже существующая, но нигде не вызывавшаяся
инфраструктура `lumen_network::HttpClient::fetch_cors`/`cors::check_cors_response_headers`
(написана и покрыта тестами, но ни один caller её не вызывал — путь картинок шёл
мимо, через `fetch_subresource`, который `Origin` не шлёт вовсе). Новый путь:
`lumen_layout::ImageRequest::crossorigin` (парсинг `crossorigin` атрибута —
`Anonymous`/`UseCredentials`, HTML LS §2.5.1) → `crates/shell/src/subresources.rs`,
`decode_image_cors` — на cross-origin URL с `crossorigin` заданным, ACAO-проверка
пройдена → canvas не заражается; проверка не пройдена или сеть недоступна →
запрос ошибается целиком (`ImgOutcome::Skip`), как и обычная сетевая ошибка, без
tainted-но-видимого fallback. Без атрибута — прежнее консервативное поведение
(всегда taint на cross-origin URL).

Известный остаток: `fetch_cors`'а собственный Phase 0-лимит — credentials-режим
не решает, летят ли реально cookies (см. doc-комментарий `HttpClient::fetch_cors`);
не блокирует основной случай `crossorigin="anonymous"` без cookies. `decode_image_cors`
намеренно не проходит через `image_cache::IMAGE_CACHE` (та же картинка без
`crossorigin` на той же странице фетчится второй раз) — см. doc-комментарий
функции. GAP-REFERRER/BUG-859 остаётся открытым для всех остальных
сабресурсов — этот срез не трогает `fetch_subresource`. Статус задачи в ROADMAP
остаётся `planned`: живой WPT-повтор (`canvas-taint-crossorigin`) против сервера,
реально отдающего ACAO, не сделан в этом срезе — следующий шаг.

## Срез 3 (2026-09-17, P1)

Живой WPT-повтор среза 2: `canvas-taint-crossorigin` (`tests/wpt/verify_replaced_content_gaps.py`)
расширен третьим `<img crossorigin>` против alt-сервера, реально отдающего
`Access-Control-Allow-Origin` (не просто заголовок в комментарии — сервер
пробы). Первый прогон ложно показал `SecurityError` на прошедшей CORS-проверку
картинке — не регрессия движка, а баг самого пробника: все три `<img>`
рисовались на одну `<canvas id=c>`, а taint-бит монотонен на канву (HTML LS
§4.12.5.1.2), и более ранний непрошедший `cross`-draw уже заразил канву
необратимо. После переноса прошедшей-CORS пары на отдельную `<canvas id=c2>`
`corsed-read` возвращает пиксели без исключения — путь среза 2 подтверждён
живым прогоном, не только модульными тестами `crates/network`.

(Задокументировано ретроактивно в срезе 5 — сама правка вносилась в срезе 3,
но эта заметка не попала в этот файл тогда, только в `ROADMAP.md`.)

## Срез 4 (2026-09-17, P1)

Закрыт первый пункт остатка среза 2 — `fetch_cors`'а credentials-лимит.
Раньше cookie-jar (если подключён через `with_cookie_jar`) прикладывал
`Cookie`-header на actual-запрос независимо от `credentials_mode` — гейтился
только `SameSite`/Total-Cookie-Protection логикой самого jar-а, не режимом
CORS-запроса. Это означало, что `crossorigin="anonymous"` (дефолт,
`CredentialsMode::SameOrigin`) на cross-origin hop-е мог утечь cookie, если
у неё нет `SameSite=Strict/Lax` — обратное тому, что требует Fetch §4.7 шаг 3
("HTTP fetch"): credentials должны прикладываться ТОЛЬКО при `Include`.

Правка в `fetch_with_redirect` (`crates/network/src/lib.rs`): cookie-инъекция
на actual cross-origin запросе теперь дополнительно гейтится
`CredentialsMode::cross_origin_credentials()` — `Omit`/`SameOrigin` не
прикладывают `Cookie` вовсе, `Include` (`crossorigin="use-credentials"`)
прикладывает как раньше. Preflight (OPTIONS) credentials не несёт и без этой
правки — Fetch §4.8.1, отдельного гейта не требовалось. Same-origin запросы и
запросы без `cors_ctx` (обычная навигация/сабресурс) не затронуты — они
прикладывали cookies всегда, это верно и для настоящих браузеров.
Два новых теста в `crates/network/src/lib.rs`:
`fetch_cors_default_credentials_omits_cookie`,
`fetch_cors_include_credentials_sends_cookie`.

Остаток: GAP-REFERRER/BUG-859 (все прочие сабресурсы кроме `<img crossorigin>`)
— отдельная, самостоятельно ведущаяся задача (ROADMAP.md, GAP-REFERRER), не
часть объёма GAP-CANVASORIGIN. Живой WPT-повтор `canvas-taint-crossorigin`
против сервера с реальным ACAO уже сделан срезом 3 — эта строка была
copy-paste остатком от среза 2 и не обновлялась; исправлено срезом 5. Статус
задачи в ROADMAP остаётся `planned` до среза 5.

## Срез 5 (2026-09-17, P1) — ревизия объёма, закрытие

Ревизия объёма GAP-CANVASORIGIN целиком (не новый код): исходная заявка —
флаг origin-clean, режим запроса по `crossorigin`, реальная CORS-проверка
ответа, сквозное заражение через `drawImage`/`createPattern`/`ImageBitmap`/
`transferToImageBitmap`, `SecurityError` на трёх читающих членах, на
элементной канве и на `OffscreenCanvas` — реализовано целиком (срезы 1-4) и
живо подтверждено (срез 3). `OffscreenCanvas`/`transferToImageBitmap`
проверены отдельно: `crates/js/src/offscreen_canvas.rs` переиспользует тот же
`img_bitmap_store`/tainted-флаг, который заполняет элементная сторона —
источник заражения один, а не два расходящихся.

Найдены и исправлены две доки-дрейфа, накопившиеся за срезы 2-4:
- `crates/shell/src/subresources.rs` (doc-комментарий `fetch_and_decode_images`):
  утверждал, что credentials-режим не влияет на отправку cookies («Phase 0
  ограничение `fetch_cors`») — это было верно до среза 4, который как раз этот
  лимит закрыл; комментарий не обновили. Исправлено здесь.
- Строка «остаток» среза 4 в этом файле и в `ROADMAP.md` повторяла
  «живой WPT-повтор... не сделан», хотя срез 3 (до среза 4 по номеру, но
  landed раньше по времени) уже его сделал. Copy-paste из остатка среза 2.
  Исправлено здесь и в `ROADMAP.md`.

Живой WPT-регрессионный повтор именно среза 4 (credentials-лимит,
`crossorigin="use-credentials"` действительно шлёт `Cookie`, `anonymous` —
нет) НЕ добавлен в `verify_replaced_content_gaps.py`: сервер пробы — plain
HTTP (`http.server` на `127.0.0.1`), а cookie jar (`crates/storage/src/cookies.rs:677`)
требует `Secure` для `SameSite=None`, а `Secure`-cookie в принципе не
отправляется не-HTTPS-запросом (`cookies.rs:681`/`269`) — кросс-сайтовый
cookie в этом харнессе физически не может долететь ни при каком исходе
`credentials_mode`, поэтому такой повтор доказывал бы только «HTTP не
HTTPS», а не поведение среза 4. Проверено модульно и точечно —
`fetch_cors_default_credentials_omits_cookie`/`fetch_cors_include_credentials_sends_cookie`
(`crates/network/src/lib.rs`) бьют ровно по гейту `cross_origin_credentials()`
без сетевого слоя вокруг; для утверждения о самом сетевом коде этого
достаточно, TLS-стенд под один Set-Cookie-регрессион непропорционален.

**Итог:** канвовый объём GAP-CANVASORIGIN закрыт. GAP-REFERRER/BUG-859
остаётся открытым как отдельная задача (не сужает эту). Статус в ROADMAP —
`done`.
