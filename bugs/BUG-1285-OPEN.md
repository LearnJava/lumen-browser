# BUG-1285 — mixed-content под wptrunner не измеряется: `http://*.localhost` для движка a priori authenticated, и swap-scheme редирект получает `upgraded` вместо `blocked`

**Статус:** OPEN
**Тип:** тулинг WPT (выбор хоста стенда), не дефект движка — см. «Почему не движок».
**Заведён:** 2026-10-05 (P2, разбор `mixed-content` после WPT-RUN-9; в `docs/wpt/pass-rate.md` §Третья полная цифра
записано «бага в этой сессии не заведено»)
**Область:** `tools/wptrunner/wptrunner/browsers/lumen.py::env_options` (`"browser_host": "localhost"`, WPT-RUN-10)
против `crates/network/src/origin.rs::is_loopback_host` (`localhost` и `*.localhost` — potentially trustworthy).

## Симптом

Перепрогон 189 шардов с исправленным сертификатом (BUG-1271, `.tmp/wpt-run9-merged/mixed-content.json`, слот `p2-work`):
`mixed-content` 265.12 → 232.47, 149 id вниз, 0 вверх. Сабтесты по форме «Expects X for <subresource> to <origin> and
<redirection> redirection from https context» (из 2 156 сабтестов категории):

| ожидание | origin | redirection | PASS | FAIL | фактически |
|---|---|---|---:|---:|---|
| blocked | cross-http | swap-scheme | 73 | **137** | `upgraded` |
| blocked | same-http | swap-scheme | 75 | **131** | `upgraded` |
| blocked | cross/same-http | no-redirect, keep-scheme | 766 | 0 | — |
| upgraded | *-http-downgrade | любая | 0 | **104** | `blocked` |
| allowed | same-https | no-redirect, keep-scheme | 330 | **156** | `blocked` |

Затронутые файлы swap-scheme: `fetch`/`xhr` (по 58), `beacon`, `link-css-tag`, `link-prefetch-tag`, `object-tag`,
`script-tag`, `worker-import-data` (по 18), `picture-tag` 14, `sharedworker-import-data` 12, `worker-import` 10,
`sharedworker-import` 8. У worklet-ов swap-scheme зелёный — они на этом пути ничего не грузят.

## Механизм (по логу прогона)

`common.sub.js:1119` при `checkScheme` ставит `action=check-scheme` для http-origin, а `subresource.py:137` пишет в
stash `upgraded`, если запрос **дошёл до сервера по https**. Swap-scheme (`subresource.py::create_url`) отвечает 301 с
http-порта на `https://<тот же хост>:<https-порт>`. В `.tmp/wpt-run9-sub/mixed-content.log`:

```
→ GET http://www1.localhost:20300/common/security-features/subresource/xhr.py?redirection=swap-scheme&action=check-scheme&…
← 301 http://www1.localhost:20300/…
→ GET https://www1.localhost:20443/common/security-features/subresource/xhr.py?action=check-scheme&…
```

То есть первый хоп `http://…` с https-страницы **ушёл в сеть**, редирект привёл на https, и сервер честно записал
`upgraded`. Тест ждёт, что браузер не отправит http-запрос вовсе (`blocked`). Во всём логе категории — 1 115
исходящих `GET http://*localhost:20300/…/subresource/*.py` и **ноль** событий `RequestBlocked` с причиной
`mixed-content` (единственные `✗` — 43 × `read: EOF before status line` на `http://…:20443`, то есть http на
https-порт, сценарий `*-downgrade`).

Почему http ушёл: `MixedContentPolicy::evaluate` → `classify_subresource_request` → `is_authenticated_url`
(`crates/network/src/mixed_content.rs:147`) считает URL a priori authenticated, если его origin potentially
trustworthy, а `Origin::is_potentially_trustworthy` (`origin.rs:135`) → `is_loopback_host` (`origin.rs:177`) возвращает
`true` для `localhost` и любого `*.localhost` (Secure Contexts §3.1, RFC 6761 §6.3). С WPT-RUN-10
(`browser_host = "localhost"`) **все** «insecure» origin стенда — `http://localhost:…` и `http://www1.localhost:…` —
для движка не mixed content. Это закреплено тестом `fetch_allows_trustworthy_http_url_with_mixed_content_policy`
(`crates/network/src/lib.rs:12114`, `http://127.0.0.1` при политике Strict — разрешён).

