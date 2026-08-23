# BUG-757 — URL документа не обновляется после HTTP-редиректа: `location.href`/`document.URL`/`document.baseURI` показывают до-редиректный адрес

**Статус:** FIXED 2026-08-10
**Компонент:** network (`crates/network/src/lib.rs:1041` — `type PageResponse =
(Vec<u8>, Vec<(String, String)>)`; `fetch_page`/`fetch_page_streaming`,
строки 3578/3645; цепочка проходится внутри `fetch_with_redirect`, строка 2601),
shell (`crates/shell/src/main.rs:3565-3577` — `base: ResourceBase::Url(url.clone())`
берёт **запрошенный** URL)
**Найден:** 2026-08-10, диагностика живого логина `tbank.ru`

## Симптом

Навигация на `https://www.tbank.ru/login/?redirectTo=/invest/portfolio/`
получает от сервера **301** на `https://www.tbank.ru/auth/login/?redirectTo=…`.
Lumen редирект отрабатывает — контент в документе от `/auth/login/`. Но URL
документа остаётся до-редиректным (замер через MCP `eval` после
`wait document_ready`):

```
location.href     https://www.tbank.ru/login/?redirectTo=/invest/portfolio/
location.pathname /login/
document.URL      https://www.tbank.ru/login/?redirectTo=/invest/portfolio/
document.baseURI  https://www.tbank.ru/login/?redirectTo=/invest/portfolio/
```

Ожидание (Chrome/Edge): всё четыре — `https://www.tbank.ru/auth/login/?redirectTo=…`.

## Причина

Редирект-цепочка целиком проходится внутри
`fetch_with_redirect` (`crates/network/src/lib.rs:2589-2601`: на 3xx берётся
`Location`, резолвится в `next` и функция рекурсивно вызывает себя). Наружу
возвращается `Response { status, headers, body }` (`lib.rs:461`) — **поля с
финальным URL в нём нет**, и публичный `PageResponse` (`lib.rs:1041`) — это
просто `(body, headers)`. Финальный URL не существует за пределами рекурсии.

Поэтому shell физически не может узнать, куда доехал запрос, и подставляет
запрошенный: `crates/shell/src/main.rs:3577`, `base: ResourceBase::Url(url.clone())`,
где `url` — аргумент навигации. Дальше этот `base` становится base URL
документа и источником `location.*`.

## Последствия

1. **Относительные подресурсы и ссылки резолвятся от неверной базы.** На
   `tbank.ru` не проявилось (скрипты/стили по абсолютным URL на CDN), но сайт,
   который после редиректа `/foo` → `/foo/v2/` грузит `./app.js`, получит
   `/app.js` вместо `/foo/v2/app.js` — 404 на всё.
2. **SPA-роутеры читают не тот `pathname`.** Роут выбирается по
   `location.pathname`, а он до-редиректный.
3. **Любая проверка «мы там, где хотели» по URL врёт** — это ровно та ловушка,
   что уже записана в `CLAUDE.md` (готча к [BUG-438](BUG-438-FIXED.md)):
   «Assert on document identity, not on a URL comparison (a server redirect
   breaks that)». Здесь она получает конкретную причину.

Отличать от [BUG-438](BUG-438-FIXED.md): там `navigate` рапортует успех о
**незагрузившейся** странице и документ остаётся прежним. Здесь страница
загрузилась правильно, ошибочен только её URL.

## Как чинить

Провести финальный URL сквозь возврат:

1. Добавить в `Response` (`lib.rs:461`) поле с URL hop-а, который дал финальный
   не-3xx ответ, и заполнять его в `fetch_with_redirect`.
2. Расширить `PageResponse` до тройки `(Vec<u8>, Vec<(String, String)>, Url)`
   (или именованной структуры — предпочтительнее, тип уже читается плохо) и
   пробросить через `fetch_page`/`fetch_page_streaming`.
3. В `main.rs:3575` строить `ResourceBase::Url` из финального URL, а не из
   аргумента.
4. Отдельно проверить `LiveWindowSession::navigate`
   (`crates/driver/src/live_session.rs`), который пишет в `current_url`
   **запрошенный** URL до попытки загрузки — после этой правки он должен брать
   финальный (пересечение с BUG-438, но чинится тем же данным).

