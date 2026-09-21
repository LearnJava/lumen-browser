# BUG-859 — ни один исходящий запрос не несёт `Referer` и `Origin`: заголовков нет в сетевом слое вообще

**Статус:** OPEN (ДОРАБОТКА → [GAP-REFERRER](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-REFERRER` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 25 — живой замер, маркер `beacon` + контрольные варианты)
**Область:** `crates/network/src/lib.rs` — единственное упоминание `Referer` во всём крейте это комментарий про запрет подделки заголовков (`lib.rs:2427`); генерации `Referer`/`Origin` нет ни на пути `fetch`, ни у сабресурсов, ни у `sendBeacon`
**Владелец:** P1/P3 (`lumen-network`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Сервер, которому страница `http://127.0.0.1:PORT/page.html` шлёт запросы,
не видит ни одного `Referer` и ни одного `Origin` — ни у сабресурса
(`<img>`, `<script src>`, `<link rel=stylesheet>`), ни у `fetch()`, ни у
`navigator.sendBeacon`, включая POST того же происхождения.

Это расходится с собственным планом проекта: [`docs/plan/privacy.md`](../docs/plan/privacy.md)
строка 18 обещает `Referer` с политикой `strict-origin-when-cross-origin`
по умолчанию, то есть **полный URL для same-origin** и origin-только для
cross-origin. Сейчас не отправляется ничего, то есть политика не «строгая»,
а «отсутствующая» — и отличить одно от другого страница/тест не может.

## Прямое измерение

`tests/wpt/verify_focus_mutation_animation_gaps.py` (2026-08-23, dev-release,
Linux, `main` = `530d0a444`); сервер пробы пишет для каждого запроса метод,
путь и заголовки `content-type`/`origin`/`referer`:

| запрос | ожидалось | получено |
|---|---|---|
| `GET /vfma-asset.js` из `fetch()` (вариант `control`) | `Referer: http://127.0.0.1:PORT/.vfma-control.html` | заголовка нет |
| `GET /vfma-pixel.gif` из `<img>` (вариант `perf-resource`) | `Referer: …` | заголовка нет |
| `POST /vfma-beacon?control-fetch` из `fetch()` | `Origin: http://127.0.0.1:PORT` + `Referer` | только `content-type` |
| `POST /vfma-beacon?abs-*` из `sendBeacon` | `Origin` + `Referer` | только `content-type` |

## Масштаб

Прямо на этом висят 2 из 4 id `beacon/headers` остатка снимка WPT-RUN-5
(`header-origin-same-origin.html`, `header-referrer-same-origin.html`,
`header-referrer-no-referrer.html` — зависший подтест `Test referer header
/beacon/resources/`), но настоящий масштаб больше и не измерен этим срезом:
в манифесте корпуса `referrer-policy/` — отдельная категория, и любой тест,
который читает `Referer`/`Origin` на стороне сервера, обречён.

Практическое следствие вне WPT: сайты, отличающие «прямой заход» от
перехода по ссылке, и любые CSRF-проверки на `Origin` видят Lumen как
клиент без контекста — это влияет на реальную совместимость, а не только на
тесты.

## Направление починки (не предписание)

Считать `Referer` по политике из `Referrer Policy §8.3` (источник политики:
`<meta name=referrer>`, заголовок ответа, атрибут `referrerpolicy`, дефолт из
`docs/plan/privacy.md`) и ставить `Origin` для всех методов кроме
`GET`/`HEAD` по `Fetch §5.4`. Обе точки — один слой формирования запроса, так
что фикс общий для `fetch`/сабресурсов/`sendBeacon`.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_focus_mutation_animation_gaps.py
   --variant control --variant beacon` — в строках `server saw` должны
   появиться `referer=`/`origin=`.
2. WPT: `run_report.py --all --root referrer-policy --recursive` и
   `--root beacon`.

## Срез 1 (2026-09-21, P6, `p6-gap-referrer`)

Закрыта половина остатка — `fetch()`/`XMLHttpRequest`/`navigator.sendBeacon`
(три из четырёх строк таблицы измерения выше, все кроме `<img>`-сабресурса)
теперь несут `Referer`/`Origin`:

- Новый модуль `crates/network/src/referrer_policy.rs` — `ReferrerPolicy`
  (все 8 keyword из Referrer Policy §3) + `compute_referrer()`, реализующий
  §8.3 (strip-to-origin, cross-origin/downgrade ветвление). Юнит-тесты на
  каждую политику.
- `HttpClient::with_document_context(referrer_url, policy)` — новое поле
  `document_context`, тот же provenance-паттерн, что у
  `with_connect_src_policy` (заполняется один раз владельцем документа).
  `fetch_request_impl` (общий провод `fetch()`/XHR/`sendBeacon`) добавляет
  `Referer` всегда и `Origin` для методов кроме GET/HEAD — оба заголовка
  вычисляются из `document_context`, не из `req.headers`: `Referer`/`Origin`
  уже были в списке forbidden author-headers (`build_author_headers` их
  режет), так что страница не могла бы их подделать и раньше.
- `crates/shell/src/page_pipeline.rs` подключает `with_document_context`
  туда же, где строится `fetch_provider`-клиент документа (рядом с
  `with_connect_src_policy`), используя `ResourceBase::url()` (новый метод) —
  URL страницы как referrer source. Политика в этом срезе всегда дефолт
  проекта (`strict-origin-when-cross-origin`, `docs/plan/privacy.md` §9.1).
- 6 живых тестов в `crates/network/src/lib.rs` (`js_fetch_*referer*`/
  `*origin*`) через `mock_server_capturing_bodies`: same-origin GET → полный
  `Referer` без `Origin`; same-origin POST → оба заголовка; cross-origin GET
  → origin-only `Referer`; без `with_document_context` — ни одного заголовка
  (гейт от регрессии дефолта).

**Не в этом срезе** (остаток задачи, не сужающий её статус `planned`):
- `<img>`/`<script src>`/`<link>` — сабресурсы строят свой `HttpClient` через
  `ResourceBase::http_client_for_subresource` в другом месте и не подключены;
  строка `perf-resource` таблицы измерения остаётся не закрытой.
- Источник политики — только дефолт проекта; `<meta name=referrer>`,
  заголовок ответа `Referrer-Policy` и атрибут `referrerpolicy` (уже
  отражаются в JS-шиме, `web_api_shim_tail_b.js`, но нигде не читаются
  Rust-стороной) не влияют на выбор `ReferrerPolicy`.
- Живой WPT-повтор (`verify_focus_mutation_animation_gaps.py`,
  `referrer-policy`/`beacon` категории) этим срезом не прогонялся — только
  юнит-тесты на мок-сервере.
