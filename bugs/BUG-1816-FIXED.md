# BUG-1816 — reCAPTCHA v3: токен отвергался (`score: null`, `browser-error`) — `Origin`/`Referer`/`Sec-Fetch-Site` кадра брали документ-контекст СТРАНИЦЫ

**Статус:** FIXED 2026-11-15 (P3, срез 7 [BUG-1647](BUG-1647-FIXED.md))
**Заведён:** 2026-11-15 (P3, срез 6 [BUG-1647](BUG-1647-FIXED.md), после
[BUG-1815](BUG-1815-FIXED.md))
**Область:** shell (`crates/shell/src/frames/spawn.rs`, `FrameLoadEnv`) /
network (`HttpClient::with_document_context`) — документ-контекст
`fetch()`/`XMLHttpRequest`/`WebSocket`/`EventSource` под-документа

## Симптом

`grecaptcha.execute()` резолвился быстро и без ошибок в stderr, но выданный
токен Google не принимал: `verify.php` → `{"success":false,"score":null,
"error-codes":["browser-error","hostname-mismatch",…]}`.

## Что оказалось неверным в рабочей гипотезе

Гипотеза «Lumen тянет `api2/webworker.js`, Chrome — нет, значит бандл ушёл в
урезанную ветку сигналов» **опровергнута**. Chrome тоже создаёт этот Worker:

- `netlog` (`--log-net-log`, `--net-log-capture-mode=Everything`): запрос
  `api2/webworker.js?hl=…&v=…` есть (`.tmp/net1816.py`);
- в обоих браузерах Worker создаётся в anchor-кадре на ~90 мс и присылает в
  окно ровно одно сообщение `recaptcha-setup` (`.tmp/wm1816.py`,
  `.tmp/pw1816.py`).

Прежний вывод «Chrome не запрашивает webworker.js» — артефакт метода:
`Network.*` на **page**-таргете CDP не видит загрузку скрипта воркера (она
принадлежит worker-таргету). Урок вынесен в [`docs/probe-method.md`](../../docs/probe-method.md).

## Причина

`FrameLoadEnv` нёс провайдеры сети **страницы** (`env.fetch_provider` /
`ws_provider` / `sse_provider`) и `spawn_frame` отдавал их под-документу как
есть. Провайдер — это `HttpClient` с `document_context`, привязанным к URL
верхнего документа (`HttpClient::with_document_context`), и из него считаются
`Referer`, `Origin` (не-GET/HEAD) и `Sec-Fetch-Site`.

Значит, XHR из anchor-кадра (`https://www.google.com/recaptcha/api2/anchor…`)
на **свой же** origin уходил как запрос чужого документа:
`Origin: https://recaptcha-demo.appspot.com`,
`Referer: https://recaptcha-demo.appspot.com/…`,
`Sec-Fetch-Site: cross-site`. Google отвечал `403 Forbidden` (страница ошибки с
`vary: Sec-Fetch-Dest, Sec-Fetch-Mode, Sec-Fetch-Site`) на
`POST api2/reload` — токена в ответе не было, `execute` отдавал короткий
`HF…`, и verify отвергал его.

Живая форма дефекта (`.tmp/if1816.py`, страница на `127.0.0.1:P1` + iframe на
`localhost:P2`, XHR внутри кадра на свой origin): до фикса Lumen слал
`Origin`/`Referer` верхней страницы и `Sec-Fetch-Site: cross-site`, Chrome —
`Origin: http://localhost:P2`, `Referer: …/inner`, `same-origin`.

## Починено

`frames/spawn.rs` строит под-документу собственный клиент по образцу top-level
(`page_pipeline.rs`): `child_base.http_client_for_subresource_with_policy(…)` с
политикой реферера САМОГО ребёнка (`document_referrer_policy(&child_doc)`) и
его собственной CSP (`connect-src`/`worker-src`/`object-src`/`media-src`/
`element-src`), плюс общий `SharedPrefetchCache`. Из `FrameLoadEnv` поля
`fetch_provider`/`ws_provider`/`sse_provider` удалены — они больше не
переезжают в под-документы (у `about:blank`/`srcdoc` base наследуется от
родителя, поэтому документ-контекст остаётся правильным).

## Проверка

- `.tmp/if1816.py` (Lumen): `Origin: http://localhost:P2`,
  `Referer: …/inner`, `Sec-Fetch-Site: same-origin`;
- `.tmp/px1816.py` (Lumen, `--maximized`, `LUMEN_NO_ADBLOCK=1`):
  `POST /recaptcha/api2/reload` → `200 application/json` (было `403`);
- `.tmp/sc1815.py` (оракул стенда руками): `{"success":true,"score":0.9,
  "action":"examples/v3scores","error-codes":[]}` — как у Chrome;
- `python scripts/captcha_stand.py --arm lumen --only recaptcha-v3-score` →
  `пройдено`, балл v3 `0.9`;
- полный стенд: `recaptcha` `4/4` (`v3-score` + три `v2`), остальные вендоры
  без изменений относительно среза 6.
