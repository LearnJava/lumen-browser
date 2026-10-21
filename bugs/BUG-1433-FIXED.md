# BUG-1433 — `url("…" referrer-policy(…)|cross-origin(…)|integrity(…))` — модификаторы остаются внутри строки URL; запрос идёт по испорченному адресу

**Статус:** FIXED 2026-10-21 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images` + `css/css-values` + `css/css-color`)
**Область:** css-parser/layout/shell (`crates/engine/layout/src/style/parse/` — разбор `url()` с `<request-url-modifier>`; `crates/shell/src/subresources.rs`)

## Симптом

`--dump-display-list`, `<div style="background-image:url('g.png' referrer-policy(origin))">`:

```
Пропуск картинки g.png' referrer-policy(origin): …\g.png' referrer-policy(origin) Не удается найти …
DrawBackgroundImage (0,40,20,20) src="g.png' referrer-policy(origin)" size=Auto …
```

`getComputedStyle(el).backgroundImage` для `url("a.png" referrer-policy(origin))` — `url("a.png\" referrer-policy(origin)")`
(модификатор внутри строки). Для `cross-origin(anonymous)` и `integrity("sha256-…")` — то же. Без модификатора
(`url(g.png)`) — `DrawBackgroundImage … src="g.png"`, всё верно.

В `urls/referrer-policy/*/url-image-referrer-policy-*.html` страница ставит `background-image: url("…/image-referrer-policy.py?…" referrer-policy(<p>))`;
сервер отвечает зелёной картинкой только если `Referer` соответствует политике. Из-за испорченного URL картинка не грузится — бокс синий.

## Как найдено

WPT-RUN-14 срез 19: 21 reftest в `css-values/urls/{cross-origin,integrity,referrer-policy/*}` (`thick`) и
`url-request-modifiers-{computed.sub,serialize.sub,invalid.sub,import-parsing.sub,font-face-parsing}.html` (13 testharness с
`urls th`). Пробой подтверждён разбор в `--dump-display-list`; применение политик (`Referer`, CORS, SRI) не проверялось —
вероятно, не реализовано ([BUG-1235](BUG-1235-OPEN.md) — смежная инфраструктура `referrer-policy/4K*`).

## Что делать

Разбирать `<url>` = `url(<string> <modifier>*)` и `src()`; хранить модификаторы; передавать политику реферера/режим CORS/
`integrity` в загрузчик подресурса; невалидный модификатор — отвергать декларацию.

## Как проверить

`css/css-values/urls/referrer-policy/origin/url-image-referrer-policy-same-origin.html`, `url-request-modifiers-computed.sub.html`.

## Исправление

Корень: `parse_url_value` ([`image.rs`](../crates/engine/layout/src/style/parse/image.rs)) брала всё между `url(` и последней `)` и срезала кавычки с краёв, поэтому хвост `' referrer-policy(origin)` оставался частью адреса. Теперь `parse_url_with_modifiers` разбирает `url(<string> <request-url-modifier>*)`: после закрывающей кавычки идут `cross-origin(anonymous|use-credentials)`, `integrity("…")`, `referrer-policy(<8 ключевых слов>)`; повтор известного модификатора, лишние токены в аргументах, число/строка вместо ident/функции и модификаторы после `url(<url-token>)` без кавычек отвергают значение (declaration отбрасывается); неизвестные модификаторы игнорируются, но их повтор — ошибка. Адрес в `BackgroundImage::Url` — без модификаторов; computed (`image_css`, `image_computed.rs`) сериализует их в каноническом порядке `cross-origin → integrity → referrer-policy`.

Тесты: `url_request_modifiers` (`image_computed.rs`), `background_image_url_with_request_modifiers` (`style/tests/images.rs`).

Не сделано, заведено отдельно: применение политик к самому запросу (`Referer`, CORS, SRI) — [BUG-1661](BUG-1661-OPEN.md).
