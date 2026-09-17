# BUG-811 — CSP разбирается, но не применяется: ни одна директива ничего не блокирует и событие `securitypolicyviolation` не диспатчится никогда

**Статус:** OPEN (ДОРАБОТКА → [GAP-CSPENF](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-CSPENF` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 18 — категория `content-security-policy`, 105 TIMEOUT остатка)
**Область:** `crates/js/src/csp.rs:1-6` (заголовок модуля: «Phase 0 … No enforcement»), `crates/js/src/csp.rs:60` (`window._lumen_dispatch_csp_violation` — определение), парсеры политики `crates/network/src/csp.rs:159` (`parse_csp_header`) и `crates/storage/src/csp_policies.rs`
**Владелец:** P1/P3 (движок: шелл + network). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Страница объявляет политику, нарушает её и ждёт события о нарушении.
Нарушение происходит (запрос не блокируется), события нет — тест висит
до таймаута враннера:

```html
<!-- content-security-policy/securitypolicyviolation/*, сокращённо -->
<meta http-equiv="Content-Security-Policy" content="img-src 'none'">
<script>
async_test(t => {
  document.addEventListener("securitypolicyviolation",
    t.step_func_done(e => assert_equals(e.violatedDirective, "img-src")));
});
</script>
<img src="pixel.png">   <!-- грузится, хотя img-src 'none' -->
```

Именно молчание даёт TIMEOUT, а не FAIL: класс `SecurityPolicyViolationEvent`
в движке есть (`typeof` возвращает `function`), поэтому feature-detect теста
проходит, а дальше ждать нечего.

## Прямое измерение

`tests/wpt/verify_csp_url_worker_gaps.py` (живое окно, http, улики из stderr
браузера; dev-release, Linux, 2026-08-21, коммит `41ee56b73`, `--seconds 6`;
10–11 тиков `setInterval` — страница жива всё это время):

| проба | ожидалось | получено |
|---|---|---|
| `csp-meta-script` — `script-src 'self'` + инлайн-скрипт | ничего (инлайн запрещён) | `inline-script-ran` — **скрипт выполнился** |
| `csp-meta-spv` — слушаем событие на `window` и на `document` | `spv-window` / `spv-document` | только `spv-class=function`; **события нет ни разу** |
| `csp-header-spv` — та же политика заголовком ответа | `spv-window` | только `header-seen`; **события нет** |
| `csp-meta-img` — `img-src 'none'` + `<img onload/onerror>` | `img-onerror` (заблокировано) | **ни одного события** — но это [BUG-804](BUG-804-FIXED.md) (события ресурсов у элементов из парсера), а не блокировка |

Строка `csp-meta-img` — единственная, из которой нельзя делать вывод о CSP:
`<img>` из парсера не диспатчит ни `load`, ни `error` независимо от политики.
Вывод о неприменении политики держится на `csp-meta-script`, где нарушение
наблюдаемо изнутри страницы.

## Причина (локализована чтением кода)

Разбор политики есть с обеих сторон — `parse_csp_header`
(`crates/network/src/csp.rs:159`) и хранилище политик
(`crates/storage/src/csp_policies.rs`), — а шага применения нет вовсе.
Заголовок `crates/js/src/csp.rs` говорит это прямым текстом:

> Phase 0: `SecurityPolicyViolationEvent` class and a native binding that
> dispatches it on `document`. **No enforcement** — the shell wires actual
> blocking in Phase 1 via `_lumen_fire_csp_violation`.

`grep -rn _lumen_dispatch_csp_violation crates/` даёт ровно три совпадения:
определение (`csp.rs:60`) и два обращения из юнит-тестов того же файла
(`csp.rs:187`, `csp.rs:199`, внутри `mod tests` с `csp.rs:76`). Ни один
загрузчик ресурса, ни один вызов скрипта, ни одна навигация хук не зовут —
то есть нарушение некому обнаружить, и событию неоткуда взяться.

Это не то же самое, что [BUG-692](BUG-692-OPEN.md): там одна директива
(`upgrade-insecure-requests`) не применяется к URL; здесь отсутствует весь
шаг применения и весь путь отчётности (`report-uri`/`report-to` — тоже).

## Масштаб

Механизм `csp-no-violation-event` в `tests/wpt/timeout_audit.py` забирает
**105 id** остатка снимка WPT-RUN-5 — самый крупный механизм среза 18.
По подкатегориям: `script-src` 17, `style-src` 12, `worker-src` 12,
`object-src` 7, `securitypolicyviolation` 7, `unsafe-hashes` 6 и хвост.
Это только те id, что ждали *события*; тесты, проверяющие сам факт
блокировки, дают FAIL и в этот счёт не входят — реальная цена по категории
выше.

Цена шире WPT: CSP — механизм безопасности, и сегодня Lumen принимает
политику любой строгости, не соблюдая её. Для приватного браузера это
расхождение между обещанным и фактическим поведением важнее, чем номер
в pass-rate.

## Направление починки (не предписание)

Порядок, в котором шаги полезны по отдельности:

1. Применение для подмножества директив, где точка проверки одна и уже
   существует, — `img-src`/`script-src`/`style-src`/`connect-src` на входе
   в сетевой слой, `script-src 'unsafe-inline'` перед вычислением инлайн-скрипта.
2. Отчётность: звать существующий `_lumen_dispatch_csp_violation` из точки
   отказа. Только этот шаг превращает 105 зависаний в осмысленные результаты.
3. `report-uri`/`report-to` — отдельно и позже.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_csp_url_worker_gaps.py
   --variant csp-meta-script` не печатает `inline-script-ran`.
2. `--variant csp-meta-spv` печатает `spv-window` и `spv-document`.
3. WPT: `run_report.py --all --root content-security-policy --recursive` —
   105 TIMEOUT уходят; часть тестов станет FAIL, и это ожидаемый
   промежуточный результат.

## Срез 1 (2026-09-12) — `script-src` из `<meta>` + диспетчеризация

Реализовано (`crates/shell/src/csp_enforce.rs`, детали —
[`subsystems/shell.md`](../subsystems/shell.md)): `script-src`/`default-src`
из `<meta http-equiv="Content-Security-Policy">` блокирует инлайновые
классические и модульные скрипты без совпавшего `'unsafe-inline'`/
`'nonce-…'` и впервые зовёт `_lumen_dispatch_csp_violation` — событие
`securitypolicyviolation` теперь реально диспатчится.

`--variant csp-meta-script` подтверждён: `inline-script-ran` больше не
печатается (печатается вообще ничего — инлайн заблокирован целиком, включая
скрипт-«тикер» самого харнесса, что корректно по спеке). `--variant
csp-meta-spv` из «направления починки» **не** годится как есть: его
слушатель `securitypolicyviolation` сам инлайновый и без nonce, поэтому
теперь тоже блокируется — обновлённый nonce-based проб (script-src
'nonce-…', второй безnonce-скрипт) подтверждает и блокировку, и доставку
события с `violatedDirective=script-src`/`blockedURI=inline`.

Ещё не покрыто (следующие срезы, по значимости): заголовок
`Content-Security-Policy` ответа (только `<meta>` разбирается — `RawPage`
без CSP-поля); все директивы кроме `script-src`
(`img-src`/`connect-src`/`style-src`/`style-src`/…); внешний `<script src>`
против host/scheme/hash источников; `report-uri`/`report-to`; hash-источники
(`'sha256-…'` и т.п. — только `'unsafe-inline'`/`'nonce-…'`).
`tests/wpt/verify_csp_url_worker_gaps.py` тоже долг: часть его CSP-вариантов
писана исходя из мира без enforcement и рассыпется под срезом 1, чинить
вместе со следующим срезом, а не отдельно.

## Срез 2 (2026-09-12) — парсинг `trusted-types`/`require-trusted-types-for`

Реализовано (`crates/network/src/csp.rs`): `CspPolicy` получила
`require_trusted_types_for_script: bool` и `trusted_types:
Option<TrustedTypesDirective>` (новая структура — `disallow_all`/
`allowed_policy_names`/`allow_duplicates`). Обе директивы падали в `_ =>
continue` парсера — это был явный первый блокер TRUSTEDTYPES-1
(ROADMAP.md), который сам указывает на это как на минимальный шаг для
разблокировки. Грамматика директив не source-list (`'script'`,
`'none'`/`'allow-duplicates'`/имена политик), поэтому они не легли в
`CspDirective`/`CspSource`, а стали отдельными полями `CspPolicy` — тем же
стилем, что уже есть у `report_uri`/`report_to`. Только парсинг: этот
крейт не решает, что с этими значениями делать — потребление (проверка
`createPolicy`/default-policy sink-путей) остаётся за TRUSTEDTYPES-1,
статус которого этим срезом разблокирован (`blocked` → `planned`,
см. ROADMAP.md). +6 unit-тестов в `csp.rs`.

Ещё не покрыто (не изменилось со среза 1): заголовок ответа
`Content-Security-Policy` (только `<meta>`); все директивы кроме
`script-src` (`img-src`/`connect-src`/`style-src`/…); внешний
`<script src>` против host/scheme/hash источников; `report-uri`/
`report-to`; hash-источники.

## Срез 3 (2026-09-12) — первый sink TRUSTEDTYPES-1: `setTimeout`/`setInterval`

Реализовано (`crates/js/src/trusted_types.rs`,
`crates/js/src/shim/web_api_shim_mid_b.js`, `crates/shell/src/scripts.rs`):
`require-trusted-types-for 'script'`, распарсенный срезом 2, впервые
потребляется. Шелл пушит флаг в рантайм один раз на навигацию (та же точка,
что `parse_time_layout`/`csp_policy` для `script-src`); JS-сторона получила
`_lumen_tt_get_compliant_script(input, sink)` (TT L2 §4.1.1, script-подмножество)
— `TrustedScript`-значение разворачивается как есть, иначе, при включённом
флаге, идёт через `defaultPolicy.createScript(value, 'TrustedScript', sink)`
или бросает `TypeError` без default policy; без директивы — поведение не
меняется (значение проходит как есть, Phase 0 для остальных страниц).
`_lumen_timer_string_handler` зовёт её синхронно в момент вызова
`setTimeout`/`setInterval` (спека требует проверку в timer-initialisation
steps, то есть на постановке в очередь, не на срабатывании) — ленивая
компиляция строки (BUG-831) не тронута, меняется только момент проверки.

Подтверждено живым окном (`--screenshot`, `require-trusted-types-for
'script'` через `<meta>`): строка/`null` без default policy бросает
`TypeError`; после `createPolicy('default', …)` то же самое проходит через
`createScript` с аргументами `(value, 'TrustedScript', 'Window
setTimeout'/'Window setInterval')`; уже готовый `TrustedScript` не идёт
через default policy повторно. Соответствует WPT
`trusted-types/Window-setTimeout-setInterval.html` и
`block-string-assignment-to-Window-setTimeout-setInterval.html` (оба
объявляют директиву через `<meta>`). +4 unit-теста
(`crates/js/src/dom/tests/v8_trusted_types.rs`).

Ещё не покрыто (весь остальной список sink'ов TT L2 §4.4, не изменилось по
объёму со сравнения в самой задаче TRUSTEDTYPES-1 в ROADMAP.md):
`innerHTML`/`outerHTML`, `Range.createContextualFragment`,
`document.write`/`writeln`, `<script>.src`/`.textContent`, `eval`/`new
Function`, атрибуты-обработчики событий через `setAttribute`; воркеры
(`DedicatedWorker`/`SharedWorker` эквиваленты `setTimeout`/`setInterval` —
шим общий, но флаг сегодня выставляется только для документа, не для
воркер-рантайма, так что sink-строка `'Window …'` там пока не годится).

## Срез 4 (2026-09-16, P6) — `img-src`/`default-src` против `<img src>`

Реализовано:

- `crates/network/src/csp.rs`: `CspPolicy::fetch_directive_allows(directive,
  url, self_origin)` — первое реальное URL-сопоставление источников (до сих
  пор `effective_sources` отдавал список токенов, а сравнение с URL нигде не
  делалось). Поддержаны `'none'`, `'self'` (через уже существующий
  `network::Origin::same_origin`), scheme-source (`https:`) и host-source
  (`example.com`, `*.example.com`, `scheme://host[:port]`) по грамматике
  CSP3 §6.7.2.4. Path-компонент host-source сознательно не матчится (шире
  спеки, никогда не уже — см. doc-comment `host_source_matches`); +14
  unit-тестов.
- `crates/shell/src/csp_enforce.rs`: `img_src_blocked(policy, url,
  self_origin)` — тонкая обёртка (`Url::parse` + `fetch_directive_allows`),
  URL, который не парсится, не считается нарушением (сохраняет обычный
  network-failure путь); +4 unit-теста.
- `crates/shell/src/persistent_js.rs`: обобщённый `PersistentJs::
  fire_csp_violation(directive, blocked_uri, original_policy)` —
  `fire_script_src_violation` (срез 1) был захардкожен на `script-src`/
  `inline`; новый метод — тот же `_lumen_dispatch_csp_violation` под любую
  директиву/URL.
- Три независимых места, где `<img src>` инициирует сетевой запрос, заведены
  под гейт (BUG-172 уже называл `decode_image` общей точкой fetch+decode для
  первых двух, но точка **решения фетчить или нет** у каждого — своя, до
  входа в `decode_image`):
  - `crates/shell/src/subresources.rs::fetch_and_decode_images` (eager-пайплайн,
    после `DOMContentLoaded`) — добавлен вариант `ImgOutcome::Blocked`,
    диспатчит `securitypolicyviolation` (через `PersistentJs::
    fire_csp_violation`, JS-рантайм к этому моменту уже существует).
  - `crates/shell/src/page_load.rs::spawn_image_requests` (общий продюсер
    `spawn_stream_image_loads`/`spawn_dynamic_image_loads` — streaming-парсинг
    и скриптовая вставка `<img>`) — блокирует до `std::thread::spawn`, шлёт
    `LoadEvent::ImageDecodeFailed`, чтобы `error` пришёл тем же путём, что и
    обычный сетевой отказ. Здесь `securitypolicyviolation` не диспатчится —
    это фоновый поток без JS-рантайма; событие всё равно приходит из
    eager-пайплайна ниже, который пересчитывает тот же гейт независимо (см.
    «Известное дублирование» ниже).
  - `crates/shell/src/frames.rs`/`frame_lazy.rs` (картинки внутри `<iframe>`)
    — **не тронуты этим срезом**: политика подфрейма своя, а гейт сейчас
    видит только политику top-level документа.

Ловушка, из-за которой первая живая проверка молчала: `lumen_layout::
ImageRequest.url` — сырое значение атрибута `src` (`.cspgap-pixel.png`),
не резолвленный URL; `Url::parse` тихо проваливался и гейт "fail open"-ил.
Фикс — `base.resolve_str(&req.url)` перед проверкой (то же самое место,
где `decode_image`/`fetch_image_bytes` резолвят его для реального фетча).

Подтверждено живым окном (`img-src 'none'`, `file://`-документ): сетевой
лог не содержит `→ GET …pixel.png` вовсе (принцип №4 «каждый исходящий байт
виден» — теперь пуст для заблокированного ресурса, а не просто неуспешен);
`securitypolicyviolation` приходит с `violatedDirective=img-src`,
`blockedURI` = резолвленный URL, `originalPolicy=img-src 'none'`; `<img
onerror>` срабатывает. `tests/wpt/verify_csp_url_worker_gaps.py --variant
csp-meta-img` теперь печатает `img-onerror` вместо `img-onload` (было
наоборот — срез 1 оставлял `img-src` полностью непроверенным).

Известное дублирование (не регрессия, существовавший паттерн): страница, где
`<img>` виден И streaming-, И eager-проходу, получает `error` дважды —
`spawn_image_requests` шлёт его первым (без CSP-события), `fetch_and_decode_images`
пересчитывает тот же URL и шлёт `error` + `securitypolicyviolation` второй раз.
Тот же паттерн двойной доставки уже существовал для обычных decode-отказов до
этого среза (оба пути независимо решают, фетчить ли URL, и оба сообщают о
неудаче) — CSP ничего не меняет в этом отношении, чинить отдельно от GAP-CSPENF.

Ещё не покрыто (следующие срезы): заголовок `Content-Security-Policy`
ответа (только `<meta>`); директивы кроме `script-src`/`img-src`
(`connect-src`/`style-src`/`media-src`/…); картинки внутри `<iframe>`;
`background-image`/`@font-face url()` (используют `fetch_image_bytes`
напрямую, не `decode_image` — не гейтятся вовсе); hash-источники;
`report-uri`/`report-to`; дедупликация двойного `securitypolicyviolation`
выше.

## Срез 5 (2026-09-16, P6) — заголовок `Content-Security-Policy` ответа

Реализовано:

- `crates/shell/src/page_source.rs`: `content_security_policy_header(
  resp_headers)` — свободная функция рядом с `cache_control_no_store` и
  `response_content_type`, поэтому unit-тестируется без сети (+6 тестов:
  есть/нет/регистр имени/повтор/пустое значение/`-Report-Only`). Имя
  сравнивается точным `eq_ignore_ascii_case`, а не префиксом — иначе
  `Content-Security-Policy-Report-Only` (который по определению ничего не
  блокирует) попал бы в enforcement. Повторный заголовок — независимая
  политика по CSP3 §3.4; здесь он сливается через `"; "`, тем же
  упрощением, которое срез 1 уже применял к нескольким `<meta>`.
- `RawPage.csp_header: Option<String>` заполняется в обоих сетевых
  конструкторах (`load_bytes`, `load_bytes_streaming`) и равен `None` во
  всех несетевых (`AboutBlank`/`File`/`Snapshot`/`Static`).
- `crates/engine/dom/src/lib.rs`: `Document::csp_header()`/`set_csp_header()`
  — поле рядом с `character_set`/`content_type` (`#[serde(default)]`, живёт
  через bfcache, как и они). Заголовок нужен **не один раз при парсинге**, а
  в каждый момент enforcement: пять точек (`scripts.rs`, `subresources.rs`,
  `page_load.rs` ×2, `page_pipeline.rs`) пересчитывают политику по `&Document`
  заново, и ни у одной из них нет доступа ни к `RawPage`, ни к `LayoutSource`.
  Поэтому заголовок едет на самом документе; ни одна из пяти точек при этом
  не изменилась.
- `render_bytes`/`parse_and_layout` получили параметр `csp_header:
  Option<&str>`; `parse_and_layout` штампует его на документ сразу после
  парсинга — до исполнения любого скрипта, тем же куском кода, что
  `set_character_set`/`set_content_type`.
- `crates/shell/src/csp_enforce.rs`: `document_meta_csp_policy` →
  `document_csp_policy` (имя больше не врёт — источников теперь два).
  Заголовок идёт первым, затем `<meta>` в порядке документа; слияние —
  то же `"; "`. По CSP3 §3 это независимые политики и нарушение любой
  из них — нарушение; честная двойная проверка вместо слияния остаётся
  отдельной работой (см. «Ещё не покрыто»).

Подтверждено живым окном (`--screenshot`, локальный HTTP-сервер, dev-release):

- `Content-Security-Policy: script-src 'self'` в заголовке + внешний
  `/listener.js` (разрешён `'self'`) + инлайновый `<script>`: `PROBE
  inline-ran` не печатается вовсе, а слушатель печатает `PROBE spv
  directive=script-src policy=script-src 'self'` — то есть заголовок
  блокирует инлайн и `originalPolicy` — именно текст заголовка.
- Слияние заголовка и `<meta>`: заголовок `img-src 'none'` + `<meta>`
  `script-src 'unsafe-inline'` на одной странице — инлайновый скрипт
  выполняется (его разрешила `<meta>`), и он же ловит `PROBE spv
  directive=img-src uri=http://…/pixel.png` от заголовка; `onload`
  картинки не срабатывает. Обе политики действуют одновременно.
- `tests/wpt/verify_csp_url_worker_gaps.py --variant csp-header-spv` больше
  не печатает `header-seen`: инлайновый скрипт этой страницы заблокирован
  заголовочным `script-src 'self'` (до среза он выполнялся). Оставшийся в
  этом варианте `img-loaded-anyway` — не свидетельство о CSP: сам скрипт
  probe-а оговаривает (BUG-804), что `<img>`, написанный парсером, шлёт
  `load`/`error` независимо от политики, и картинку здесь успевает забрать
  streaming-продюсер, который заголовка не видит (см. ниже).

Ещё не покрыто (следующие срезы): честная независимая проверка заголовка и
`<meta>` вместо их слияния в одну строку; streaming-продюсер картинок
(`spawn_stream_image_loads`) — он работает над частичным DOM от
`IncrementalTreeBuilder`, у которого заголовка нет вовсе (`<meta>` он видит,
заголовок — нет), плюс lazy-путь; директивы кроме `script-src`/`img-src`
(`connect-src`/`style-src`/`media-src`/…); картинки внутри `<iframe>`;
`background-image`/`@font-face url()`; hash-источники; `report-uri`/
`report-to`; дедупликация двойного `securitypolicyviolation`.

## Срез 6 (2026-09-17, P6) — `script-src`/`default-src` против внешнего `<script src>`

Реализовано:

- `crates/network/src/csp.rs`/`crates/shell/src/csp_enforce.rs`: `script_src_blocked(policy, url, self_origin)` —
  та же host/scheme/`'self'` проверка (`CspPolicy::fetch_directive_allows`), которую
  срез 4 уже сделал для `img-src`, теперь применена к `CspDirective::ScriptSrc`; +4
  unit-теста в `csp_enforce.rs`.
- `crates/shell/src/scripts.rs::resolve_script_sources` получила параметр `doc:
  &Document` — та же одноразовая точка пересчёта политики, что и у `subresources.rs`
  (`document_csp_policy` заново по документу), и резолвит абсолютный URL (`base.
  resolve_str(src)`) ДО обеих веток (`ResolvedResource::File`/`Url`): заблокированный
  URL не читается ни с диска, ни из сети — тот же принцип «ни одного исходящего
  байта», что срез 4 применил к картинкам. `ResolvedScript` получила поле
  `csp_blocked: bool`, отличающее «файл не пришёл из-за CSP» от «файл не пришёл из-за
  сети» — оба случая дают `external_ok: Some(false)` (и потому `error` на элементе,
  BUG-804), но только первый обязан ещё и диспатчить `securitypolicyviolation`.
- `run_scripts_with_dom` (обе точки исполнения — classic и module) диспатчит
  `securitypolicyviolation` при `csp_blocked`, используя резолвленный `url` как
  `blockedURI` — `fire_script_src_violation` (срез 1, был захардкожен на
  `blocked_uri = "inline"`) обобщена на произвольный `blockedURI`, вызовы для
  инлайна передают `"inline"` явно.
- Три места, вызывавшие `resolve_script_sources` без документа
  (`page_pipeline.rs`, `frames.rs` — своя политика для каждого фрейма, не
  top-level, в отличие от ограничения среза 4 для картинок, — и
  `tab_lifecycle/hibernate.rs`), обновлены на новую сигнатуру.

Подтверждено unit-тестами `csp_enforce.rs` (`script_src_none_blocks_external`,
`script_src_allowed_host_passes`, `no_script_src_allows_external`,
`script_src_unparseable_url_not_blocked`) и полным `cargo build -p lumen-shell`
+ `cargo clippy -p lumen-shell --all-targets -- -D warnings` (оба чисто) +
`scripts/scoped-test.sh` (единственный красный тест — `cases::snapshot_cpu::
cpu_snapshots_match_references`, тот же 7-файловый дрейф эталонов, что уже
числится на `main` до этой ветки, не регрессия этого среза).

Ещё не покрыто (следующие срезы): директивы кроме `script-src`/`img-src`
(`connect-src`/`style-src`/`media-src`/…); картинки/скрипты внутри `<iframe>`
top-level документа (у скриптов внутри фрейма политика теперь своя, см. выше,
но top-level картинки по-прежнему не видят политику подфрейма); честная
независимая проверка заголовка и `<meta>` вместо их слияния; hash-источники;
`report-uri`/`report-to`; дедупликация двойного `securitypolicyviolation` для
картинок (не менялось этим срезом).

## Срез 7 (2026-09-17, P6) — `style-src`/`default-src` против внешнего `<link rel=stylesheet>`

Реализовано:

- `crates/shell/src/csp_enforce.rs`: `style_src_blocked(policy, url, self_origin)` —
  тот же host/scheme/`'self'` фетч-гейт (`CspPolicy::fetch_directive_allows`), что
  срезы 4/6 дали `img-src`/`script-src`, теперь применён к `CspDirective::StyleSrc`;
  +5 unit-тестов.
- `crates/shell/src/stylesheets.rs::load_linked_stylesheets` получила третий элемент
  возврата — `Vec<String>` заблокированных resolved URL — и считает политику
  документа один раз до параллельного фетча (та же одноразовая точка, что
  `fetch_and_decode_images`/`resolve_script_sources` уже используют), резолвит `href`
  ДО обращения к `fetch_stylesheet_text`: заблокированный лист не читается ни с диска,
  ни из сети. Заблокированный `<link>` даёт тот же `false`-исход, что сетевая
  неудача — `error` на элементе (BUG-804) срабатывает без изменений в этом коде.
- `page_pipeline.rs::PageCascade` получила поле `blocked_by_style_src`; после
  `link_outcomes` диспатчит `securitypolicyviolation` по каждому заблокированному
  URL — та же одноразовая схема, что `blocked_by_img_src` (срез 4).
- `frames.rs` (CSS под-документа `<iframe>`) уже вызывает
  `load_linked_stylesheets` и потому тоже блокирует фетч заблокированных листов, но
  не диспатчит `securitypolicyviolation` для этого — тот же пробел, что `img-src`
  уже имеет в этой функции (там тоже нет `blocked_by_img_src`-провода).

Не покрыто этим срезом: инлайновые `<style>`/атрибут `style` (не блокируются,
только внешний `<link>`); `@import` внутри уже загруженного листа наследует
политику владельца без отдельной проверки; директивы кроме `script-src`/
`img-src`/`style-src` (`connect-src`/`worker-src`/…); `securitypolicyviolation`
для листов, заблокированных внутри `<iframe>` (см. выше); честная независимая
проверка заголовка и `<meta>` вместо их слияния; hash-источники;
`report-uri`/`report-to`.

Подтверждено unit-тестами `csp_enforce.rs` (`no_style_src_allows_external`,
`style_src_none_blocks_external`, `style_src_allowed_host_passes`,
`style_src_default_src_fallback_blocks`, `style_src_unparseable_url_not_blocked`)
и `cargo clippy -p lumen-shell --all-targets -- -D warnings` (чисто) +
`scripts/scoped-test.sh` (единственный красный тест — `cases::snapshot_cpu::
cpu_snapshots_match_references`, тот же 7-файловый дрейф эталонов, что уже
числится на `main` до этой ветки, не регрессия этого среза).

## Срез 8 (2026-09-17, P6) — `securitypolicyviolation` внутри `<iframe>` для `img-src`/`style-src`

Реализовано (`crates/shell/src/frames.rs`): закрыт ровно пробел, названный
срезами 4/7 («`frames.rs` уже блокирует фетч заблокированных ресурсов
подфрейма, но не диспатчит `securitypolicyviolation` для этого») — но теперь
против собственной политики ребёнка, а не top-level документа (в отличие от
ограничения среза 4/6 для картинок/скриптов top-level страницы, у фрейма
своя политика с самого начала, как и у скриптов внутри него, срез 6).

- `fetch_frame_subresources` считает `img-src`/`default-src` гейт ребёнка
  (`csp_enforce::document_csp_policy` по `doc` подфрейма) той же одноразовой
  точкой, что и `subresources.rs::fetch_and_decode_images`, и резолвит URL
  `<img>` (`base.resolve_str`) до входа в `decode_image` — заблокированный
  ресурс не фетчится вовсе, тот же принцип «ни одного исходящего байта».
  Заблокированный/сетевой исход различаются третьим элементом кортежа
  параллельной фазы (`Option<String>` — резолвленный URL, если заблокирован
  `img-src`), чтобы последовательная фаза 2 не путала CSP с обычным сетевым
  отказом.
- `load_linked_stylesheets` (срез 7) уже считала и отбрасывала список
  заблокированных `style-src` URL для CSS подфрейма — список просто
  перестал отбрасываться.
- `FrameSubresourceOutcomes` получила `blocked_by_img_src`/
  `blocked_by_style_src: Vec<String>`; `spawn_frame` после того, как рантайм
  ребёнка создан (`js.notify_dom_content_loaded()`), диспатчит
  `securitypolicyviolation` по каждому URL через `PersistentJs::
  fire_csp_violation` — та же одноразовая схема, что `page_pipeline.rs` уже
  применяет к top-level `blocked_by_img_src`/`blocked_by_style_src` (срезы
  4/7), только политика читается заново с документа ребёнка
  (`child_doc_arc`), а не с top-level.

Не покрыто этим срезом: `script-src` внутри фрейма (срез 6 уже блокирует
фетч через `resolve_script_sources(doc, …)` с политикой ребёнка, но диспатч
`securitypolicyviolation` для скриптов фрейма не проверен отдельно — тот же
путь исполнения, что и top-level, должен уже работать, но не подтверждён
живым пробом в этом срезе); вложенные фреймы фрейма (рекурсия та же
функция, не тестировалась); директивы кроме `script-src`/`img-src`/
`style-src`; honest независимая проверка заголовка и `<meta>`;
hash-источники; `report-uri`/`report-to`; дедупликация двойного
`securitypolicyviolation` для картинок (не касается фреймов — там нет
streaming-продюсера).

Подтверждено `cargo build -p lumen-shell --features v8` + `cargo clippy
-p lumen-shell --all-targets --features v8 -- -D warnings` (оба чисто) +
`scripts/scoped-test.sh` (единственный красный тест — `cases::snapshot_cpu::
cpu_snapshots_match_references`, тот же 7-файловый дрейф эталонов
(`55-text-rendering`, `57-canvas-2d`, `32-list-markers`, `34-forms`,
`45-multiple-backgrounds`, `51-scrollbar-rendering`, `1000000-final`), что
уже числится на `main` до этой ветки (BUG-1008), не регрессия этого среза).

## Срез 9 (2026-09-17, P6) — `img-src` против `loading="lazy"` `<img>`

Реализовано (`crates/shell/src/page_load.rs::fetch_and_register_lazy_images`):
последний производитель картинок, у которого не было вообще никакого
`img-src`-гейта, — eager-пайплайн (`fetch_and_decode_images`) и
streaming/dynamic-продюсер (`spawn_image_requests`) получили его срезом 4, а
деферренный `loading="lazy"`-путь (запросы, которые JS шлёт из
`_lumen_deliver_lazy_images`, когда картинка входит в проксимити-маржу
вьюпорта) фетчил байты безусловно. Страница, чья политика запрещает
происхождение, могла обойти её, просто пометив `<img>` как `lazy`.

- Один одноразовый `document_csp_policy` перед циклом (тот же документ, что
  `spawn_dynamic_image_loads` уже читает через `self.layout_source`), а не на
  каждый URL — то же обоснование, что и у остальных точек enforcement.
- На каждый URL: `base.resolve_str` → `csp_enforce::img_src_blocked`.
  Заблокированный URL не долетает до `fetch_image_bytes` вовсе (тот же
  принцип «ни одного исходящего байта», что срез 4 дал eager-пути) —
  `securitypolicyviolation` (`PersistentJs::fire_csp_violation`) и `error` на
  элементе (`fire_image_error`) диспетчатся одним `route_task_js`-действием,
  сохраняя порядок «CSP-событие раньше decode-ошибки», как в
  `scripts.rs`/`page_pipeline.rs`.

Не покрыто этим срезом: `background-image`/`@font-face url()` — используют
`fetch_image_bytes` напрямую в обход и eager-, и lazy-, и streaming-гейтов, не
проверяются вовсе; директивы кроме `script-src`/`img-src`/`style-src`;
honest независимая проверка заголовка и `<meta>`; hash-источники;
`report-uri`/`report-to`; дедупликация двойного `securitypolicyviolation` для
картинок (существовавший паттерн, см. срез 4); `script-src`
`securitypolicyviolation` внутри `<iframe>` не подтверждён живым пробом (см.
срез 8).

Подтверждено `cargo build -p lumen-shell --features v8` + `cargo clippy
-p lumen-shell --all-targets --features v8 -- -D warnings` (оба чисто) +
`cargo test -p lumen-shell --features v8 --bin lumen` (1822 passed, 0
failed). `scripts/scoped-test.sh` не догнан до конца — известный сломанный
гейт [BUG-805](BUG-805-OPEN.md) (виснет на
`lumen-network::h3::udp::tests::udp_round_trip`, не связано с этой правкой,
не регрессия этого среза).

## Срез 10 (2026-09-17, P6) — `connect-src` против `fetch()`/`XMLHttpRequest`

Первая директива этого GAP, применённая не к производителю сабресурсов
парсера/страницы, а к запросу, который скрипт может выпустить в любой момент
жизни документа — поэтому решение архитектурно другое, чем у срезов 4/6/7/9
(`img-src`/`script-src`/`style-src`): там шелл держит URL и документ в одной
точке кода (парсинг, вставка узла) и может проверить политику до вызова сети.
`fetch()`/`XMLHttpRequest` вызываются из JS в произвольный момент через
нативный мост (`crates/js/src/v8_runtime/install/net.rs`), у которого нет
`&Document` — только `Arc<dyn JsFetchProvider>` (сам `HttpClient`). Поэтому
гейт переехал в `lumen-network`:

- `Error::CspConnectSrcBlocked { blocked_uri, original_policy }`
  (`crates/core/src/error.rs`) — новый вариант, отдельный от `Error::Network`,
  чтобы JS-мост мог отличить «CSP заблокировал» от обычного сетевого сбоя и
  продиспетчить `securitypolicyviolation` с правильными `blockedURI`/
  `originalPolicy`.
- `HttpClient::with_connect_src_policy(policy, self_origin, original_policy)`
  (`crates/network/src/lib.rs`) — билдер-метод, вызываемый один раз в
  `page_pipeline.rs::parse_and_layout` сразу после того, как документ
  посчитал `csp_enforce::document_csp_policy` (тот же агрегат заголовка +
  `<meta>`, что и у остальных срезов), перед тем как `HttpClient` уходит в
  `fetch_provider`/`ws_provider`/`sse_provider`. `fetch_request_impl` —
  единственный путь и `fetch()`, и `XMLHttpRequest` (оба используют
  `_lumen_fetch_sync*`/`_lumen_fetch_cancellable*`/`_lumen_fetch_async_*`) —
  проверяет `connect-src` (или `default-src`) сразу после `Url::parse`, до
  SW-перехвата и до любого DNS/сокета.
- На JS-стороне пять нативных мостов (`_lumen_fetch_sync`,
  `_lumen_fetch_sync_with_body`, `_lumen_fetch_cancellable[_with_body]`,
  асинхронный `_lumen_fetch_async_start`/`_poll`) возвращали только
  bool/u32 — недостаточно, чтобы пронести `blocked_uri`/`original_policy` до
  шима. Синхронные/cancellable делят один side-channel слот
  (`last_csp_block`, то же однослотовое допущение, что уже несёт `cache` —
  JS вызывает их блокирующе, по одному за раз), читаемый и очищаемый новым
  `_lumen_fetch_last_csp_block()`; асинхронный путь несёт `CspBlocked{..}`
  прямо в `AsyncOutcome` (свой поток на хэндл, общий слот был бы гонкой) и
  отдаёт через `_lumen_fetch_async_csp_info(handle)`, `poll` → `4`. Общий
  JS-хелпер `_lumen_fire_connect_src_violation` (`web_api_shim_mid_b.js`)
  зовёт уже существующий `_lumen_dispatch_csp_violation('connect-src', …)`
  (срез 1) из всех точек отказа — `fetch()`'а (sync/cancellable/async) и
  `xhr.rs` (отдельный `rt.eval`, делит нативы с `fetch()`, поэтому обошёлся
  без собственного моста).
- WebSocket/EventSource делят тот же `HttpClient`, но не гейтятся этим
  срезом — `connect-src` по CSP3 §6.7.2 покрывает и их, это осознанно
  оставлено следующему срезу (гейт только у `JsFetchProvider::fetch_request`,
  не у `JsWebSocketProvider`/`JsSseProvider`).

Не покрыто этим срезом: WebSocket/EventSource против `connect-src`;
`sendBeacon` (свой путь `fetch_with_body_sync` в обход `fetch_request_impl`,
не гейтится); директивы кроме `script-src`/`img-src`/`style-src`/
`connect-src`; `report-uri`/`report-to`; hash-источники; честная независимая
проверка заголовка и `<meta>`.

Тесты: 3 юнит-теста в `crates/network/src/lib.rs` (блок до сети, разрешённый
хост не гасится гейтом, отсутствие политики не изобретает нарушение — все три
без реального сетевого ввода-вывода) + 2 интеграционных в
`crates/js/src/dom/tests/v8_whatwg_streams.rs` (мок-провайдер, всегда
возвращающий `CspConnectSrcBlocked`, доказывает, что `fetch()` и `XHR`
реально диспетчат `securitypolicyviolation` с `connect-src`/`blockedURI`/
`originalPolicy`). Подтверждено `cargo clippy --workspace --all-targets -- -D
warnings` (чисто) + адресные `cargo test -p lumen-network`, `-p lumen-js
--features v8-backend --lib`, `-p lumen-shell csp` (все зелёные).
`scripts/scoped-test.sh` не догнан до конца — тот же известный сломанный
гейт [BUG-805](BUG-805-OPEN.md), не регрессия этого среза.

## Срез 11 (2026-09-17, P6) — `connect-src` против WebSocket/EventSource

Реализовано: тот же `connect_src_policy`-гейт, который срез 10 дал
`fetch()`/`XMLHttpRequest`, теперь применён к двум оставшимся JS-инициированным
сетевым API, явно отложенным срезом 10 («WebSocket/EventSource делят тот же
`HttpClient`, но не гейтятся этим срезом»):

- `crates/network/src/lib.rs`: `JsWebSocketProvider::connect` и
  `JsSseProvider::connect_sse` для `HttpClient` получили ровно ту же проверку
  `policy.fetch_directive_allows(&CspDirective::ConnectSrc, &url, self_origin)`,
  что `fetch_request_impl` уже делает — до любой работы с сокетом
  (`WebSocket::connect_deflate`/`SseProvider::connect_sse`), поэтому TCP-хендшейк
  для заблокированного origin не начинается вовсе. `HttpClient` уже нёс
  `connect_src_policy` одним полем на все три провайдера — новый код только
  читает его, билдер (`with_connect_src_policy`) и точка установки
  (`page_pipeline.rs::parse_and_layout`) не изменились.
- `crates/js/src/v8_runtime/install/net.rs`: `install_websocket`/`install_sse`
  получили каждый свой `last_csp_block: Arc<Mutex<Option<(String, String)>>>` —
  тот же однослотовый side-channel паттерн, что срез 10 завёл для
  `_lumen_fetch_last_csp_block` (оба API соединяются синхронно и по одному в
  конструкторе, поэтому одного слота на рантайм достаточно). Новые нативы
  `_lumen_ws_last_csp_block()`/`_lumen_sse_last_csp_block()` читают и очищают
  слот; `_lumen_ws_connect`/`_lumen_sse_connect` при `Err(CspConnectSrcBlocked)`
  пишут туда `(blocked_uri, original_policy)` и по-прежнему возвращают `0`
  (тот же код ошибки, что и обычный сбой соединения — JS-сторона уже отличала
  «провайдера нет» от «сокет не открылся» только по факту `h === 0`, поэтому
  различение живёт в side-channel, а не в возвращаемом типе).
- `crates/js/src/shim/web_api_shim_mid_b.js`: конструкторы `WebSocket` и
  `EventSource` в ветке `if (!h)` читают свой side-channel **до** постановки
  `setTimeout(fn, 0)` (слот однослотовый — следующий вызов `_lumen_ws_connect`
  его перезапишет) и зовут уже существующий `_lumen_fire_connect_src_violation`
  (срез 10) внутри самого таймера, перед диспатчем `error`/`close` — тот же
  порядок «CSP-событие раньше сетевой ошибки», что срезы 4/6/9 уже
  устанавливали для картинок/скриптов.

Не архитектурно другое решение, в отличие от того, как срез 10 сам был
архитектурно другим относительно срезов 4/6/7/9: `HttpClient` уже был общей
точкой для всех трёх JS-сетевых API после среза 10, оставалось только
продублировать одну и ту же проверку в двух оставшихся методах трейта.

Подтверждено тестами (без реального сетевого ввода-вывода): 3 юнит-теста в
`crates/network/src/lib.rs` (`connect_src_none_blocks_websocket_before_any_handshake`,
`connect_src_none_blocks_event_source_before_any_handshake`,
`no_connect_src_policy_does_not_block_websocket_or_sse` — тот же
"before-any-network-io"/"no-policy-no-block" рисунок, что срез 10 уже
проверял для `fetch()`) + 4 интеграционных в
`crates/js/src/dom/tests/v8_ws_sse.rs` (`websocket_connect_src_block_reaches_native_side_channel`,
`websocket_connect_src_block_fires_security_policy_violation_event`,
`eventsource_connect_src_block_reaches_native_side_channel`,
`eventsource_connect_src_block_fires_security_policy_violation_event` —
последние два, в отличие от среза 10's fetch-тестов, реально прогоняют
таймер (`_lumen_tick_timers()`, уже использованный существующим
`eventsource_constructor_no_provider_stays_connecting_then_closes_async`) и
проверяют полный `securitypolicyviolation` с `violatedDirective=connect-src`
и правильными `blockedURI`/`originalPolicy`, а не только сам side-channel).
Подтверждено `cargo clippy --workspace --all-targets -- -D warnings` (чисто)
+ `cargo test -p lumen-network connect_src` (6/6) + `cargo test -p lumen-js
--features v8-backend --lib dom::tests::v8_ws_sse` (135/135, включая новые
4) + `cargo build -p lumen-shell --features v8` + `cargo clippy -p
lumen-shell --all-targets --features v8 -- -D warnings` (оба чисто).

Не покрыто этим срезом: `sendBeacon` (свой путь `fetch_with_body_sync` в
обход `fetch_request_impl`, всё ещё не гейтится — то же ограничение, что
срез 10 уже называл); директивы кроме `script-src`/`img-src`/`style-src`/
`connect-src`; `report-uri`/`report-to`; hash-источники; честная независимая
проверка заголовка и `<meta>`; картинки/скрипты/листы внутри `<iframe>` не
покрытые срезами 6/8.

## Срез 12 (2026-09-17, P6) — `connect-src` против `navigator.sendBeacon`

Реализовано: последний JS-инициированный сетевой API, названный не покрытым
срезом 10/11, — `sendBeacon` действительно шёл в обход `fetch_request_impl`
(и потому в обход `connect_src_policy`), но не потому, что кто-то забыл
гейт: `_lumen_send_beacon` (`crates/js/src/v8_runtime/install/net.rs`)
специально спавнит `fetch_with_body_sync` на detached-потоке (W3C Beacon §3 —
fire-and-forget, вызывающий скрипт не блокируется), а к тому моменту, когда
поток дошёл бы до `fetch_request_impl`'s гейта, вызвавший JS уже получил
`true` и продолжил выполнение — сообщать о блокировке уже некому и незачем.

- `crates/core/src/ext.rs`: новый метод трейта `JsFetchProvider::
  check_connect_src(url) -> Result<()>` — I/O-free пре-чек (только `Url::parse`
  + сверка с политикой, без похода в сеть), с default-реализацией `Ok(())` для
  двойников без CSP (совпадает с поведением `HttpClient` без установленной
  `connect_src_policy`).
- `crates/network/src/lib.rs`: `fetch_request_impl`'s инлайновая проверка
  вынесена в `HttpClient::connect_src_gate(&Url)` — общий приватный хелпер;
  `check_connect_src` — тонкая обёртка (`Url::parse` + `connect_src_gate`),
  та же логика, что срезы 10/11 уже применяют к `fetch()`/XHR/WS/SSE.
- `crates/js/src/v8_runtime/install/net.rs`: `_lumen_send_beacon` зовёт
  `provider.check_connect_src(&url)` **до** `std::thread::spawn` — заблокированный
  URL не доходит до `fetch_with_body_sync` вовсе (тот же принцип «ни одного
  исходящего байта», что срезы 4/6/7/9/10 уже применяют к своим API), и вместо
  спавна потока пишет `(blocked_uri, original_policy)` в тот же однослотовый
  `last_csp_block: Arc<Mutex<Option<(String, String)>>>`, который срез 10 уже
  завёл для синхронных путей `fetch()` (beacon соединяется синхронно и по
  одному вызову за раз — тот же аргумент об одном слоте на рантайм). Новый
  натив `_lumen_beacon_last_csp_block()` читает и очищает слот.
- `crates/js/src/shim/web_api_shim_mid_b.js`: `navigator.sendBeacon` при
  `!ok` читает `_lumen_beacon_last_csp_block()` и зовёт уже существующий
  `_lumen_fire_connect_src_violation` (срез 10) — тот же общий хелпер, что
  fetch/XHR/WS/SSE уже используют. `sendBeacon` при блокировке возвращает
  `false` — по спеке "queue a request" внутри алгоритма беакона проваливается
  на CSP-проверке, значит запрос не поставлен в очередь.

Подтверждено тестами (без реального сетевого ввода-вывода): 3 юнит-теста в
`crates/network/src/lib.rs` (`connect_src_none_blocks_beacon_check_before_any_thread_is_spawned`,
`connect_src_allowed_host_passes_beacon_check`,
`no_connect_src_policy_does_not_block_beacon_check` — тот же "no real I/O"/
"no-policy-no-block" рисунок, что срезы 10/11 уже проверяли) + 4
интеграционных в `crates/js/src/dom/tests/v8_page_visibility_beacon.rs`
(`send_beacon_connect_src_block_returns_false`,
`send_beacon_connect_src_block_reaches_native_side_channel`,
`send_beacon_connect_src_block_fires_security_policy_violation_event`, плюс
существующий `send_beacon_with_provider_returns_true` остался зелёным без
изменений — не регрессия для незаблокированного пути). WPT
`content-security-policy/connect-src/connect-src-beacon-blocked.sub.html`
ожидает ровно это: `securitypolicyviolation` с
`violatedDirective=connect-src`, что теперь и происходит.
Подтверждено `cargo clippy -p lumen-network --all-targets -- -D warnings`
(чисто), `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` (чисто), `cargo build -p lumen-shell --features v8` +
`cargo clippy -p lumen-shell --all-targets --features v8 -- -D warnings`
(оба чисто).

Не покрыто этим срезом: директивы кроме `script-src`/`img-src`/`style-src`/
`connect-src`; `report-uri`/`report-to`; hash-источники; честная независимая
проверка заголовка и `<meta>`; картинки/скрипты/листы внутри `<iframe>` не
покрытые срезами 6/8; двойная доставка `securitypolicyviolation` для
дублирующихся продюсеров картинок (существующий паттерн, см. срез 4).