Тест: mock-сервер (в `lib.rs` уже есть hop-тесты около строк 5381/5706) — hop 1
отдаёт `302 Location: /next`, hop 2 — 200; проверять, что наружу пришёл URL
`/next`. Плюс сквозной: страница на `/a/` редиректит на `/b/c/`, в ней
`<img src="pic.png">` — запрос обязан уйти на `/b/c/pic.png`.

## Исправление (2026-08-10)

Финальный URL проведён наружу, и — что оказалось важнее — до **всех** копий
базы документа, а не только до самого документа.

**1. network.** `fetch_with_redirect` возвращает `(Response, Url)`: второй
элемент — URL hop-а, отдавшего не-3xx ответ, уже после HSTS-апгрейда. Кортеж, а
не поле `Option<Url>` в `Response`: адрес — свойство обхода цепочки, а не
ответа, и тотальный тип не позволяет caller-у молча подставить запрошенный URL
(14 call-site-ов стали ошибками компиляции, а не тихо-старым поведением).
`PageResponse` из кортежного алиаса стал pub-структурой `{ body, headers,
final_url }`.

**2. shell, документ.** `RawPage.base` строится из `final_url` — этого хватает
для `location.*`, `document.URL`/`baseURI`, origin хранилищ и подресурсов
финального прохода.

**3. shell, поток.** Живая проба показала, что пункта 2 **мало**: preload-сканер
(`feed_preload_and_emit`, streaming-поток) и загрузчик картинок/`@font-face`
(`spawn_image_requests`, UI-поток) держат собственные копии базы, снятые с
запрошенного адреса, — в логе видно, как хинт посчитан правильно
(`⤷ preload img /auth/login/pic.png`), а GET уходит на `/login/pic.png`, после
чего правильный запрос уже не повторяется. Поэтому:

* chunk-sink навигации стал `PageChunkSink` — порция тела приходит вместе с URL
  своего hop-а. Адаптер живёт внутри `fetch_with_redirect` (протокольные уровни
  ниже про URL по-прежнему не знают), а sink срабатывает только на 2xx-теле,
  то есть URL всегда финальный;
* streaming-поток пересобирает базу сканера по этому URL и шлёт
  `LoadEvent::DocumentBase` — до того, как частичный DOM начнёт заказывать
  подресурсы;
* `Lumen::document_resource_base()` отдаёт эту базу вместо `self.source`
  (запрошенного адреса). Пара `(base, generation)` вместо сброса на каждой
  навигации: устаревшая база просто перестаёт подходить. Событие шлётся ровно
  при расхождении адресов, поэтому без редиректа и для несетевых источников
  поведение ровно прежнее.

**4. automation.** `LiveWindowSession::current_url` спрашивает `location.href` у
самого документа; локальный слепок запрошенного адреса остался фолбэком на
случай отсутствующего JS-контекста. Это же чинит и второй источник вранья —
навигацию, инициированную самой страницей.

### Проверка

Юнит-тесты (`crates/network/src/lib.rs`): `fetch_page_reports_final_url_after_
redirect`, `fetch_page_streaming_reports_final_url_after_redirect` (плюс
проверка URL, приходящего в chunk-sink) и негативный контроль
`fetch_page_final_url_without_redirect_is_the_request_url`.

Живая проба (`--mcp-live-port` + локальный сервер, `/login/` → 301
`/auth/login/`, в странице `<img src="pic.png">`): все четыре значения —
`/auth/login/`, картинка запрошена как `/auth/login/pic.png`. Негативный
контроль — та же страница, отданная без редиректа: значения и список запросов
совпадают с редирект-прогоном.

### Остаток

При включённом `HttpCache` кэш-хит отдаёт запрошенный URL как `final_url`:
снапшот ключуется запрошенным адресом (`cache.store(&url_str, …)` кладёт под
него тело финального hop-а) и финального адреса не хранит. Навигация ходит с
выключенным кэшем, так что на живом пути это не проявляется; чинить придётся
вместе с ключом кэша. Отмечено в doc-комментарии `fetch_page`.

## Смежные

* [BUG-756](BUG-756-FIXED.md) — второй дефект, найденный на той же цепочке
  (cookie default-path); именно он блокирует логин, этот — нет.
* [BUG-438](BUG-438-FIXED.md) — успешный ответ `navigate` о несостоявшейся
  загрузке; пересекается пунктом 4 «как чинить».