Enforcement на редирект-хопе при этом есть и работает: `fetch_with_redirect` проверяет политику на каждом хопе
(`lib.rs:2552`), тест `fetch_subresource_blocks_on_redirect_hop_to_http` (`lib.rs:12067`) блокирует hop
`https → http://<не loopback>`. Гипотеза из `pass-rate.md` «движковая находка про mixed-content на редирект-хопе»
пробой не подтверждается: хоп не блокируется только потому, что цель — loopback. Проверено на `main` 5221d4f6f:
`cargo test -p lumen-network --profile dev-release --lib -- fetch_subresource_blocks_on_redirect_hop_to_http
fetch_allows_trustworthy_http_url_with_mixed_content_policy fetch_blocks_non_trustworthy_http_url_with_mixed_content_policy`
— 3/3 ok: `http://cdn.invalid` и редирект-хоп на не-loopback блокируются, `http://127.0.0.1` при Strict — нет.

## Почему прежние PASS были ложными, а нынешние «зелёные» — тоже

- До BUG-1271 https на поддоменах не открывался: `cross-http` → swap-scheme → `https://www1.localhost` падал на TLS,
  сервер ничего не записывал, тест читал `blocked`. Это не было блокировкой mixed content.
- 766 зелёных `blocked` на no-redirect/keep-scheme объясняются тем же: запрос `http://*.localhost` уходит, `check-scheme`
  на http ничего не пишет (пишет только при https), и `take` возвращает исходное значение `put` → `blocked`. Тест
  `mixed-content` по построению не отличает «заблокировано» от «ушло по http без апгрейда» — различает только
  swap-scheme. Значит, **категория `mixed-content` целиком не измеряет enforcement движка**, пока стенд на `localhost`.
- `upgraded` для `*-http-downgrade` (104 FAIL) — автоапгрейд optionally-blockable (`img`/`audio`/`video`, Mixed Content
  L2 §4.1): `http://localhost:<https-порт>` уходит как есть на https-порт и падает `EOF before status line`.
  Автоапгрейда в движке нет (`grep -rn autoupgrad crates` — пусто); но и при нём loopback-URL не был бы mixed и не
  апгрейдился бы. Отдельный вопрос, этим багом не закрывается.
- `allowed same-https` (156 FAIL, `blocked`) — worklet-ы, `video`/`audio`-tag, `svg-a-tag`: запрос не дошёл вовсе;
  к loopback отношения не имеет, причина не разбиралась.

## Почему не движок

Поведение соответствует спецификациям: Secure Contexts §3.1 требует считать `localhost`/`*.localhost`
potentially trustworthy, Mixed Content §4.3 не трогает a priori authenticated URL. Chromium ведёт себя так же
(`http://localhost` с https-страницы не блокируется); upstream WPT обходит это хостом `web-platform.test` через
hosts-файл. Чинить движок под тест — значит сломать правило, на котором стоят `secure-contexts`, service workers и
dev-серверы на `localhost`.

Тот же артефакт уже записан в [BUG-1069](BUG-1069-FIXED.md) («Второй артефакт `browser_host = "localhost"`»:
`shared-storage/insecure-context.*.http.html`), но как частный случай; здесь — масштаб для `mixed-content`.

## Затронутые категории (не измерены)

- `mixed-content` — 388 id; ~400 сабтестов FAIL и ~770 PASS — оба артефакт хоста.
- `upgrade-insecure-requests` — 197 id; 496 сабтестов `expected "allowed" but got "blocked"` (та же схема stash,
  проверить, что механизм тот же — не разбиралось).
- `secure-contexts` и всё, что проверяет insecure context на `.http.`-странице (см. BUG-1069).

## Что нужно

Стенд, где «insecure» origin не loopback по имени. Варианты (выбор — P2/пользователь, в духе WPT-RUN-10):

1. `browser_host = "web-platform.test"` + разрешение имён без записи в hosts-файл: флаг/переменная окружения движка,
   отображающая `*.web-platform.test` и `*.not-web-platform.test` на `127.0.0.1` в `lumen_network` резолвере
   (аналог `--host-resolver-rules` у Chromium). Закрывает заодно 305 id альтернативного домена
   (`env_options`, комментарий у `browser_host`).
2. Отдельный прогон `mixed-content`/`upgrade-insecure-requests`/`secure-contexts` с машинным hosts-файлом — дешевле,
   но вне правила «стенд не трогает машину».

Чинить `is_loopback_host` нельзя (см. «Почему не движок»).

## Как проверить

После перевода стенда: `run_corpus.py --prefixes mixed-content` — в логе есть `RequestBlocked` с причиной
`mixed-content: blockable` на http-хопах, swap-scheme `blocked` зелёные без исходящего `GET http://…`, а no-redirect
`blocked` по-прежнему зелёные (теперь — потому что запрос не ушёл). До этого цифра `mixed-content` в
`docs/wpt/pass-rate.md` — не цифра движка.
