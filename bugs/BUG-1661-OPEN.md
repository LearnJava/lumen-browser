# BUG-1661 — модификаторы `url(… referrer-policy()|cross-origin()|integrity())` разобраны, но не применяются к запросу подресурса

**Статус:** OPEN
**Заведён:** 2026-10-21 (P3, остаток [BUG-1433](BUG-1433-FIXED.md))
**Область:** shell/net (`crates/shell/src/subresources.rs`; `BackgroundLayer`/`BackgroundImage::Url` не хранят модификаторы)

## Симптом

После BUG-1433 `background-image: url("…/image-referrer-policy.py?…" referrer-policy(origin))` грузится по правильному адресу, но
запрос уходит с политикой по умолчанию: `Referer`, режим CORS (`cross-origin(use-credentials)`) и проверка `integrity("sha…")`
игнорируются. Reftest-ы `css/css-values/urls/{referrer-policy,cross-origin,integrity}/*` (21 id, `thick`) проходят только когда
сервер видит ожидаемый `Referer`/CORS/SRI. `BackgroundImage::Url(String)` держит голый адрес; модификаторы остаются лишь в
`BackgroundLayer::image_css` (computed-запись).

## Что делать

Хранить `UrlModifiers` рядом с адресом (поле слоя или вариант `BackgroundImage::Url`), передавать их в загрузчик картинки фона
(`referrerpolicy` → заголовок `Referer`, `cross-origin` → режим CORS/credentials, `integrity` → SRI-проверка тела; при провале
картинка не рисуется). Смежное: [BUG-1235](BUG-1235-OPEN.md) (`referrer-policy/4K*`). Отдельно не проверено: `@import url() integrity(…)`
(`url-request-modifiers-import-parsing.sub.html`), `@font-face src:url() …` (`url-request-modifiers-font-face-parsing.html`), `mask-image`,
`cursor`, `list-style-image` (там модификаторы срезаются/игнорируются так же).

## Как проверить

`css/css-values/urls/referrer-policy/origin/url-image-referrer-policy-origin.html`, `urls/integrity/*`, `urls/cross-origin/*`.
