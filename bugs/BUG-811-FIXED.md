# BUG-811 — CSP разбирается, но не применяется: ни одна директива ничего не блокирует и событие `securitypolicyviolation` не диспатчится никогда

**Статус:** FIXED 2026-09-25 — дрейф трекера: закрыт GAP-CSPENF (2026-09-20); строка BUGS.md не была перенесена при закрытии задачи (сверка с кодом 2026-09-25)
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

## Срез 13 (2026-09-17, P6) — `worker-src` против `new Worker()`/`new SharedWorker()`

Реализовано: пятая директива этого GAP и первая направленная не на fetch/XHR/
WS/SSE/beacon, а на конструирование воркера. `CspDirective::WorkerSrc` был
распарсен с самого начала (`crates/network/src/csp.rs`), но нигде не
проверялся — `grep -rn WorkerSrc crates/` до этого среза давал только парсер и
doc-comment `csp_enforce.rs`, называвший директиву непокрытой.

- Fallback-цепочка: CSP3 §6.4 формально даёт `worker-src` промежуточный шаг
  через `child-src`, затем `script-src`, и только потом `default-src`.
  `child-src` в этом кодовом дереве не распарсен вовсе (`grep -n child-src
  crates/network/src/csp.rs` — 0 совпадений), и ни одна другая директива здесь
  не проверяет многошаговый фолбэк — только прямой `effective_sources`,
  который уже общий для любой директивы (`.or_else(|| default-src)`). Этот
  срез не стал заводить `child-src` ради одного промежуточного звена: у
  `worker-src` тот же однократный фолбэк на `default-src`, что и у всех
  остальных директив в этом файле — уже, чем полный CSP3-алгоритм, но
  единообразно с img-src/script-src/style-src/connect-src, и это осознанно
  задокументированная граница, а не пропуск.
- Архитектурно этот срез — не срез 4/6/7/9 (`&Document` есть в коде страницы) и
  не 100% срез 10-12: `new Worker(url)`/`new SharedWorker(url)` резолвят и
  фетчят свой классический скрипт синхронно из нативного байндинга
  (`_lumen_worker_fetch_script`/`_lumen_sw_fetch_script`,
  `crates/js/src/worker.rs`/`shared_worker.rs`), у которого нет `&Document` —
  та же причина, по которой срез 10 переехал в `lumen-network`. Поэтому гейт
  живёт там же:
  - `crates/core/src/error.rs`: новый вариант `Error::CspWorkerSrcBlocked
    { blocked_uri, original_policy }`, отдельный от `CspConnectSrcBlocked` —
    разные директивы, разный `violatedDirective` на выходе.
  - `crates/core/src/ext.rs`: `JsFetchProvider::check_worker_src(url) ->
    Result<()>` — I/O-free пре-чек, тот же контракт, что `check_connect_src`
    (срез 12) даёт `sendBeacon`; default-реализация `Ok(())` для двойников без
    CSP.
  - `crates/network/src/lib.rs`: `HttpClient` получила `worker_src_policy:
    Option<(CspPolicy, Option<Origin>, String)>` — та же тройка, что и
    `connect_src_policy`, но отдельное поле (проверяется против другой
    директивы, `CspDirective::WorkerSrc` вместо `ConnectSrc`), новый билдер
    `with_worker_src_policy`, приватный `worker_src_gate(&Url)` и
    `check_worker_src` как override трейта.
  - `crates/shell/src/page_pipeline.rs::parse_and_layout`: тот же
    `document_csp_policy`, что срез 10 уже собирает для `connect_src_policy`,
    теперь клонируется и на `with_worker_src_policy` — один расчёт политики,
    два гейта.
  - `crates/js/src/worker.rs`/`shared_worker.rs`: `_lumen_worker_fetch_script`/
    `_lumen_sw_fetch_script` зовут `check_worker_src(&url)` **до**
    `fetch_worker_script` — заблокированный URL не долетает до сети вовсе (тот
    же принцип «ни одного исходящего байта», что и у всех предыдущих срезов).
    Отказ пишется в однослотовый side-channel (`last_csp_block`, тот же паттерн,
    что срезы 10/11/12 уже используют для `fetch`/WS/SSE/beacon — конструктор
    синхронный и по одному вызову за раз), читаемый новыми нативами
    `_lumen_worker_last_csp_block()`/`_lumen_sw_last_csp_block()`.
  - `crates/js/src/shim/web_api_shim_mid_b.js`: новый тонкий хелпер
    `_lumen_fire_worker_src_violation(csp)`, по образцу
    `_lumen_fire_connect_src_violation` (срез 10) — зовёт тот же
    `_lumen_dispatch_csp_violation('worker-src', …)`.
  - `WORKER_SHIM`/`SHARED_WORKER_SHIM` (внутри `worker.rs`/`shared_worker.rs`,
    не в общем шиме — у них своя JS-строка): в ветке «скрипт не загрузился»
    (уже существующей с BUG-364 для обычного сетевого отказа) читают side
    channel и зовут новый хелпер перед диспатчем `error` — тот же порядок
    «CSP-событие раньше сетевой ошибки», что срезы 4/6/9/11 уже дают
    картинкам/скриптам/WS. Отказ неотличим от обычного сетевого сбоя без
    side channel: оба дают `undefined` от `_lumen_*_fetch_script`, поэтому
    `Worker`/`SharedWorker` фейлится тем же путём, что и BUG-364 —
    `error`-событие, `_id`/порт остаются «никогда не запущен».
  - `HttpClient::fetch_sync`/`fetch_request` (через который
    `fetch_worker_script` реально ходит в сеть) уже проверяет `connect-src`
    в `fetch_request_impl` — то есть до этого среза классический воркер-скрипт
    *случайно* гасился чужой директивой (`connect-src`), если она была строже.
    После этого среза оба гейта действуют независимо: `worker-src` (или
    `default-src`) — до входа в `fetch_worker_script`, `connect-src` (или свой
    `default-src`) — внутри него, если первый пропустил. Не регрессия (страница
    строже не станет), но означает, что заблокированный `connect-src`-политикой
    воркер-скрипт по-прежнему не получит `violatedDirective=worker-src` —
    получит `connect-src`, если это единственная сработавшая директива;
    см. «Не покрыто» ниже.

Подтверждено 5 юнит-тестов в `crates/network/src/lib.rs`
(`worker_src_none_blocks_check_before_any_network_io`,
`worker_src_allowed_host_passes_check`,
`worker_src_falls_back_to_default_src`,
`no_worker_src_policy_does_not_block_check`,
`worker_src_unparseable_url_not_blocked` — тот же "before-any-network-io"/
"no-policy-no-block"/"fail-open-on-parse-error" рисунок, что срезы 10-12 уже
проверяли) + 7 интеграционных в `crates/js/src/dom/tests/v8_webworker.rs`
(`worker_src_block_reaches_native_side_channel`,
`worker_src_block_never_starts_worker_and_fires_onerror`,
`worker_src_block_fires_security_policy_violation_event`,
`shared_worker_src_block_reaches_native_side_channel`,
`shared_worker_src_block_fires_onerror`,
`shared_worker_src_block_fires_security_policy_violation_event` — полный
`securitypolicyviolation` с `violatedDirective=worker-src` и правильными
`blockedURI`/`originalPolicy`, не только сам side channel). Подтверждено
`cargo clippy -p lumen-network --all-targets -- -D warnings` (чисто),
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
(чисто), `cargo build -p lumen-shell --features v8` + `cargo clippy -p
lumen-shell --all-targets --features v8 -- -D warnings` (оба чисто).

Не покрыто этим срезом: `importScripts()` внутри уже запущенного воркера
(гейтится только начальный скрипт конструктора — `resolve_import_url` в
`worker.rs` не тронут); `blob:`/`data:` worker-скрипты (уже опаковые URL,
`worker-src` по CSP3 их не матчит источниками — вне логики фолбэка на
`default-src`, отдельно не проверялось); `child-src`/`script-src`
промежуточный фолбэк CSP3 §6.4 (см. выше — используется только прямой фолбэк
на `default-src`, как и у всех остальных директив здесь); неразличимость
`worker-src`- и `connect-src`-блокировки при срабатывании обеих (см. выше —
`violatedDirective` может уйти как `connect-src`, если тот гейт сработал
первым внутри `fetch_worker_script`); `ServiceWorker` (`sw_worker.rs` — своя
регистрация, `worker-src` по CSP3 её не покрывает, это отдельная директива нет
в этом кодовом дереве); директивы кроме `script-src`/`img-src`/`style-src`/
`connect-src`/`worker-src`; `report-uri`/`report-to`; hash-источники; честная
независимая проверка заголовка и `<meta>`; картинки/скрипты/листы внутри
`<iframe>` не покрытые срезами 6/8.

## Срез 14 (2026-09-17, P6) — доставка отчётов `report-uri`

Реализовано (`crates/js/src/csp.rs`, JS-only — единственная точка изменений):
`_lumen_dispatch_csp_violation` — уже единственная точка диспетчеризации
`securitypolicyviolation` для всех пяти директив (срезы 1-13, script-src/
img-src/style-src из `crates/shell`, connect-src/worker-src через
side-channel `lumen-network`), и каждый вызывающий её путь уже передаёт
`originalPolicy` (объединённый текст заголовка + `<meta>`, `document_csp_policy`
из `csp_enforce.rs`). `CspPolicy.report_uri` разобран на Rust-стороне с самого
начала (`crates/network/src/csp.rs:135`), но ни разу не пересекал границу
Rust/JS ни к одной из пяти точек нарушения — плюс к самой границе прибавился
бы шестой провод (после script-src/img-src/style-src/connect-src/worker-src).
Вместо этого — переизвлечение `report-uri` из уже доехавшей строки
`originalPolicy` регулярным выражением `/(?:^|;)\s*report-uri\s+([^;]+)/i`:
одна точка изменений вместо пяти.

Новая функция `_lumen_send_csp_reports(originalPolicy, evt)` строит JSON-тело
`csp-report` (CSP2 §5, поля `document-uri`/`referrer`/`violated-directive`/
`effective-directive`/`original-policy`/`disposition`/`blocked-uri`/
`status-code` — те же, что уже несёт `SecurityPolicyViolationEvent`) и шлёт
`fetch(..., {method:'POST', headers:{'Content-Type':'application/csp-report'}})`
на каждый URI из директивы (несколько URI — несколько POST), резолвя их
против `document.baseURI`. Отказ `fetch` (сеть недоступна, эндпоинт не
отвечает) молча проглатывается (`.catch(() => {})`) — доставка отчёта не
имеет наблюдаемого эффекта на странице по спеке. Если `fetch`/`URL` не
определены в рантайме (воркер-контекст, тестовый стаб), функция тихо
возвращается — тот же "no observable effect" принцип, не бросает.

Живой пробой (`--mcp-live-port`, локальный HTTP-сервер, `dev-release`):
страница с `<meta http-equiv="Content-Security-Policy" content="script-src
'none'; report-uri /csp-report">` и инлайн-скриптом — сервер получает POST
`/csp-report` с `Content-Type: application/csp-report` и телом
`{"csp-report":{"document-uri":"http://127.0.0.1:8199/page","violated-directive":"script-src",
"effective-directive":"script-src","original-policy":"script-src 'none'; report-uri /csp-report",
"disposition":"enforce","blocked-uri":"inline","status-code":0}}` — `referrer`
отсутствует в JSON, потому что `document.referrer` в этом движке сегодня
`undefined`, не пустая строка, и `JSON.stringify` опускает `undefined`-поля;
не регрессия этого среза. +5 unit-тестов в `crates/js/src/csp.rs`
(`report_uri_posts_report_to_endpoint`, `report_uri_posts_to_every_listed_endpoint`,
`no_report_uri_sends_no_reports`, `report_uri_without_fetch_does_not_throw`).

Не покрыто этим срезом: `report-to` (Reporting API) — нужны группы эндпоинтов
из заголовка `Report-To`, который этот движок не разбирает, заметно большая
задача; `Content-Security-Policy-Report-Only` (report-only политика вообще не
доезжает до `document_csp_policy` — см. `csp_enforce.rs`, отдельная задача);
отчёты для нарушений, обнаруженных до появления JS-рантайма (streaming-продюсер
картинок, `page_load.rs` — см. срез 4, тот же провал, что и у самого события);
директивы кроме `script-src`/`img-src`/`style-src`/`connect-src`/`worker-src`;
hash-источники; честная независимая проверка заголовка и `<meta>`;
картинки/скрипты/листы внутри `<iframe>` не покрытые срезами 6/8.

## Срез 15 (2026-09-18, P6) — `frame-src` против навигации `<iframe>`/`<frame>`

Ещё одна директива без гейта: `crates/shell/src/csp_enforce.rs` проверял
`script-src`/`img-src`/`style-src`, но ни один код не читал `frame-src` — CSP
любой строгости не мешала фрейму навигироваться на произвольный источник.
Добавлена `frame_src_blocked` (`csp_enforce.rs`) — тот же host/scheme/`'self'`
фетч-гейт, что уже даёт `img_src_blocked`/`script_src_blocked`/
`style_src_blocked` (срезы 4/6/7), под директиву `CspDirective::FrameSrc`
(fallback на `default-src`, уже разбирается `crates/network/src/csp.rs`).

Гейт живёт не в `csp_enforce.rs`, а в `frames.rs::spawn_frame` (тот же повод,
что у срезов 10-13 для `connect-src`/`worker-src`: у гейта нет готового
`&Document`/`ResourceBase` внутри самого `csp_enforce.rs`, а `spawn_frame`
уже держит и родительский `Document`, и обе `ResourceBase` для резолва
относительного `src`). Проверяются оба пути, которыми фрейм получает адрес:
первичная вставка разметки (`info.src`, база — `base` родителя) и навигация
(`dest: Some((href, nav_base))` — клик по ссылке внутри старого под-документа,
скриптовое переприсваивание `.src`) — оба идут через один и тот же
`fetch_iframe_source`, гейт стоит перед обоими вызовами. `about:blank` и
пустой `src` не проверяются: CSP3 §6.5 их не ограничивает, `fetch_iframe_source`
и без гейта не долетает для них ни до сети, ни до диска (короткое замыкание
раньше резолва). Заблокированная навигация не обрывает загрузку фрейма — тот
же путь, что FRAME-4 срез 2 уже даёт сетевым неудачам: `FetchError` с текстом
причины превращается в синтетическую страницу «Не удалось загрузить фрейм»,
и `securitypolicyviolation` летит через уже существующий `PersistentJs::
fire_csp_violation("frame-src", …)` (родительский `parent_js` — фрейм
встроен в родительский документ, нарушение принадлежит ему).

+10 unit-тестов в `crates/shell/src/csp_enforce.rs` (симметрично `style_src_*`).
Живой пробой (`--dump-layout`, headless): страница с `<meta http-equiv=
"Content-Security-Policy" content="frame-src 'self'">` и двумя `<iframe>` —
один на свой origin (`/local.html`), другой на чужой (`http://127.0.0.1:8299/
other.html`) — лог фетчей показывает `GET /local.html`, но **ни одного**
`GET` на порт 8299; без директивы (baseline-прогон той же страницы без CSP)
оба фрейма фетчатся. Подтверждено `cargo clippy -p lumen-shell --all-targets
--features v8 -- -D warnings` (чисто) и `cargo test -p lumen-shell --features
v8 csp_enforce` (26/26 зелёных).

Не покрыто этим срезом: `frame-src` на верхнеуровневую навигацию (директива
относится только к вложенным browsing context, топ-уровня не касается);
`frame-ancestors` (обратная директива — ограничивает, КТО может встраивать
ЭТУ страницу, отдельная проверка на встраиваемой стороне, не тронута);
остальные директивы (`object-src`/`media-src`/`manifest-src`/…); `report-to`;
hash-источники; честная независимая проверка заголовка и `<meta>`.

## Срез 16 (2026-09-19, P6) — `object-src` против `<embed src>`/`<object data>`

Реализовано: `object-src` (CSP3 §6.4, покрывает `<embed>`/`<object>`) была
разобрана с самого начала (`crates/network/src/csp.rs`), но нигде не
проверялась, тем же паттерном, что срез 13 уже закрыл для `worker-src`.

Архитектурно этот срез — не срез 4/6/7/9/15 (`&Document` есть внутри шелла) и
похож на срезы 10-13: `<embed src>`/`<object data>` целиком грузятся из
JS-шима (`_lumen_embed_object_reload`, `crates/js/src/shim/web_api_shim_mid.js`,
переиспользует `<link>`-хинтовый `fetch()` — BUG-798), у которого нет
`&Document`. Поэтому гейт живёт в `lumen-network`, той же тройкой, что срезы
10/12/13 уже дали `connect-src`/`worker-src`:

- `crates/core/src/error.rs`: новый вариант `Error::CspObjectSrcBlocked
  { blocked_uri, original_policy }`.
- `crates/core/src/ext.rs`: `JsFetchProvider::check_object_src(url) ->
  Result<()>` — I/O-free пре-чек, тот же контракт, что `check_worker_src`
  (срез 13); default-реализация `Ok(())` для двойников без CSP.
- `crates/network/src/lib.rs`: `HttpClient` получила `object_src_policy:
  Option<(CspPolicy, Option<Origin>, String)>`, билдер `with_object_src_policy`,
  приватный `object_src_gate(&Url)` (против `CspDirective::ObjectSrc`,
  fallback на `default-src` — общий для любой директивы) и `check_object_src`
  как override трейта; +5 unit-тестов (симметрично `worker_src_*`).
- `crates/shell/src/page_pipeline.rs::parse_and_layout`: тот же
  `document_csp_policy`, что уже собирается для `connect_src_policy`/
  `worker_src_policy`, теперь клонируется и на `with_object_src_policy` — один
  расчёт политики, три гейта на один `HttpClient`.
- `crates/js/src/v8_runtime/install/net.rs`: новый нативный биндинг
  `_lumen_check_object_src(url) -> bool` в `install_fetch` — зовёт
  `check_object_src` и, если заблокировано, пишет `(blocked_uri,
  original_policy)` в свой однослотовый side-channel (тот же паттерн, что
  срезы 10/12 уже используют для `fetch`/beacon — вызывающий JS синхронный и
  по одному разу за загрузку ресурса), читаемый `_lumen_object_src_last_csp_block()`.
- `crates/js/src/shim/web_api_shim_mid_b2.js`: `_lumen_fire_object_src_violation(csp)`
  — тот же тонкий хелпер, что `_lumen_fire_worker_src_violation` (срез 13),
  зовёт `_lumen_dispatch_csp_violation('object-src', …)`.
- `crates/js/src/shim/web_api_shim_mid.js::_lumen_embed_object_reload`: после
  резолва URL и до вызова `_lumen_link_hint_fetch` зовёт
  `_lumen_check_object_src(url)` — заблокированный URL не долетает до `fetch()`
  вовсе (тот же принцип «ни одного исходящего байта», что и у всех
  предыдущих срезов), диспатчит `securitypolicyviolation` и `error` на
  элементе тем же путём, что и сетевой отказ (BUG-798).

Подтверждено живым окном (`--mcp-live-port`, локальный HTTP-сервер,
`dev-release`): страница с `<meta http-equiv="Content-Security-Policy"
content="object-src 'none'">` и `<embed src="pixel.png">` — сетевой лог
(`resource://network`) пуст (ни одного `GET /pixel.png`), консоль печатает
`PROBE spv directive=object-src uri=http://127.0.0.1:8399/pixel.png
policy=object-src 'none'` и `PROBE embed-error`. Та же страница без директивы
(baseline) фетчит `pixel.png` (`GET` со статусом 200) и печатает
`PROBE embed-load`. Юнит-тесты: `cargo test -p lumen-network object_src`
(5/5). `cargo clippy -p lumen-core -p lumen-network --all-targets -- -D
warnings`, `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings`, `cargo clippy -p lumen-shell --all-targets --features v8 -- -D
warnings` — все чисто.

Не покрыто этим срезом: `<applet>` (устаревший тег, не поддержан этим
движком вовсе — вне области); директивы кроме `script-src`/`img-src`/
`style-src`/`connect-src`/`worker-src`/`frame-src`/`object-src`
(`media-src`/`manifest-src`/…); `frame-src` top-level; `frame-ancestors`;
`report-to`; hash-источники; честная независимая проверка заголовка и
`<meta>`; `<embed>`/`<object>` внутри `<iframe>` — `document_csp_policy` в
`page_pipeline.rs::parse_and_layout` строится только для top-level документа,
подфрейм получает свой `HttpClient` отдельно (`frames.rs`), не тронутый этим
срезом.

## Срез 17 (2026-09-19, `p6-gap-cspenf-srez17`) — `media-src` против `<video>`/`<audio>`/`<track>`

Реализовано: `media-src` (CSP3 §6.1, покрывает `<video>`, `<audio>` и связанные
с ними текстовые дорожки `<track>`) разбиралась с самого начала
(`crates/network/src/csp.rs:84`/`:354`), но нигде не проверялась — `grep -rn
MediaSrc crates/` до этого среза давал только парсер и doc-comment
`csp_enforce.rs`, называвший директиву непокрытой. Тем же паттерном, что срезы
13/16 закрыли `worker-src`/`object-src`.

Этот движок не несёт полного медиа-стека, поэтому первым шагом был поиск точек,
которые реально выпускают байты, а не перечисление тегов. Их оказалось **четыре**
у трёх разных владельцев, и одна из них — не та, что предполагалась:

- `<video src>`/`<source src>`: `startFetch` → `startGifLoad` →
  `__lumen_video_load(nid, src)` (`crates/js/src/video_bindings.rs`) —
  реальное декодирование только GIF, фетчит шелл из `pending_loads`.
- `<audio src>`: `startLoad` → `__lumen_audio_load(handle, url)`
  (`crates/js/src/audio_element.rs`) → `PlatformAudioPlayer::load` →
  `fetch_audio_bytes` на фоновом потоке шелла. Вопреки ожиданию «аудио
  заглушено сильнее видео» — это полноценный сетевой путь.
- `<track src>`: `readTrackBody` → обычный `fetch()` (`video_bindings.rs`).
- `<track src>` **второй раз**: `tracks::load_video_tracks` через
  `fetch_vtt_text` (`crates/shell/src/page_pipeline.rs`,
  `crates/shell/src/subresources.rs`) — оверлейный снапшот шелла, который
  ходит за тем же `.vtt` до появления JS вообще. В заявке на срез этой точки не
  было; она вскрылась живым прогоном: гейт только в шиме давал `spv` с
  `violatedDirective=media-src`, а `GET /cap.vtt` при этом всё равно уходил на
  провод. Инвариант «ни одного исходящего байта» держится только когда
  перекрыты обе половины.

Первые три точки JS-шимовые и без `&Document`, поэтому их гейт живёт в
`lumen-network`, той же тройкой, что срезы 10/12/13/16:

- `crates/core/src/error.rs`: новый вариант `Error::CspMediaSrcBlocked
  { blocked_uri, original_policy }`.
- `crates/core/src/ext.rs`: `JsFetchProvider::check_media_src(url) -> Result<()>`
  — I/O-free пре-чек, тот же контракт, что `check_object_src` (срез 16);
  default-реализация `Ok(())` для двойников без CSP.
- `crates/network/src/lib.rs`: `HttpClient` получила `media_src_policy:
  Option<(CspPolicy, Option<Origin>, String)>`, билдер `with_media_src_policy`,
  приватный `media_src_gate(&Url)` (против `CspDirective::MediaSrc`, fallback
  на `default-src` — общий для любой директивы) и `check_media_src` как
  override трейта; +5 unit-тестов (симметрично `object_src_*`). Отдельное поле,
  а не переиспользование `object_src_policy`: другая директива. Заметная
  особенность по сравнению с срезами 10-13 — ни один из трёх путей не гонит
  свои байты через этот самый `HttpClient` (видео отдаёт URL GIF-стору шелла,
  аудио — фоновому потоку с собственным `HttpClient`, трек — обычному
  `fetch()`), так что политика здесь консультируется **только** как пре-чек.
- `crates/js/src/v8_runtime/install/net.rs`: нативный биндинг
  `_lumen_check_media_src(url) -> bool` в `install_fetch` — зовёт
  `check_media_src` и при блокировке пишет `(blocked_uri, original_policy)` в
  свой однослотовый side-channel, читаемый `_lumen_media_src_last_csp_block()`
  (тот же паттерн, что `_lumen_check_object_src`). Слот один на все три
  JS-точки: каждая вызывает пару «проверь — прочитай» синхронно и по одному
  разу за загрузку ресурса.
- `crates/js/src/shim/web_api_shim_mid_b2.js`:
  `_lumen_fire_media_src_violation(csp)` — тот же тонкий хелпер, что
  `_lumen_fire_object_src_violation` (срез 16), зовёт
  `_lumen_dispatch_csp_violation('media-src', …)`. Живёт в общем шиме, а не в
  `VIDEO_SHIM`/`AUDIO_SHIM` (у обоих своя JS-строка), именно потому что зовут
  его все три.
- `crates/js/src/video_bindings.rs::startFetch`: гейт после `loadstart` и
  **до** `startGifLoad`, то есть до того, как URL попадёт в `pending_loads`;
  отказ идёт уже существующим путём `failResource` (BUG-825) — `error` на
  элементе либо на текущем `<source>` с переходом к следующему кандидату.
  Гейт стоит впереди проверки формата, а не позади: заблокированный источник —
  это отказ CSP независимо от того, какой у него был контейнер, поэтому
  не-GIF `src` теперь сообщает `media-src`, а не «unsupported media format».
- `crates/js/src/video_bindings.rs::readTrackBody`: гейт до `fetch(abs)`;
  отказ — `Promise.reject`, который уходит в уже существующий
  `failTrackLoad` (`readyState = ERROR` + `error` на `<track>`).
- `crates/js/src/audio_element.rs::startLoad`: гейт до
  `__lumen_audio_load` — это последняя синхронная точка, где ещё ничего не
  запрошено (дальше шелл уходит в `thread::spawn`), та же причина, по которой
  срез 12 поставил гейт `sendBeacon` перед его `thread::spawn`. URL резолвится
  против базы документа **только для проверки**: в загрузчик атрибут уходит
  сырым (существующее ограничение этого пути, к CSP отношения не имеющее — из-за
  него относительный `<audio src>` в этом движке и так не доходит до сети,
  `Url::parse` падает), но матчить политику имеет смысл только против
  абсолютного URL.

Четвёртая точка — в шелле, у неё есть `&Document`, поэтому она гейтится там же,
где `img-src`/`style-src` (срезы 4/7):

- `crates/shell/src/csp_enforce.rs`: новая `media_src_blocked` — тот же
  host/scheme/`'self'` фетч-гейт, что `img_src_blocked`/`frame_src_blocked`.
- `crates/shell/src/page_pipeline.rs`: замыкание `tracks::load_video_tracks`
  резолвит `src`, спрашивает `media_src_blocked` и возвращает `None` вместо
  вызова `fetch_vtt_text`; заблокированные URL копятся и диспатчатся через уже
  существующий `PersistentJs::fire_csp_violation("media-src", …)` — тот же
  отложенный диспатч, что срез 4 применил к `blocked_by_img_src`, потому что
  блокировка произошла раньше, чем появился JS-рантайм.
- `crates/shell/src/page_pipeline.rs::parse_and_layout`: тот же
  `document_csp_policy`, что уже собирается для `connect_src_policy`/
  `worker_src_policy`/`object_src_policy`, теперь клонируется и на
  `with_media_src_policy` — один расчёт политики, четыре гейта на один
  `HttpClient`.

Подтверждено живым окном (`--mcp-live-port`, локальный HTTP-сервер с логом
каждого запроса, `dev-release`), A/B двумя армами одной страницы. Арм с
`<meta http-equiv="Content-Security-Policy" content="media-src 'none'">`:
серверный лог содержит только `GET /page.html`, `resource://network` пуст, а
консоль печатает три нарушения — `spv directive=media-src
uri=http://127.0.0.1:8501/clip.gif`, и **дважды** `uri=.../cap.vtt` (по одному
на каждую из двух заблокированных попыток фетча трека — шелловскую и шимовую).
`new Audio('http://127.0.0.1:8501/tone.mp3')` на той же странице даёт
`spv directive=media-src uri=http://127.0.0.1:8501/tone.mp3`, `networkState`
остаётся `NETWORK_EMPTY` и на провод не уходит ничего. Baseline-арм той же
страницы без директивы: `GET /clip.gif`, `GET /cap.vtt` (трижды — оба
владельца плюс перезагрузка), `GET /tone.mp3`, событие `video-loadeddata`
(GIF реально декодировался) и ни одного `securitypolicyviolation`.

Юнит-тесты: `cargo test -p lumen-network media_src` (5/5 —
`media_src_none_blocks_check_before_any_network_io`,
`media_src_allowed_host_passes_check`, `media_src_falls_back_to_default_src`,
`no_media_src_policy_does_not_block_check`,
`media_src_unparseable_url_not_blocked`) и `cargo test -p lumen-shell
--features v8 csp_enforce` (32/32, +6 новых `media_src_*`, включая
`img_src_none_does_not_block_media` — строгая соседняя директива не должна
подменять `media-src`). `cargo clippy -p lumen-core -p lumen-network
--all-targets -- -D warnings`, `cargo clippy -p lumen-js --all-targets
--features v8-backend -- -D warnings`, `cargo clippy -p lumen-shell
--all-targets --features v8 -- -D warnings` — все чисто. Регрессий в
существующих медиа-тестах нет: `cargo test -p lumen-js --features v8-backend
audio` (50/50), `… video_bindings` (40/40).

Не покрыто этим срезом: `securitypolicyviolation` для **разметочного**
`<audio src>` — сам байт не уходит, но событие не наблюдаемо. `AUDIO_SHIM`
патчит разметочные `<audio>` одним проходом `document.querySelectorAll('audio')`
в момент установки шима, то есть `startLoad` (и вместе с ним синхронный
диспатч нарушения) успевает раньше, чем страница может повесить слушатель;
`error` при этом доезжает, потому что он на `setTimeout(0)`. Скриптовые пути
(`new Audio(url)`, `audio.src = …`, `createElement('audio')`) наблюдаемы
полностью — подтверждено выше. Это тот же класс провала, что срез 4
задокументировал для потокового продюсера картинок, а не новое следствие
гейта. Далее: `blob:`/`data:` тела `<track>` (читаются локально из
object-URL-стора, до сети не доходят вовсе — гейтится только сетевая ветка
`readTrackBody`); MSE/`srcObject` (в этом движке нет); `<video poster>`
(по CSP3 это `img-src`, не `media-src` — отдельная поверхность, не тронута);
директивы кроме `script-src`/`img-src`/`style-src`/`connect-src`/`worker-src`/
`frame-src`/`object-src`/`media-src` (`manifest-src`/`font-src`/`child-src`/…);
`frame-src` top-level; `frame-ancestors`; `report-to`; hash-источники;
честная независимая проверка заголовка и `<meta>`; медиа внутри `<iframe>` —
`document_csp_policy` в `page_pipeline.rs::parse_and_layout` строится только
для top-level документа, подфрейм получает свой `HttpClient` отдельно
(`frames.rs`), не тронутый этим срезом.

## Срез 18 (2026-09-19, `p6-gap-cspenf-srez18`) — `img-src` против `background-image: url(...)`

Реализовано: последний из трёх производителей, названных срезом 4 как не
покрытые («`background-image`/`@font-face url()` — используют
`fetch_image_bytes` напрямую, не `decode_image` — не гейтятся вовсе»), из
которых `background-image` — единственный, что фетчит байты страницы (а не
подресурс, к которому нет доступа `&Document`).

- `crates/shell/src/subresources.rs::fetch_and_decode_background_images`
  получила параметр `csp_gate: Option<(&CspPolicy, Option<&Origin>)>` и
  второй элемент возврата — `Vec<String>` заблокированных резолвленных URL,
  тем же контрактом, что срезы 7/17 уже дали `load_linked_stylesheets`/
  `load_video_tracks`. Внутри `parallel_map` каждый URL сначала резолвится
  (`base.resolve`), затем проверяется уже существующим
  `csp_enforce::img_src_blocked` (срез 4) — тем же гейтом, что и `<img src>`,
  раз CSP3 §6.7 не различает происхождение картинки внутри одной директивы;
  заблокированный URL не доходит до `fetch_image_bytes` вовсе (тот же принцип
  «ни одного исходящего байта»).
- `crates/shell/src/page_pipeline.rs::parse_and_layout`: политика документа
  считается один раз (тот же `document_csp_policy`, что срезы 4/7/9/17 уже
  используют) перед вызовом фетч-функции; заблокированные URL диспатчат
  `securitypolicyviolation` (`directive="img-src"`) после параллельного
  фетча — та же отложенная one-shot-push схема, что срез 4 применяет к
  `blocked_by_img_src`, только здесь "отложено" означает "после фетча",
  а не "до JS-рантайма" (JS-рантайм к этому моменту уже существует).
- `frames.rs::fetch_frame_background_images` (фон под-документа `<iframe>`) —
  отдельная, не переиспользующая функция; не тронута этим срезом, тот же
  пробел, что срезы 4/6/8 уже документировали для картинок/скриптов фрейма
  до их собственного среза.

Подтверждено живым окном (`--mcp-live-port`, локальный HTTP-сервер с логом
каждого запроса, `dev-release`), A/B двумя страницами. Арм с `<meta
http-equiv="Content-Security-Policy" content="img-src 'none'">` и
`#bg { background-image: url(bg.png) }`: серверный лог содержит только `GET
/page.html` (`bg.png` не запрошен вовсе), консоль печатает `PROBE spv
directive=img-src uri=http://127.0.0.1:8511/bg.png`. Baseline-арм той же
страницы без директивы: `GET /bg.png` уходит на провод, `securitypolicyviolation`
не приходит.

Тесты: новая логика — тонкая проводка (резолв + вызов уже протестированного
`img_src_blocked`), отдельных unit-тестов не заводилось — тот же прецедент,
что срез 9 уже принял для `loading="lazy"` картинок (гейт там тоже сведён к
переиспользованию `img_src_blocked`, подтверждён только живым пробом и
регрессионным прогоном). `cargo test -p lumen-shell --features v8 --bin
lumen` без регрессий, `cargo build -p lumen-shell --features v8` + `cargo
clippy -p lumen-shell --all-targets --features v8 -- -D warnings` — чисто.

Не покрыто этим срезом: `@font-face url()` (использует `fetch_font_bytes`,
отдельный путь, не тронут); `background-image` внутри `<iframe>`
(`frames.rs`, см. выше); директивы кроме `script-src`/`img-src`/`style-src`/
`connect-src`/`worker-src`/`frame-src`/`object-src`/`media-src`; `frame-src`
top-level; `frame-ancestors`; `report-to`; hash-источники; честная
независимая проверка заголовка и `<meta>`.

## Срез 19 (2026-09-19, `p6-gap-cspenf-srez19`) — `font-src` против `@font-face url()`

Реализовано: `font-src` (CSP3 §6.1, покрывает `@font-face src`) не была даже
распарсена — `grep -n FontSrc crates/network/src/csp.rs` до этого среза давал
ноль совпадений, а не «распарсена, но не проверяется», как у предыдущих
директив (срезы 13/16/17). Добавлена `CspDirective::FontSrc` (парсер, тот же
generic-фолбэк на `default-src`, что и у всех остальных директив здесь — не
нужно ничего сверх строки `"font-src" => CspDirective::FontSrc` в
`crates/network/src/csp.rs`) и `csp_enforce::font_src_blocked` — тот же
host/scheme/`'self'` фетч-гейт, что уже есть у `img_src_blocked`/
`media_src_blocked` (срезы 4/17); +5 unit-тестов симметрично `media_src_*`.

Единственная точка, реально фетчащая байты `@font-face url()` страницы, —
`crates/shell/src/page_load.rs::apply_loaded_page`, PH3-19: цикл по
`page.pending_web_fonts`, каждый источник грузится на детач-`std::thread::spawn`
(асинхронно, чтобы не держать первый paint — FOUT). У детач-потока нет
`&self`, то есть нет `js_ctx`/`engine_thread` для диспатча
`securitypolicyviolation` изнутри него (та же причина, по которой срезы 10-13/
16/17 переносили гейт в `lumen-network` — здесь переносить некуда, точка
решения «фетчить или нет» синхронная и на главном потоке). Поэтому политика
документа (`document_csp_policy`, тот же `self.layout_source`, что срез 9 уже
читает для `loading="lazy"`) считается один раз до цикла, а сам
`font_src_blocked`-чек стоит **до** `std::thread::spawn`: заблокированный URL
не долетает до `fetch_font_bytes` вовсе (принцип «ни одного исходящего
байта»), нарушение диспатчится через `route_task_js`/`fire_csp_violation`
(тот же путь, что срез 9 использует для заблокированной lazy-картинки), и
поток для этого источника просто не порождается — шрифт не регистрируется в
`page_font_registry`, каскад падает на системный фолбэк тем же путём, что и
любой другой `@font-face`, который не успел/не смог загрузиться.

Не покрыто этим срезом: `@font-face url()` внутри `<iframe>`
(`frames.rs::load_frame_fonts` — синхронная загрузка фреймовых шрифтов, не
тронута, тот же пробел, что срезы 4/6/8/16 документировали для
картинок/скриптов/object-src до их собственного среза); директивы кроме
`script-src`/`img-src`/`style-src`/`connect-src`/`worker-src`/`frame-src`/
`object-src`/`media-src`/`font-src` (`manifest-src`/`child-src`/…);
`frame-src` top-level; `frame-ancestors`; `report-to`; hash-источники;
честная независимая проверка заголовка и `<meta>`.

Подтверждено `cargo build -p lumen-shell --features v8` + `cargo clippy -p
lumen-shell -p lumen-network --all-targets --features v8 -- -D warnings`
(оба чисто) + `cargo test -p lumen-shell --features v8 csp_enforce` (38/38,
+6 новых `font_src_*`) + `cargo test -p lumen-network font_src` (без
регрессий — новых Rust-тестов в `lumen-network` не заводилось, парсинг
проверен через `csp_enforce`'s unit-тесты).

## Срез 20 (2026-09-19, `p6-gap-cspenf-srez20`) — `'sha256-…'`/`'sha384-…'`/`'sha512-…'` hash-источники для инлайновых скриптов

Реализовано: `CspSource::Hash` разбирался (`crates/network/src/csp.rs`) с
самого среза 1, но ни разу не участвовал в проверке — `inline_script_blocked`
(`crates/shell/src/csp_enforce.rs`) сопоставляла только `'unsafe-inline'` и
`'nonce-…'`, так что инлайновый скрипт под политикой, единственный
разрешённый источник которой — хэш (`script-src 'sha256-…'`, обычная форма
для страниц без nonce-инфраструктуры), читался как всегда заблокированный —
тот же класс «объявлено, не доставлено», что и остальная задача, только на
уровне одного источника внутри уже enforced директивы, а не целой директивы.

- `HashAlgorithm::digest_base64` (`crates/network/src/csp.rs`) — считает
  SHA-256/384/512 (`sha2`, уже в дереве зависимостей `lumen-network` через
  TLS-цепочку сертификатов, отдельного добавления в `Cargo.toml` не
  понадобилось) и кодирует в base64 тем же `lumen_core::hash::base64_encode`,
  что уже даёт WebSocket `Sec-WebSocket-Accept`. Сравнение — байт-в-байт со
  значением из политики в исходной кодировке (стандартный base64, не
  base64url); объявленное значение в другой кодировке не совпадёт — тот же
  принцип «не изобретать эквивалентность», что уже стоит на непарсящихся URL
  в `img_src_blocked`/`font_src_blocked`.
- `inline_script_blocked` получила третий параметр `body: &str` — тело
  инлайнового скрипта, уже доступное в обоих вызывающих местах
  (`scripts.rs`: классический скрипт как `src` из `ResolvedScript`, модуль —
  `item.source`) без дополнительного чтения `Document`. Ветка `CspSource::Hash
  { algorithm, value }` добавлена в тот же `.any()`, что уже перебирает
  `UnsafeInline`/`Nonce` — совпадение любого источника (nonce ИЛИ хэш ИЛИ
  `'unsafe-inline'`) допускает исполнение.
- Событийные обработчики (`onclick=…`) и `style-src`-хэши (`'unsafe-hashes'`,
  CSP3 §8.1 отдельно оговаривает их) не тронуты этим срезом — только тело
  `<script>`/module script, та же граница, что и у nonce-проверки с среза 1.

Тесты: 4 новых unit-теста в `csp_enforce.rs` — совпадающий/несовпадающий
`sha256`, совпадающий `sha384` (доказывает, что не захардкожен один
алгоритм), и политика с nonce ПЛЮС хэш-источником (доказывает, что цикл
сопоставления не останавливается на первом виде источника). Тестовые
дайджесты посчитаны независимо (`hashlib` Python) для тел `alert(1)`/
`alert(2)`, не самим кодом под тестом.

Подтверждено `cargo build -p lumen-network -p lumen-shell --features v8` +
`cargo clippy -p lumen-network --all-targets -- -D warnings` +
`cargo clippy -p lumen-shell --all-targets --features v8 -- -D warnings`
(оба чисто) + `cargo test -p lumen-shell --features v8 --bin lumen
csp_enforce` (42/42, +4 новых). Живой проб не делался — тот же прецедент, что
срез 18 уже принял для тонкой match-веточной проводки без сетевого
наблюдаемого эффекта; здесь эффект и вовсе внутри одного процесса (нет
сети), unit-тест — исчерпывающая проверка.

Не покрыто этим срезом: `'unsafe-hashes'`/событийные обработчики,
`style-src`-хэши, остальное — без изменений (см. список выше).

## Срез 21 (2026-09-19, `p6-gap-cspenf-srez21`) — `style-src` против инлайновых `<style>`

Реализовано: последний класс инлайна, названный не покрытым срезом 7
(«инлайновые `<style>`/атрибут `style` (не блокируются, только внешний
`<link>`)») — инлайновый `<style>` до этого среза не проверялся вовсе, то
есть страница со `style-src` любой строгости не могла запретить собственный
`<style>`-блок, только внешний лист.

- `crates/shell/src/csp_enforce.rs`: `inline_script_blocked` (срезы 1/20)
  разложена на общий `inline_directive_blocked(policy, directive, nonce,
  body)` — тот же `'unsafe-inline'`/`'nonce-…'`/`'sha256-…'`-набор
  источников, что уже есть у скриптов, теперь под именем
  `inline_style_blocked` применён к `CspDirective::StyleSrc`; +6 unit-тестов
  (совпадающий/несовпадающий nonce, хэш, `'unsafe-inline'`, и один
  cross-check — `'unsafe-inline'` в `script-src` не открывает `style-src`).
- `crates/shell/src/doc_extract.rs`: `extract_style_blocks`
  (`walk_style_blocks`) — единственное место, где текст всех инлайновых
  `<style>` страницы склеивается в один каскад, — получила параметр
  `csp_gate: Option<&CspPolicy>` и второй элемент возврата (`usize` —
  число заблокированных узлов). Каждый `<style>`-узел проверяется
  независимо (свой `nonce`, тело для хэша) **до** склейки: заблокированный
  узел не попадает в текст, который парсит `lumen_css_parser`, вовсе — тот
  же принцип «не применённый CSS», что срез 7 уже даёт заблокированному
  внешнему `<link>`. +2 unit-теста (склейка пропускает только
  заблокированный узел; без политики ничего не режется).
- `crates/shell/src/page_pipeline.rs`: `build_page_cascade` считает
  `document_csp_policy` той же одноразовой точкой, что остальные срезы уже
  используют (не параметром — не нужно менять сигнатуру ради одного гейта),
  передаёт её в `extract_style_blocks`; `PageCascade` получила
  `blocked_inline_style_count: usize` (без URL — `blockedURI` для инлайна
  всегда `"inline"`, как у скриптов, счётчика достаточно). После того как
  JS-рантайм появляется, диспатчится `securitypolicyviolation` по одному на
  заблокированный узел — та же one-shot-push схема, что `blocked_by_style_src`
  (срез 7) уже применяет к внешним листам.
- `crates/shell/src/relayout.rs::refresh_dynamic_css` (BUG-743, поздняя
  CSS-in-JS вставка `<style>` после навигации) получила тот же гейт — без
  него скрипт мог бы обойти `style-src`, просто вставив стиль после
  парсинга вместо разметки. Здесь JS-рантайм уже существует, поэтому
  нарушения диспатчатся сразу, без отложенной схемы.
- Три места, читавшие `extract_style_blocks` без политики документа
  (`frames.rs::fetch_frame_subresources` — каскад подфрейма, та же граница,
  что срез 7 уже документирует для внешнего `<link>` ребёнка;
  `lumen/docking.rs::open_sidebar_page` — sidebar-панель, отдельный
  navigable без концепции CSP; `lumen/hibernation.rs` — снимок/восстановление
  T3-гибернации, политика документа не сохраняется в `HibernatedTab`),
  обновлены на новую сигнатуру с явным `None` и комментарием о причине.

Подтверждено живым окном (`--screenshot`, три арма одной пробы): страница с
`<meta ... content="style-src 'none'">` и `<style>#probe{color:red}</style>`
даёт `getComputedStyle(#probe).color === rgb(0, 0, 0)` (стиль не применился)
и `securitypolicyviolation` с `directive=style-src`; baseline той же
страницы без директивы даёт `rgb(255, 0, 0)`; арм с `style-src
'nonce-abc123'` и `<style nonce="abc123">` даёт `rgb(0, 128, 0)` — nonce
пропускает совпавший блок. `cargo test -p lumen-shell --features v8 --bin
lumen` без регрессий (1868 passed), `cargo clippy -p lumen-shell
--all-targets --features v8 -- -D warnings` чисто.

Не покрыто этим срезом: атрибут `style=` (не блокируется, только тело
`<style>`); инлайновый `<style>` внутри `<iframe>`/sidebar/восстановленной
после гибернации вкладки (см. выше); директивы кроме
`script-src`/`img-src`/`style-src`/`connect-src`/`worker-src`/`frame-src`/
`object-src`/`media-src`/`font-src`; `frame-ancestors`; `report-to`;
честная независимая проверка заголовка и `<meta>`.

## Срез 22 (2026-09-19, `p6-gap-cspenf-srez22`) — `style-src` против инлайновых `<style>` внутри `<iframe>`

Реализовано: ровно тот пробел, что срез 21 назвал не покрытым — инлайновый
`<style>` внутри `<iframe>` не гейтился вовсе, хотя политика ребёнка уже
считалась в том же месте кода для `img-src`/внешнего `<link>` (срез 8).

- `crates/shell/src/frames.rs::fetch_frame_subresources`: `csp_gate`
  (`document_csp_policy` ребёнка) была вычислена ПОСЛЕ вызова
  `extract_style_blocks(doc, None)` — политика для картинок уже существовала
  в теле функции, просто не была доступна раньше по порядку кода. Срез 22
  переставил вычисление `csp_gate` перед этим вызовом и передал его тем же
  `inline_style_blocked`-гейтом, что срез 21 уже даёт top-level документу
  (`csp_gate.as_ref().map(|(p, _)| p)`) — заблокированный `<style>`-узел
  ребёнка не попадает в текст, который парсит каскад фрейма, вовсе, тот же
  принцип «не применённый CSS».
- `FrameSubresourceOutcomes` получила `blocked_inline_style_count: usize` —
  `extract_style_blocks` уже возвращает это число (срез 21), оно просто
  отбрасывалось здесь (`let (inline, _blocked) = …`).
- `crates/shell/src/frames.rs::spawn_frame`: диспатч `securitypolicyviolation`
  после появления JS-рантайма ребёнка расширен на `blocked_inline_style_count`
  — `blockedURI = "inline"` для каждого заблокированного узла, тот же
  one-shot-push путь, что уже несёт `blocked_by_img_src`/`blocked_by_style_src`
  для этого ребёнка (срез 8).

Тесты: +2 в `crates/shell/src/tests/page_resources.rs`
(`frame_subresources_reports_csp_blocked_inline_style` — `style-src 'none'`
блокирует и счётчик, и текст каскада; `frame_subresources_no_policy_keeps_inline_style`
— без политики гейт не срабатывает). Живой проб не делался — то же обоснование,
что срез 18 уже принял для тонкой match-веточной проводки, переиспользующей
уже протестированный примитив (`inline_style_blocked`, срез 21) в новой точке
вызова. Подтверждено `cargo clippy -p lumen-shell --all-targets --features
v8 -- -D warnings` (чисто) и `cargo test -p lumen-shell --features v8 --bin
lumen` (1870 passed, 0 failed, включая 2 новых).

Не покрыто этим срезом: атрибут `style=` (ни top-level, ни внутри `<iframe>`);
инлайновый `<style>` внутри sidebar/восстановленной после гибернации вкладки
(`docking.rs`/`hibernation.rs` по-прежнему зовут `extract_style_blocks(&doc,
None)` — ни один navigable там не несёт CSP-политику); директивы кроме
`script-src`/`img-src`/`style-src`/`connect-src`/`worker-src`/`frame-src`/
`object-src`/`media-src`/`font-src`; `frame-ancestors`; `report-to`; честная
независимая проверка заголовка и `<meta>`.

## Срез 23 (2026-09-19, `p6-gap-cspenf-srez23`) — `style-src-attr` против атрибута `style=`

Реализовано: последний класс инлайна, названный не покрытым срезами 21/22, —
атрибут `style=""` на произвольном элементе. Архитектурно другое решение, чем
срезы 21/22 (гейт текста `<style>` в `doc_extract::walk_style_blocks`): точка
потребления атрибута — `lumen_layout::style::cascade` (`crates/engine/layout`),
единственный choke point для парсер-, скрипт- и CSSOM-вставленного значения
(`element.style.setProperty`/`.cssText` и обычный `setAttribute('style', …)`
сходятся в один и тот же DOM-атрибут, который cascade.rs читает напрямую), но
`layout` не зависит от `lumen-network`/`CspPolicy` (layering `dom → layout`,
не `network → layout`) — решение не может считаться там же, где считается
политика.

- `crates/shell/src/csp_enforce.rs`: `style_attribute_blocked(policy, body)` —
  тот же дух, что [`inline_style_blocked`] даёт `<style>`-тексту, но с другим
  набором источников (CSP3 §6.4.2/§8.1): нет `nonce` (у атрибута нет своего
  `nonce=`, в отличие от `<style nonce="…">`), а хэш-источник допускает
  совпадение только вместе с `'unsafe-hashes'` — голого хэша достаточно для
  `<style>` элемента (срез 20/21), но никогда для атрибута. Фолбэк на один
  уровень глубже остальных директив этого файла: `style-src-attr` →
  `style-src` → `default-src` (CSP3 §6.4 granular chain, до этого среза
  `CspDirective::StyleSrcAttr` был распарсен, но нигде не участвовал в этом
  фолбэке). +7 unit-тестов.
- `crates/engine/dom/src/lib.rs`: `Document` получила
  `style_attr_csp_blocked: HashSet<NodeId>` и пару методов
  (`is_style_attr_csp_blocked`/`set_style_attr_csp_blocked`) — данные без
  какого-либо CSP-типа, тот же паттерн, что уже даёт `csp_header:
  Option<String>` (срез 5): `dom`/`layout` не узнают о CSP ничего сверх
  голого набора id, все узнают только через shell, который заполняет набор.
- `crates/shell/src/doc_extract.rs::collect_style_attr_csp_blocked` — обход
  всего дерева (не только `<style>`-узлов, в отличие от `walk_style_blocks`),
  каждый непустой атрибут `style` проверяется независимо; +3 unit-теста.
- `crates/engine/layout/src/style/cascade.rs`: перед `parse_inline_style`
  проверяет `doc.is_style_attr_csp_blocked(node)` — заблокированный атрибут
  не попадает в парсер вовсе (тот же принцип «не применённый CSS», что уже
  даёт заблокированный `<link>`/`<style>`), но остаётся в DOM нетронутым —
  `getAttribute('style')` продолжает возвращать исходный текст, меняется
  только эффект на каскад.
- `crates/shell/src/page_pipeline.rs::build_page_cascade` считает набор той
  же одноразовой политикой, что уже даёт `extract_style_blocks`
  (`PageCascade::blocked_style_attr_nodes`); набор передаётся в `Document` в
  обеих точках, где документ ещё изменяем до layout — сразу после начальной
  сборки каскада и после пересборки при `scripts_changed_css` (та же ветка,
  что уже пересчитывает `blocked_inline_style_count`). `securitypolicyviolation`
  диспатчится по одному на узел с `violatedDirective=style-src-attr`,
  `blockedURI=inline` — той же one-shot-push схемой, что и инлайновый
  `<style>` (срез 21), но отдельной директивой в отчёте (CSP3 §6.4 granular
  effective directive, не `style-src`).

Подтверждено живым окном (`--screenshot`, три арма): страница с `<meta
... content="style-src-attr 'none'">` и `<div style="color:red">` даёт
`getComputedStyle(#probe).color === rgb(0, 0, 0)` (атрибут не применился) и
`securitypolicyviolation` с `directive=style-src-attr`; baseline той же
страницы без директивы даёт `rgb(255, 0, 0)`. `cargo test -p lumen-shell
--features v8 --bin lumen` без регрессий (1881 passed), `cargo test -p
lumen-dom` (303 passed), `cargo test -p lumen-layout` (77 passed), `cargo
clippy -p lumen-dom -p lumen-layout -p lumen-network -p lumen-shell
--all-targets --features v8 -- -D warnings` чисто.

Покрывает только элементы дерева на момент вычисления каскада (начальный
парсинг + пересборка при `scripts_changed_css`) — узел, получивший `style=""`
другим путём после этого момента (простой `setAttribute`/`style.cssText`, не
трогающий `<style>`/`<link>` и потому не запускающий пересборку каскада),
гейт не видит.

Не покрыто этим срезом: атрибут `style=` внутри `<iframe>`; точечная
DOM-мутация после первого layout (см. выше); директивы кроме
`script-src`/`img-src`/`style-src`/`style-src-attr`/`connect-src`/`worker-src`/
`frame-src`/`object-src`/`media-src`/`font-src`; `frame-ancestors`;
`report-to`; честная независимая проверка заголовка и `<meta>`.

## Срез 24 (2026-09-19, `p6-gap-cspenf-srez24`) — `style-src-attr` против атрибута `style=` внутри `<iframe>`

Реализовано: ровно тот пробел, что срез 23 назвал не покрытым — атрибут
`style=""` внутри `<iframe>` не гейтился вовсе, хотя политика ребёнка уже
считалась в том же месте кода для `img-src`/внешнего `<link>`/инлайнового
`<style>` (срезы 8/22).

- `crates/shell/src/frames.rs::fetch_frame_subresources`: сразу после гейта
  инлайнового `<style>` (срез 22) добавлен `collect_style_attr_csp_blocked`
  (срез 23) по тому же `csp_gate` — набор заблокированных узлов пишется прямо
  на `doc` (`Document::set_style_attr_csp_blocked`) ДО того, как документ
  передаётся в `run_scripts_with_dom`/`layout_frame_document`: `lumen_layout`'s
  cascade читает этот набор с документа так же, как у top-level документа
  (`page_pipeline.rs::build_page_cascade`), без отдельного провода через
  layout ребёнка.
- `FrameSubresourceOutcomes` получила `blocked_style_attr_count: usize` — тот
  же счётчик-без-URL, что уже есть у `blocked_inline_style_count` (срез 22):
  `blockedURI` для атрибута тоже всегда `"inline"`.
- `crates/shell/src/frames.rs::spawn_frame`: диспатч `securitypolicyviolation`
  после появления JS-рантайма ребёнка расширен на `blocked_style_attr_count`
  с `violatedDirective=style-src-attr` — тот же one-shot-push путь, что уже
  несёт `blocked_by_img_src`/`blocked_by_style_src`/`blocked_inline_style_count`
  для этого ребёнка (срезы 8/22).

Тесты: +2 в `crates/shell/src/tests/page_resources.rs`
(`frame_subresources_reports_csp_blocked_style_attr` — `style-src-attr 'none'`
блокирует счётчик; `frame_subresources_no_policy_keeps_style_attr` — без
политики гейт не срабатывает). Живым окном (`--screenshot`, три арма):
`<iframe>` с `<meta ... content="style-src-attr 'none'">` и `<div
style="color:red">` внутри даёт `getComputedStyle(#probe).color === rgb(0, 0,
0)` (атрибут не применился) и `securitypolicyviolation` с
`directive=style-src-attr policy=style-src-attr 'none'` внутри фрейма;
baseline той же страницы без директивы даёт `rgb(255, 0, 0)`.

Подтверждено `cargo build -p lumen-shell --features v8` + `cargo clippy -p
lumen-shell -p lumen-dom -p lumen-layout -p lumen-network --all-targets
--features v8 -- -D warnings` (чисто) + `cargo test -p lumen-shell --features
v8 --bin lumen` (1885 passed, 0 failed, включая 2 новых).

Не покрыто этим срезом: точечная DOM-мутация после первого layout (см. срез
23); директивы кроме `script-src`/`img-src`/`style-src`/`style-src-attr`/
`connect-src`/`worker-src`/`frame-src`/`object-src`/`media-src`/`font-src`;
`frame-ancestors`; `report-to`; честная независимая проверка заголовка и
`<meta>`; вложенные фреймы фрейма (рекурсия та же функция, не тестировалась
отдельно для этого гейта, как и остальные срезы 8/22 её не тестировали).

## Срез 25 (2026-09-19, `p6-gap-cspenf-srez25`) — `font-src`/`img-src` против `@font-face url()`/`background-image` внутри `<iframe>`

Реализовано: ровно два пробела, что срезы 18/19 назвали не покрытыми — фон
(`background-image: url()`) и веб-шрифты (`@font-face url()`) ребёнка
`<iframe>` фетчились совсем без проверки CSP-политики, хотя та же проверка
для его собственных `<img src>`/`<link rel=stylesheet>`/инлайновых
`<style>` уже действовала с срезов 8/22.

- `crates/shell/src/frames.rs::load_frame_fonts`: новые параметры `csp_gate`/
  `self_origin` — каждый `url()`-источник `@font-face`, прежде чем звать
  `fetch_font_bytes`, проверяется тем же [`font_src_blocked`], что срез 19
  уже даёт top-level документу; `local()`-источники не затронуты (CSP гейтит
  сетевой фетч, а не системный поиск шрифта, который уже отработал выше в
  `load_font_faces`). Возвращает третий элемент кортежа —
  `blocked_by_font_src: Vec<String>` (резолвленные URL) для последующего
  `securitypolicyviolation`.
- `crates/shell/src/frames.rs::fetch_frame_background_images`: та же пара
  параметров, каждый URL проверяется [`img_src_blocked`] (уже даёт top-level
  документу срез 18) до фетча; замыкание `parallel_map` возвращает
  `(Option<(raw, key, image)>, Option<blocked_url>)` вместо голого `Option`,
  чтобы отличить «не найдено» от «заблокировано». Третий возвращаемый вектор
  — `blocked_by_img_src`.
- `crates/shell/src/frames.rs::spawn_frame`: политика ребёнка считается один
  раз (`child_csp_gate`, тем же `document_csp_policy(&child_doc_arc, ...)`,
  что уже даёт `fetch_frame_subresources`) перед обоими вызовами вместо
  повторного разбора внутри каждого; диспетчер `securitypolicyviolation`
  (уже несущий `img-src`/`style-src`/инлайн/атрибут с срезов 8/22/24)
  расширен `font-src` и вторым `img-src` источником (фон), переиспользуя
  тот же `child_csp_gate` вместо повторного лока `child_doc_arc`.

Тесты: +4 в `crates/shell/src/tests/page_resources.rs` —
`frame_fonts_reports_csp_blocked_font_src`/`frame_fonts_no_policy_does_not_report_blocked`
(прямой вызов `load_frame_fonts`, без сети — политика проверяется до
`fetch_font_bytes`) и
`frame_background_images_reports_csp_blocked_img_src`/`frame_background_images_no_policy_does_not_report_blocked`
(реальный layout через `lumen_layout::layout_measured` с
`background-image: url(...)`, прямой вызов `fetch_frame_background_images`).
`cargo test -p lumen-shell --features v8 --bin lumen` без регрессий,
`cargo clippy -p lumen-shell --all-targets --features v8 -- -D warnings`
чисто.

Не покрыто этим срезом: остальные директивы (`manifest-src`/`child-src`/…);
`frame-ancestors` (распознаётся парсером, но нигде не проверяется — сама
проверка ancestor-цепочки происхождений архитектурно другая форма, чем все
fetch-гейты этого файла); `report-to`; честная независимая проверка
заголовка и `<meta>`; атрибут `style=` внутри `<iframe>` после точечной
DOM-мутации; вложенные фреймы фрейма (та же функция вызывается рекурсивно,
не тестировалась отдельно для этого гейта).

## Срез 26 (2026-09-19, `p6-gap-cspenf-srez26`) — директива `child-src` + CSP3 §6.4 granular-фолбэк для `frame-src`/`worker-src`

Закрыт ровно тот пробел, что срез 25 назвал не покрытым первым пунктом
списка: `child-src` не была распознана парсером вовсе (`crates/network/src/
csp.rs` — токен `child-src` падал в безымянный `_ => continue`, как любая
неизвестная директива), а `frame-src`/`worker-src` фолбэк на `default-src`
проходили одношаговым `effective_sources`, минуя промежуточный `child-src`,
который CSP3 §6.4 требует проверить первым для обеих директив.

- `crates/network/src/csp.rs`: новый вариант `CspDirective::ChildSrc`,
  строка `"child-src"` теперь маппится на него в парсере. Новый метод
  `CspPolicy::fetch_directive_allows_via_child_src` — тот же принцип
  granular-фолбэка, что `style_attribute_blocked` (срез 23) уже даёт
  `style-src-attr` (`directive` → `style-src` → `default-src`), только
  публичный метод самого `CspPolicy`, а не приватная функция шелла: `frame-
  src`/`worker-src` живут в двух разных крейтах (`csp_enforce.rs` в shell,
  `HttpClient` в network), обоим нужен один и тот же трёхшаговый фолбэк.
  Приватный `effective_sources_via_child_src` под капотом — `directive` →
  `child-src` → `default-src`.
- `crates/shell/src/csp_enforce.rs::frame_src_blocked`: вызывает новый метод
  вместо `fetch_directive_allows`.
- `crates/network/src/lib.rs::worker_src_gate` (backing `check_worker_src`,
  срез 13): тот же переход на новый метод.

Тесты: +4 в `crates/network/src/csp.rs` (парсинг `child-src`, фолбэк-цепочка
для `frame-src` — приоритет собственной директивы над `child-src`, `child-
src` над `default-src`, отсутствие всех трёх директив не блокирует), +1 в
`crates/network/src/lib.rs` (`worker_src_falls_back_to_child_src_before_default_src`),
+1 в `crates/shell/src/csp_enforce.rs`
(`frame_src_falls_back_to_child_src_before_default_src`).
`cargo test -p lumen-network csp::`/`worker_src` и
`cargo test -p lumen-shell --features v8 --bin lumen frame_src` без
регрессий; `cargo clippy -p lumen-network --all-targets -- -D warnings` и
`cargo clippy -p lumen-shell --all-targets --features v8 -- -D warnings`
чисто.

Не покрыто этим срезом: остальные директивы (`manifest-src`/…) — `manifest-
src` не гейтится, потому что `<link rel=manifest>` вообще не обрабатывается
движком (Web App Manifest не реализован — не CSP-дефект); `frame-ancestors`;
`report-to`; честная независимая проверка заголовка и `<meta>`; атрибут
`style=` внутри `<iframe>` после точечной DOM-мутации; вложенные фреймы
фрейма; `importScripts()` внутри уже запущенного воркера.

## Срез 27 (2026-09-19, `p6-gap-cspenf-srez27`) — директива `frame-ancestors`

Закрыт ровно тот пробел, что срез 26 назвал не покрытым: `CspDirective::
FrameAncestors` уже разбиралась парсером (`crates/network/src/csp.rs`), но
ни одна точка кода её не проверяла — политика ребёнка `<iframe>` могла
объявить `frame-ancestors 'none'` и всё равно быть встроена куда угодно.

- `crates/network/src/csp.rs`: новый метод `CspPolicy::frame_ancestor_allowed`
  — принимает origin встраивающего документа (`ancestor_origin`) и origin
  защищаемого документа (`self_origin`, тот же смысл `'self'`, что уже несут
  все fetch-директивы этого файла). В отличие от каждого fetch-гейта выше,
  **без фолбэка на `default-src`** — CSP3 §6.4 явно исключает навигационные
  директивы (`frame-ancestors`, `sandbox`) из наследования `default-src`;
  отсутствие директивы значит «не ограничено», как и у остальных гейтов.
- `crates/shell/src/csp_enforce.rs::frame_ancestors_blocked` — тонкая обёртка
  той же формы (`policy`/`self_origin` → `bool`), что и `frame_src_blocked`/
  `font_src_blocked` выше в файле, но читает политику ЗАЩИЩАЕМОГО документа
  (ребёнка), а не встраивающего — единственная директива в этом модуле с
  такой инверсией направления.
- `crates/shell/src/frames.rs::fetch_frame_subresources` — новый параметр
  `ancestor_origin`; проверка идёт СРАЗУ после вычисления `csp_gate` ребёнка,
  до единого сетевого похода за `<img>`/`<link>`/инлайновым `<style>` —
  заблокированный `frame-ancestors` не должен тратить сеть ни на что из
  содержимого, которое всё равно не покажется (та же экономия, что срезы
  8/18/19/25 уже дают отдельным `img-src`/`font-src` блокировкам, только
  здесь она отменяет ВСЁ содержимое разом). Новое поле
  `FrameSubresourceOutcomes::frame_ancestors_blocked: bool` — структура
  получила `#[derive(Default)]`, чтобы ранний `return` с одним выставленным
  полем не пришлось писать все 12 полей вручную.
- `crates/shell/src/frames.rs::spawn_frame` — `ancestor_origin` передаётся
  как уже посчитанный `self_origin` этой функции (origin РОДИТЕЛЯ,
  `base.origin()` — тот же биндинг, что `frame_src_check` уже использует
  чуть выше как `self_origin` для проверки политики РОДИТЕЛЯ против
  навигации; здесь то же значение читается как «origin встраивающего», а
  политика — ребёнка). При блокировке `child_doc` целиком заменяется тем же
  синтетическим `frame_error_document`, что уже даёт ветка `Some(Err(e))`
  сетевой неудачи — скрипты/раскладка/дальнейшие подресурсы ребёнка не
  запускаются вовсе (эквивалент «резюме загрузки» настоящего браузера при
  `X-Frame-Options`/`frame-ancestors`-отказе).

Не в этом срезе (по спеке `frame-ancestors` — не fetch-исход, а отказ
рендерить документ целиком, поэтому у заблокированного документа никогда не
появляется JS-контекст): доставка `report-uri`/`securitypolicyviolation` для
этого конкретного нарушения (нечему диспатчить событие — тот же класс
решения, что уже принят для report-to во всём модуле); проверка ancestor
ЦЕПОЧКИ целиком (top + все промежуточные предки) — проверяется только
непосредственный родитель, чего достаточно для однократной вложенности,
типичной для WPT-тестов этой категории, но не для `iframe` внутри `iframe`
с разными origin на каждом уровне; заголовок ответа `X-Frame-Options`
(отдельный, более старый механизм с похожей целью — не разбирается вовсе).

Тесты: +4 в `crates/network/src/csp.rs` (`frame_ancestor_allowed` — host
allow/deny, `'none'`, `'self'` относительно origin защищаемого документа,
отсутствие директивы не наследует `default-src`), +3 в
`crates/shell/src/csp_enforce.rs` (обёртка `frame_ancestors_blocked`), +2 в
`crates/shell/src/tests/page_resources.rs` (`fetch_frame_subresources`
блокирует и не трогает сеть при несовпадающем embedder-origin; пропускает
при совпадающем). `cargo test -p lumen-network --lib` (2264 passed) и
`cargo test -p lumen-shell --features v8 --bin lumen` (1900 passed, 0
failed) без регрессий; `cargo clippy -p lumen-network -p lumen-shell
--all-targets --features v8 -- -D warnings` чисто.

## Срез 28 (2026-09-19, `p6-gap-cspenf-srez28`) — `worker-src` против `importScripts()` внутри уже запущенного воркера

Закрыт ровно тот пробел, что срез 13 сам назвал непокрытым в своей записи:
`worker-src` гейтил только начальный классический скрипт конструктора
(`_lumen_worker_fetch_script`/`_lumen_sw_fetch_script`), а последующие вызовы
`importScripts(url)` внутри уже запущенного `Worker`/`SharedWorker` уходили
прямиком в `fetch_worker_script` — тот же `fetch_sync` без единой проверки
политики, хотя CSP3 §6.4 явно говорит: `worker-src` управляет «worker'а
скриптом и его импортированными скриптами» как одним целым.

- `crates/js/src/worker.rs`: новая `import_scripts_csp_blocked(provider, url)`
  — `true`, если `url` не `data:`/`blob:lumen/` (эти никогда не идут в сеть в
  `resolve_import_url`, поэтому им нечего гейтить) и `provider.check_worker_src(url)`
  отказывает. Та же функция `check_worker_src` (`crates/core/src/ext.rs`,
  срез 13), что уже гейтит конструктор — здесь просто вызывается из второй
  точки входа.
- Обе регистрации `_lumen_import_scripts_resolve` — dedicated worker
  (`worker.rs::install_worker_globals_v8`) и `SharedWorker`
  (`shared_worker.rs`) — зовут `import_scripts_csp_blocked` до
  `resolve_import_url`/`fetch_worker_script`; заблокированный URL не
  фетчится вовсе, тот же принцип «ни одного исходящего байта», что каждый
  предыдущий срез этого GAP уже применяет на своей точке. `ServiceWorker`'s
  `importScripts` (`sw_worker.rs`) не тронут — его конструирование само
  никогда не было в объёме среза 13 (только `new Worker`/`new SharedWorker`),
  так что это отдельный, более широкий пробел, не сужаемый этим срезом.
- Никакого `securitypolicyviolation` для этого случая: в отличие от
  конструктора (чей гейт стоит на **родительском** JS-рантайме, где
  `document`/CSP-шим уже есть), `_lumen_import_scripts_resolve` исполняется
  на рантайме самого воркера — там нет ни `document`, ни установленного
  `SecurityPolicyViolationEvent`. Заблокированный вызов выглядит для скрипта
  как обычная сетевая неудача (`Error: importScripts: cannot load script: …`,
  тот же путь, что уже даёт истёкший/недоступный URL) — само блокирование
  происходит независимо от того, доставлено ли событие.

Тесты: +3 в `crates/js/src/worker.rs::tests`
(`v8_import_scripts_blocked_by_worker_src_never_reaches_fetch` — сквозной
прогон через реальный `importScripts()` с провайдером, чей `fetch_sync`
вернул бы исполняемое тело, если бы был достигнут;
`import_scripts_csp_blocked_skips_data_and_blob_urls`;
`import_scripts_csp_blocked_without_provider_never_blocks`).
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
warnings` (чисто), `cargo test -p lumen-js --features v8-backend --lib
worker::tests` и `shared_worker::tests` (без регрессий).

## Срез 29 (2026-09-19, `p6-gap-cspenf-srez29`) — директива `form-action`

Ещё одна навигационная директива CSP3 (§6.4.3), до этого среза только
парсившаяся (`CspDirective::FormAction` существовал в
`crates/network/src/csp.rs`, но ничего его не читало) — как и
`frame-ancestors` до среза 27, отправка формы не проверялась вовсе.

- `crates/network/src/csp.rs`: новый `CspPolicy::form_action_allowed`
  (`action_url`, `self_origin`) — та же форма, что
  `frame_ancestor_allowed` (срез 27): **без фолбэка на `default-src`**, CSP3
  §6.4 явно исключает навигационные директивы из наследования; отсутствие
  директивы значит «не ограничено».
- `crates/shell/src/csp_enforce.rs::form_action_blocked` — тонкая обёртка
  той же формы, что и `frame_src_blocked`/`font_src_blocked` выше в файле.
- `crates/shell/src/lumen/form_submit.rs::run_form_submission` — `csp_gate`
  документа считывается в том же коротком заимствовании, что уже даёт
  `submit_event`/`enctype`/`dialog_node` (тот же паттерн, что и
  `document_csp_policy` в `frames.rs`). Новый метод
  `form_action_navigation_blocked` вызывается ПОСЛЕ резолва адреса и ДО
  `navigate_to` в обеих ветках, у которых вообще есть навигация (`get` и
  POST-«submit as entity body»); `method="dialog"` не гейтится — CSP3
  §6.4.3 ограничивает цель НАВИГАЦИИ, а `dialog` не навигирует никуда, она
  закрывает `<dialog>`.
- `crates/shell/src/lumen/frame_form_submit.rs::frame_submit_navigate` —
  зеркало для формы под-документа `<iframe>`: гейтится политикой РЕБЁНКА
  (форма его собственная) и его собственным origin (`nav_base.origin()`),
  до диспетчеризации по `LinkTarget` (`Page`/`Frame`/`NewWindow`) — ни один
  из трёх реальных исходов навигации не должен случиться, если действие
  заблокировано. `securitypolicyviolation` шлётся через `h.js` напрямую
  (контекст РЕБЁНКА, как `fire_dialog_close` чуть выше в этом файле), а не
  через `route_task_js` (тот адресует только рантайм страницы).
- POST-путь `<iframe>`-формы не тронут: `run_frame_form_submission` уже не
  отправляет POST по сети вовсе (только `eprintln`, см. комментарий на
  месте) — гейтить нечего, пока сама отправка не написана.

Тесты: +4 в `crates/network/src/csp.rs` (`form_action_allowed` — host
allow/deny, `'none'`, `'self'` относительно origin документа-владельца
формы, отсутствие директивы не наследует `default-src`), +4 в
`crates/shell/src/csp_enforce.rs` (обёртка `form_action_blocked`, те же
четыре случая). `cargo test -p lumen-network --lib csp` (40 passed) и
`cargo test -p lumen-shell --features v8 --bin lumen` (1906 passed, 0
failed) без регрессий; `cargo clippy -p lumen-network -p lumen-shell
--all-targets --features v8 -- -D warnings` чисто.

## Срез 30 (2026-09-19, `p6-gap-cspenf-srez30`) — `worker-src` против `serviceWorker.register()`

Закрыт пробел, оставшийся незамеченным срезами 13/28 (оба покрыли
`new Worker()`/`new SharedWorker()`/`importScripts()`, но не третий
конструктор воркерной области): `navigator.serviceWorker.register(url)`
(`crates/js/src/shim/web_api_shim_mid_b.js::_sw_run_lifecycle`) забирает
текст регистрационного скрипта обычным страничным `fetch(scriptURL)` — тем
самым `fetch()`, который срез 10 уже гейтит по `connect-src`, но CSP3 §6.4
явно относит SW-регистрацию к `worker-src` (с фолбэком на `default-src`), не
к `connect-src`. Политика `worker-src 'none'` без отдельного ограничения
`connect-src` регистрировала любой воркер беспрепятственно —
`grep -rn WorkerSrc crates/js/src/shim` до этого среза не находил ничего в
пути регистрации.

- `crates/js/src/v8_runtime/install/net.rs::install_service_worker` — новый
  натив `_lumen_sw_check_worker_src(url) -> [] | [blockedUri, originalPolicy]`,
  тот же I/O-free `check_worker_src` (`crates/core/src/ext.rs`), что срезы
  13/28 уже используют для конструктора воркера/`importScripts()`, здесь
  вызванный из установщика Service Worker бindings (`fp_sw_net` — тот же
  провайдер, что страница передаёт в `fetch()`/`XMLHttpRequest`/WebSocket/
  SSE/`sendBeacon`). В отличие от однослотового side-channel паттерна
  (`_lumen_worker_last_csp_block` и т.п.), здесь достаточно одного
  синхронного вызова с прямым возвратом массива — `register()` зовёт его
  один раз за вызов, конкурентных гонок между чтением и следующим вызовом
  нет.
- `crates/js/src/shim/web_api_shim_mid_b.js::register` — проверка
  синхронно ПЕРВЫМ шагом, до создания `_sw_make_registration`/записи в
  `_sw_registrations`/`_lumen_sw_register` — заблокированный вызов не
  оставляет никакого состояния регистрации (SW spec §register() шаг о CSP
  идёт раньше запуска job'а установки). При блокировке: диспатч
  `securitypolicyviolation` через уже существующий `_lumen_dispatch_csp_violation`
  (`'worker-src'`, `blockedUri`, `originalPolicy`, `'enforce'`) и
  `Promise.reject(new DOMException(…, 'SecurityError'))` — тот же тип
  исключения, что спека требует для CSP-отказа `register()`.
- Не тронуто: сама доставка `fetch(scriptURL)` внутри `_sw_run_lifecycle`
  (срабатывает уже ПОСЛЕ прохождения этой проверки, на активации) — её
  `connect-src`-гейт (срез 10) остаётся дополнительной, а не единственной
  преградой, ровно как для скриптов/картинок обе директивы могут действовать
  одновременно; `ServiceWorker`'s собственный `importScripts()`
  (`sw_worker.rs`) по-прежнему не гейтится — тот же остаток, что срез 28 сам
  назвал не сузившимся этим классом решений.

Тесты: +4 в `crates/js/src/dom/tests/v8_events_cache.rs`
(`sw_register_worker_src_block_rejects_promise` — `SecurityError`;
`sw_register_worker_src_block_never_registers` — `_lumen_sw_has_registration`
остаётся `false`; `sw_register_worker_src_block_fires_security_policy_violation_event`;
`sw_register_allowed_when_no_worker_src_policy` — тот же "no policy means no
block" инвариант, что все предыдущие срезы этого GAP проверяют). Плюс
попутный фикс дрейфа: `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` (запущенный до этого среза, для чистоты
собственного гейта) уже был красным на `main` — `worker.rs`'s тестовый мок
`CspBlockedImportNet::fetch_sync` не собирал `JsFetchResult` после того, как
BUG-984 добавил туда поле `url` (мок написан срезом 28 раньше слияния
BUG-984, ни один из них не задевал файл другого); однострочный фикс
(`url: _url.to_string()`) в этом же коммите, не отдельным.

`cargo test -p lumen-js --features v8-backend --lib` (3906 passed, 0 failed)
и `cargo test -p lumen-shell --features v8 --bin lumen` (1916 passed, 0
failed) без регрессий; `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` и `cargo clippy -p lumen-shell --all-targets
--features v8 -- -D warnings` чисто.

## Срез 31 (2026-09-19, `p6-gap-cspenf-srez31`) — `worker-src` против `importScripts()` внутри самого Service Worker

Закрыт остаток, который срез 30 сам назвал не сузившимся: срез 28 гейтил
`importScripts()` для `Worker`/`SharedWorker` (`crates/js/src/worker.rs`,
`shared_worker.rs`), но конструирование самого Service Worker никогда не
входило в объём среза 13 — поэтому у его собственного `importScripts`
(`crates/js/src/sw_worker.rs`, отдельный рантайм, не связанный с
`worker.rs`) не было вообще никакой проверки: чистый JS-шим напрямую звал
`_lumen_sw_net_fetch` без предварительного гейта.

- `crates/js/src/sw_worker.rs::install_sw_globals_v8` — новый натив
  `_lumen_sw_check_worker_src(url) -> bool`, переиспользующий
  `worker::import_scripts_csp_blocked` (срез 28) — тот же I/O-free
  precheck с тем же пропуском `data:`/`blob:lumen/` целей, а не
  повторная реализация. Рантайм SW получает `fetch_provider` тем же
  `Arc<dyn JsFetchProvider>`, что уже используется для
  `_lumen_sw_net_fetch`/`fetch_bypassing_sw`.
- Шим `globalThis.importScripts` (в `sw_globals_shim`) зовёт
  `_lumen_sw_check_worker_src(abs)` первым шагом внутри цикла по
  аргументам, до `_lumen_sw_net_fetch` — заблокированный URL не уходит в
  сеть вовсе, бросает тот же `Error('importScripts: cannot load script: …')`,
  что уже используется для обычного сетевого отказа (тот же паттерн, что
  срез 28 применил для `Worker`/`SharedWorker`: в рантайме воркера нет
  `document`/CSP-шима, поэтому `securitypolicyviolation` здесь не
  диспатчится — блокировка видна скрипту как обычная сетевая неудача).

Тесты: +2 в `crates/js/src/sw_worker.rs::tests_v8`
(`sw_import_scripts_blocked_by_worker_src_never_reaches_fetch` — мок
`CspBlockedSwNet` отказывает безусловно, тело блокированного скрипта
никогда не исполняется; `sw_import_scripts_allowed_when_no_worker_src_policy`
— тот же «нет политики значит нет блока» инвариант, что все предыдущие
срезы этого GAP проверяют). `cargo test -p lumen-js --features v8-backend
--lib` (3908 passed, 0 failed) без регрессий; `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` чисто.

## Срез 32 (2026-09-19, `p6-gap-cspenf-srez32`) — директива `base-uri` против `<base href>`

Единственная директива из `CspDirective`, у которой был парсинг
(`CspDirective::BaseUri`, `crates/network/src/csp.rs`), но вообще не было
проверяющей функции — `grep -rn "BaseUri\|base_uri" crates/network/src
crates/shell/src` до этого среза не находил ни одного enforcement-сайта.
Единственная точка потребления `<base href>` — `crates/shell/src/
page_pipeline.rs::effective_base` — резолвила её без всякой проверки.

- `crates/network/src/csp.rs`: новый `CspPolicy::base_uri_allowed`
  (`base_url`, `self_origin`) — та же форма, что `frame_ancestor_allowed`
  (срез 27)/`form_action_allowed` (срез 29): **без фолбэка на
  `default-src`**, CSP3 §6.4 относит `base-uri` к navigation directives
  наравне с ними; отсутствие директивы значит «не ограничено».
- `crates/shell/src/csp_enforce.rs::base_uri_blocked` — тонкая обёртка той
  же формы, что `form_action_blocked`.
- `crates/shell/src/page_pipeline.rs::effective_base` — новый приватный
  `base_uri_href_blocked(doc, base, href)` вызывается ПЕРЕД
  `base.resolve_as_base(href)`; заблокированный href отбрасывается целиком
  (базой остаётся исходный URL документа), не просто игнорируется одно
  обращение — HTML LS §4.2.3 шаг 6 уже отбрасывает `<base>`, чей href не
  парсится, CSP3 §6.4.1 добавляет вторую, политика-зависимую причину. Важно:
  `self_origin` для проверки берётся из НЕ-адаптированного `base`
  (параметра функции), а не из уже применённого `<base>` — иначе `'self'`
  вырождался бы в «всегда true» для любого href, который сам себе
  назначает базу.
- `effective_base` — единственная точка потребления, но вызывается на
  каждый резолв URL (10+ сайтов вызова, намеренно пересчитывается заново,
  не кэшируется — BUG-752), поэтому диспатч `securitypolicyviolation`
  оттуда означал бы событие на каждый резолв. Вместо этого — одноразовый
  репорт в `parse_and_layout`, тот же «one-shot-push» паттерн, что срезы
  7/21/23 уже применяют для choke point'ов без `js_ctx`: `<base href>`
  документа (после исполнения скриптов — те могут вставить/сменить
  `<base>`) перепроверяется один раз тем же `base_uri_href_blocked`, и при
  блокировке шлётся ровно одно событие `securitypolicyviolation`
  (`violatedDirective="base-uri"`).

Тесты: +4 в `crates/network/src/csp.rs` (`base_uri_allowed` — host
allow/deny, `'none'`, `'self'` относительно origin документа, отсутствие
директивы не наследует `default-src`), +4 в `crates/shell/src/
csp_enforce.rs` (обёртка `base_uri_blocked`, те же четыре случая), +3 в
`crates/shell/src/tests/page_pipeline.rs` (`effective_base_*base_uri*` —
кросс-origin `<base href>` отклонён `base-uri example.com` и база
документа остаётся исходной; совпадающий host пропускается;
`base-uri 'none'` отбрасывает даже относительный href).

`cargo test -p lumen-network --lib` (2273 passed), `cargo test -p
lumen-shell --features v8 --bin lumen` (1923 passed, 0 failed) без
регрессий; `cargo clippy -p lumen-network --all-targets -- -D warnings` и
`cargo clippy -p lumen-shell --all-targets --features v8 -- -D warnings`
чисто.

## Срез 33 (2026-09-19, `p6-gap-cspenf-srez33`) — директива `navigate-to` против перехода по `<a href>`

После среза 32 `navigate-to` осталась последней директивой `CspDirective` с
парсингом (`crates/network/src/csp.rs:473`), но без единой точки применения:
`grep -rn "NavigateTo\|navigate_to_allowed" crates/` до этого среза давал
ровно два совпадения — вариант перечисления и строка парсера. Политика
`navigate-to 'none'` принималась и не мешала клику по ссылке уйти куда
угодно.

- `crates/network/src/csp.rs`: новый `CspPolicy::navigate_to_allowed`
  (`target_url`, `self_origin`) — та же форма, что `frame_ancestor_allowed`
  (срез 27)/`form_action_allowed` (срез 29)/`base_uri_allowed` (срез 32):
  **без фолбэка на `default-src`**, CSP3 §6.4 относит `navigate-to` к
  navigation directives наравне с ними; отсутствие директивы значит «не
  ограничено». `form-action` намеренно не участвует: директивы независимы,
  страница, объявившая одну, не ограничивает другую (отдельный тест
  `navigate_to_is_independent_of_form_action`).
- `crates/shell/src/csp_enforce.rs::navigate_to_blocked` — тонкая обёртка той
  же формы, что `form_action_blocked`/`base_uri_blocked` выше в файле:
  нераспарсившийся URL нарушением не считается (fail-open, как у каждого
  fetch-гейта этого модуля).
- `crates/shell/src/lumen/click.rs`: политика документа читается тем же
  ОДНИМ заимствованием `src.document.lock()`, что уже отдаёт `href`/`target`/
  `rel` кликнутого якоря (`csp_enforce::document_csp_policy(&doc, root)`) —
  отдельный проход мог бы увидеть документ, изменённый скриптом между двумя
  чтениями, тот же аргумент, по которому срез 29 считает `csp_gate` внутри
  `prepared`-замыкания `run_form_submission`.
- Новый приватный `Lumen::navigate_to_link_blocked(csp_gate, href)` вызывается
  ОДИН раз — до всего дерева ветвления `_blank`/именованный фрейм/именованная
  вкладка/фрагмент/обычный переход: все пять веток кончаются навигацией на
  один и тот же резолвленный URL, поэтому проверка в каждой была бы пятью
  копиями одного ответа. `href` резолвится (`self.source.resolve_href`) ДО
  сверки с политикой — та же ловушка сырого значения атрибута, на которой
  срез 4 сначала «fail open»-ил для `<img src>`.
- `securitypolicyviolation` диспатчится через уже существующий
  `route_task_js` + `PersistentJs::fire_csp_violation("navigate-to", …)` (тот
  же путь, что `form-action` в `form_submit.rs`), и переход не происходит
  вовсе — ни новой вкладки, ни сетевого запроса.

Не тронуто этим срезом (осознанно, чтобы срез остался одной директивой против
одной точки потребления): `<a href="javascript:…">` — код исполняется в
кликнувшем документе и никуда не навигирует, сравнивать директиве не с чем
(исполнение — дело `script-src`, срез 1), поэтому гейт стоит ПОСЛЕ раннего
`return` этой ветки; навигация из JS (`location.href`/`location.assign`/
`window.open`) и из `<meta http-equiv=refresh>` — другие точки потребления, по
одной на срез; ссылки внутри `<iframe>` (`frame_links.rs`/`click.rs` ветка
фрейма) — политика ребёнка, тот же разрыв, что срезы 8/22/24 закрывали
отдельными срезами для своих директив; переход «назад/вперёд» по истории
(`navigate-to` его не покрывает по спеке); директива `sandbox` — единственная
оставшаяся распарсенная-но-не-применённая, но её применение требует
интеграции с моделью sandbox-флагов `<iframe sandbox>`
(`crates/js/src/iframe_element.rs`) и не сужается до одного среза этого
рисунка.

Тесты: +5 в `crates/network/src/csp.rs` (`navigate_to_allows_listed_host`,
`navigate_to_none_blocks_every_target`,
`navigate_to_self_matches_documents_own_origin`,
`navigate_to_absent_does_not_fall_back_to_default_src`,
`navigate_to_is_independent_of_form_action`), +5 в
`crates/shell/src/csp_enforce.rs` (обёртка `navigate_to_blocked`: блокировка
неперечисленной цели, пропуск перечисленной, отсутствие фолбэка на
`default-src`, `'self'` относительно origin документа,
`navigate_to_unparseable_url_not_blocked`).

`cargo test -p lumen-network --lib` (2278 passed, 0 failed) и `cargo test -p
lumen-shell --features v8 --bin lumen` (1928 passed, 0 failed) без регрессий;
`cargo clippy -p lumen-network -p lumen-shell --all-targets --features v8 --
-D warnings` чисто.

## Срез 34 (2026-09-19, `p6-gap-cspenf-srez34`) — директива `navigate-to` против `location.href`/`.assign()`/`.replace()`

После среза 33 у `<a href>` был гейт, а у JS-навигации — нет: `navigate-to
'none'` не мешало `location.href = 'https://…'` уйти куда угодно, хотя
`navigate-to` покрывает ЛЮБУЮ навигацию верхнего документа, а не только клик.
Все три JS-формы (`location.href=`, `.assign()`, `.replace()`) уже
схлопнуты JS-шимом в один нативный вызов `_lumen_navigate(url, replace)`
(`_lumen_navigate_or_fragment`, `web_api_shim_mid_b.js:233`) и одну точку
исполнения на Rust-стороне — `Lumen::on_about_to_wait`'а
`pending_js_navigate`-ветку (`crates/shell/src/app/about_to_wait.rs:1725`),
где `JsNavigateRequest::Push`/`Replace` оба вызывают `resolve_js_navigation`
перед фактической навигацией. Ровно этот выбор консультирующий агент назвал
самым узким следующим срезом: одна точка потребления, минимум веток —
`window.open` неоднородна (`_self`/именованный target/reuse/opener), а
`<meta http-equiv=refresh>` вообще не отдельный путь — она уже проходит через
эту же ветку (шим превращает её в одноразовый `setTimeout` с
`location.replace`/`location.reload`, `scripts.rs:729-750`), так что этот
срез закрывает её бесплатно.

- Новый приватный `Lumen::js_navigate_to_blocked(url)`
  (`about_to_wait.rs`, та же форма, что `click.rs::navigate_to_link_blocked`
  среза 33): один `layout_source.document.lock()`, `document_csp_policy`,
  `csp_enforce::navigate_to_blocked`, при блокировке — `route_task_js` +
  `fire_csp_violation("navigate-to", …)`, `eprintln!` и возврат `true`.
  Вызывается в НАЧАЛЕ каждой из веток `Push`/`Replace` (после проверки
  `javascript:`-URL, которую срез 33 тоже не трогал по той же причине —
  `javascript:` исполняется на месте, а не навигирует), так что при блокировке
  `resolve_js_navigation`/`navigate_to`/`navigate_replace` не вызываются
  вовсе — ни истории, ни сети.
- `url`, дошедший до `js_navigate_to_blocked`, уже прогнан JS-шимом через
  `new URL(raw, base).href`, т.е. абсолютный — `self.source.resolve_href`
  внутри гейта на нём чаще всего no-op, но вызывается всё равно: тот же
  путь для случая, когда шим не смог распарсить и передал сырую строку,
  что и у клика (иначе гейт незаметно перестал бы совпадать с ним на границе).
- `js_navigate_to_blocked` — отдельная функция с собственным
  `#[allow(clippy::unwrap_used)]` (блокировка мьютекса), а не встраивание в
  уже помеченный `on_about_to_wait`: тело `Mutex::lock` живёт в новой функции,
  а не в месте вызова.

Не тронуто этим срезом (по той же логике «один срез — одна точка
потребления»): `window.open` (сложнее по ветвлению, кандидат на отдельный
срез); переход по истории (`navigate-to` его не покрывает по спеке);
ссылки/JS-навигация внутри `<iframe>` (политика ребёнка — тот же разрыв, что
уже закрывали по директивам срезы 8/22/24); директива `sandbox` — по-прежнему
единственная распарсенная-но-не-применённая, требует интеграции с моделью
sandbox-флагов `<iframe sandbox>` и не сужается до одного среза.

Живая проверка (`--mcp-port`, персистентный V8): страница с
`<meta http-equiv="Content-Security-Policy" content="navigate-to 'self'">`,
`eval` `location.href='https://other.example/blocked'` — `location.href`
после вызова остаётся исходным `file://`-адресом документа, перехода не
произошло. Чистых unit-тестов на сам `js_navigate_to_blocked` не добавлено:
покрываемая им логика (`navigate_to_blocked`/`document_csp_policy`) уже имеет
10 тестов среза 33 (`crates/network/src/csp.rs` + `crates/shell/src/
csp_enforce.rs`), а сам метод — только проводка в новую точку потребления,
той же формы, что `click.rs::navigate_to_link_blocked`, который тоже не
покрыт отдельным unit-тестом (тот путь тоже верифицировался живым пробником).

`cargo clippy -p lumen-shell --all-targets --features v8 -- -D warnings`
чисто. `cargo test -p lumen-driver --test all` — один снятый провал,
`cases::snapshot_cpu::cpu_snapshots_match_references`, тот же байтовый
сигнатурный дрейф 7 файлов (BUG-1008), воспроизводимый на `main` при
`git stash` этого диффа — чужой, не регрессия этого среза.

## Срез 35 (2026-09-19, `p6-gap-cspenf-srez35`) — директива `navigate-to` против `window.open()`

Срез 34 сам назвал `window.open` не тронутым — «сложнее по ветвлению
(`_self`/именованный target/reuse/opener), кандидат на отдельный срез». Это
он: до этого среза `navigate-to 'self'` не мешало
`window.open('https://other.example/…')` открыть новую вкладку и уйти куда
угодно — цикл обработки popup-запросов в `Lumen::on_about_to_wait`
(`about_to_wait.rs:1206`) никогда не спрашивал CSP.

- Новый приватный `Lumen::window_open_navigate_to_blocked(url)`
  (`about_to_wait.rs`, тот же контур, что `js_navigate_to_blocked` среза 34
  и `click.rs::navigate_to_link_blocked` среза 33): один
  `layout_source.document.lock()`, `document_csp_policy`,
  `csp_enforce::navigate_to_blocked`, при блокировке — `route_task_js` +
  `fire_csp_violation("navigate-to", …)`, `eprintln!` и возврат `true`.
- Вызывается в ветке `resolved`-вычисления ДО `open_new_tab()`/`switch_tab()`
  — пока `self.source`/`self.layout_source` ещё указывают на OPENER, не на
  ещё не существующую вкладку: навигация проверяется политикой
  инициатора, ровно как того требует спека (CSP3 §6.9). При блокировке
  `resolved` получает `Err("blocked by CSP navigate-to")` — тот же путь,
  которым уже идут прочие отказы этой ветки (невалидный URL,
  web→file guard), так что новая вкладка всё равно открывается (та же
  вкладка-заглушка `about:blank`, что и у любого другого `Err` здесь), но
  СЕТЕВОЙ РЕСУРС не запрашивается: `resolve_js_navigation`/сеть не
  вызываются вовсе.
- `javascript:` URL и пустой `url` (`about:blank`) проверяются раньше этой
  ветки и гейт не проходят — тот же принцип, что срез 34 применил к
  `location.href=`: `javascript:` исполняется в контексте OPENER'а, а не
  навигирует, поэтому `navigate-to` его не касается.

Не тронуто этим срезом: навигация внутри уже открытого `window.open()`-попапа
(его собственный `location.href=`/клики уже гейтятся срезами 33/34 — это не
разрыв, а естественное покрытие); история (`navigate-to` её не покрывает по
спеке); `<iframe>`/дочерние документы (отдельный, ранее закрытый пласт —
срезы 8/22/24); директива `sandbox` (по-прежнему единственная
распарсенная-но-не-применённая).

Живая проверка — `tests/wpt/verify_gap_cspenf_window_open_navigate_to.py`
(`--mcp-live-port`, HTTP-сервер, dev-release, коммит текущего среза):
страница A с `navigate-to 'self'` открывает `window.open('/.wo-allowed.html')`
(same-origin) — сервер видит запрос, новая вкладка навигирует туда; затем,
со свежей вкладки той же политики, `window.open('http://127.0.0.1:<чужой
порт без сервера>/.wo-blocked.html')` (cross-origin) — сервер на чужом порту
НЕ получает запроса (иначе — connection-refused в логе, а не тишина), а
`stderr` содержит `window.open: navigation to … blocked by CSP
navigate-to`. Оба случая — ЗЕЛЁНЫЙ. Чистых unit-тестов на сам
`window_open_navigate_to_blocked` не добавлено — та же причина, что у среза
34 (`navigate_to_blocked`/`document_csp_policy` уже 10 тестов среза 33,
новый метод — только проводка).

`cargo clippy -p lumen-shell --all-targets -- -D warnings` чисто.

## Срез 36 (2026-09-19, `p6-gap-cspenf-srez36`) — директива `navigate-to` против ссылок под-документа `<iframe>`

Срезы 33/34/35 сами назвали ссылки/JS-навигацию ВНУТРИ фрейма отдельным
пробелом — «политика ребёнка, тот же разрыв, что срезы 8/22/24 закрывали
отдельными срезами для своих директив»: `frame_links.rs::frame_link_click`
никогда не спрашивал CSP, поэтому `navigate-to 'self'`, объявленная РЕБЁНКОМ
в его собственной `<meta>`, не мешала клику по его ссылке уйти на любой
чужой origin — ни `grep -rn "navigate_to" crates/shell/src/lumen/frame_links.rs`,
ни `frame_form_submit.rs` (тот покрыт `form-action`, срез 29, отдельной
директивой) не находили ни одной точки применения `navigate-to` в этом файле.

- Новый приватный `Lumen::frame_navigate_to_link_blocked(idx, csp_gate, href,
  nav_base)` (`crates/shell/src/lumen/frame_links.rs`) — зеркало
  `click.rs::navigate_to_link_blocked` (срез 33), но политика и origin
  сравнения — собственные у РЕБЁНКА (`nav_base`, `handle.base`), а не у
  страницы: ссылку написал ребёнок, поэтому именно его `navigate-to` решает,
  куда ему можно, тем же принципом, что `frame_form_submit.rs`'s
  `form_action_blocked`-вызов уже применяет для `form-action` (срез 29).
- `frame_link_click`: политика ребёнка (`csp_enforce::document_csp_policy`)
  читается ОДНИМ заимствованием вместе с `href`/`target`/`rel` — та же
  причина, что уже даёт этой тройке единый лок (срез 24): отдельный проход
  мог бы увидеть документ, изменённый скриптом ребёнка между двумя чтениями.
  Гейт стоит ОДИН раз ПЕРЕД всем деревом ветвления `_blank`/именованный
  фрейм/именованная вкладка/`_top`/`_self`/`_parent` — тот же порядок, что
  срез 33 уже даёт `<a href>` страницы: каждая из веток `LinkTarget`
  кончается навигацией на один и тот же резолвленный `href`, так что
  проверка внутри каждой была бы пятью копиями одного ответа.
- `securitypolicyviolation` уходит прямым `eval_js`-хэндлом ребёнка
  (`fire_csp_violation`), не через `route_task_js` (тот адресует только
  контекст СТРАНИЦЫ) — тот же путь, что `frame_form_submit.rs` уже
  использует для `form-action` во фрейме.

Не тронуто этим срезом: `javascript:`-ссылки ребёнка — `frame_link_click`
никогда их не исполнял (`links::is_navigable_href` отфильтровывает схему
раньше любой навигации), гейтить нечего; JS-навигация фрейма
(`location.href=` и т.п. в контексте ребёнка) — ещё один разрыв той же формы,
что срезы 34/35 закрыли для страницы, но для ребёнка отдельная точка
потребления; `window.open()` из скрипта ребёнка — туда же, отдельный срез.

Живая проверка —
`tests/wpt/verify_gap_cspenf_frame_navigate_to.py` (`--mcp-live-port`,
HTTP-сервер, dev-release, коммит текущего среза): родитель с двумя `<iframe>`,
у каждого своя `navigate-to 'self'`; клик по ссылке на СВОЙ origin в первом —
сервер видит запрос, окно фрейма меняет цвет; клик по ссылке на ДРУГОЙ origin
(порт без поднятого сервера) во втором — ни один сервер не видит запроса
(тишина, не connection-refused — сам факт этим не доказывался бы), а `stderr`
содержит `iframe: navigation to … blocked by CSP navigate-to`. Оба случая —
ЗЕЛЁНЫЙ. Чистых unit-тестов на сам `frame_navigate_to_link_blocked` не
добавлено — та же причина, что у срезов 34/35: покрываемая логика
(`navigate_to_blocked`/`document_csp_policy`) уже имеет 10 тестов среза 33, а
новый метод — только проводка в ещё одну точку потребления.

`cargo test -p lumen-shell --features v8 --bin lumen` (1951 passed, 0 failed,
20 ignored) без регрессий; `cargo clippy -p lumen-shell --all-targets
--features v8 -- -D warnings` чисто.

## Срез 37 (2026-09-19, `p6-gap-cspenf-srez37`) — `style-src-attr` против точечной мутации `style=""` без затрагивания `<style>`/`<link>`

Срез 23 дал top-level документу проверку атрибута `style=""`, но её
пересчёт (`page_pipeline.rs::parse_and_layout`) был привязан к
`scripts_changed_css` — триггеру «скрипт тронул `<style>`/`<link>`», а не к
факту мутации DOM вообще. Скрипт, который создаёт новый узел с `style=""`
(или меняет его на существующем через `setAttribute`/`style.cssText`) и не
трогает ни одного `<style>`/`<link>`, оставлял `style_attr_csp_blocked`
снимком ДО своей мутации — для нового узла это буквально пустой список
(узла ещё не существовало на момент первого прохода `build_page_cascade`),
так что запрещённый инлайн-стиль доезжал до layout незаблокированным.
Разрыв назван самим срезом 23 («узел, получивший `style=""` другим путём
после этого момента … гейт не видит») и подтверждён живым воспроизведением
через `parse_and_layout_for_test` (см. регресс-тест ниже — падает на
`main` без этого среза).

- `crates/shell/src/page_pipeline.rs::parse_and_layout` — рядом с уже
  существующим `dom_touched` (флаг «скрипты трогали DOM», до этого среза
  использовался только для решения «пересчитывать layout-снимок или нет»):
  если `dom_touched` истинен, а `scripts_changed_css` — нет, документ
  перечитывается на `style-src-attr` заново (`document_csp_policy` +
  `collect_style_attr_csp_blocked`, те же примитивы, что срез 23 уже
  использует и что уже покрыты своими unit-тестами в `doc_extract.rs`) —
  без пересборки каскада/layout, это отдельный дешёвый проход по дереву.
  Условие исключает `scripts_changed_css`, чтобы не дублировать работу:
  та ветка уже делает это как часть полной пересборки `cascade`.
- Покрывает ровно один пост-скриптовый чекпойнт (тот же момент, что срез 23
  уже перепроверял) — не живой хук на каждую мутацию. Более поздняя
  асинхронная мутация (обработчик события, таймер, уже после этой точки)
  по-прежнему не гейтится — та же граница, что срез 23 сам обозначил, шире
  не стала и не сужалась специально этим срезом.
- `<iframe>`-документы не тронуты (у них своя, ранее закрытая точка —
  срез 24, — и своя отдельная схема пересчёта в `frames.rs`, не
  `page_pipeline.rs`); та же граница «top-level документ отдельно от
  фрейма», что срезы 21/22 и 23/24 уже провели.

Регресс-тест `script_created_style_attr_is_csp_checked_without_stylesheet_touch`
(`crates/shell/src/tests/page_pipeline.rs`) — страница с `style-src-attr
'none'`, скрипт создаёт `<div style="color:rgb(255,0,0)">` и добавляет его в
`document.body` БЕЗ единого `<style>`/`<link>`, `DOMContentLoaded`-хендлер
читает `getComputedStyle` на этот узел; без фикса цвет доезжает
(`rgb(255, 0, 0)`), с фиксом — блокируется. Проверено вручную откатом фикса
(`git stash` только по `page_pipeline.rs`) — тест красный без него, зелёный
с ним.

`cargo test -p lumen-shell --features v8 --bin lumen` (1953 passed, 0
failed, 20 ignored) без регрессий; `cargo clippy -p lumen-shell
--all-targets --features v8 -- -D warnings` чисто. Полный
`scripts/scoped-test.sh` не был доведён до конца — замыкание тянет
`lumen-network`, и прогон упёрся в уже известный [BUG-805](BUG-805-OPEN.md)
(бинарь виснет навсегда независимо от правки); до зависания видны только
чужой дрейф — `cpu_snapshots_match_references` (BUG-1008, тот же
7-файловый сигнатурный набор: `55-text-rendering`, `57-canvas-2d`,
`32-list-markers`, `34-forms`, `45-multiple-backgrounds`,
`51-scrollbar-rendering`, `1000000-final`) и два непроверенных отдельно
падения `lumen-network --lib` (`http_cache_miss_fetches_and_stores`,
`auth_digest_sha256_response_is_64_hex`) — ни один из трёх не связан с
CSP/`page_pipeline.rs`, файлы этого среза их не трогают.

## Срез 38 (2026-09-19, `p6-gap-cspenf-srez38`) — `style-src`/`default-src` против цели `@import`

Список «не покрыто» с самого начала (срез 1, `csp_enforce.rs` doc-comment) и
срез 14 отдельно называли пробел: `@import` внутри уже загруженного листа
«наследует политику владельца, отдельно не проверяется». До этого среза
`inline_css_imports` (`crates/shell/src/stylesheets.rs`) — единственная
функция, резолвящая `@import`-цепочки, общая для ВСЕХ трёх сайтов
потребления (собственные `@import` внешнего `<link>`, `@import` top-level
инлайнового `<style>`, `@import` инлайнового `<style>` внутри `<iframe>`) —
не принимала CSP вообще: `style-src '<host>'`, запрещавший внешний
`<link>`, не мешал тому же самому листу дотянуться до запрещённого хоста
через `@import`, если сам `<link>` был на разрешённом origin (или лист был
инлайновым `<style>`, у которого вообще нет проверки фетча).

- `crates/shell/src/stylesheets.rs::inline_css_imports` — новый параметр
  `csp_gate: Option<(&CspPolicy, Option<&Origin>)>`, той же формы, что
  каждый fetch-гейт этого файла уже принимает (`style_src_blocked`, срез 7).
  Проверяется КАЖДЫЙ уровень вложенности: `csp_gate` передаётся дальше без
  изменений в рекурсивный вызов — CSP3 §6.4.1 не даёт импортированному
  листу своей политики, действует политика владельца всей цепочки.
  Заблокированная цель не фетчится вовсе (та же «сеть её не видела»
  гарантия, что срез 7 даёт `<link>`) и её resolved URL уходит вторым
  элементом возврата — функция сменила тип с `String` на
  `(String, Vec<String>)`.
- Три сайта потребления подключены к общему `blocked_by_style_src`, который
  `page_pipeline.rs`/`frames.rs` уже дispatch'ат как
  `securitypolicyviolation` (`violatedDirective="style-src"`) для
  заблокированного `<link>` — новый список просто подмешивается в тот же
  вектор ДО дispatch'а, отдельного события/пути не заводилось:
  - `stylesheets.rs::load_linked_stylesheets` — свой `csp_gate`/
    `self_origin` уже посчитаны для проверки самого `<link>`, переданы в
    рекурсивный вызов `inline_css_imports` без изменений; per-item
    `Result` сменил тип с `Result<String, Option<String>>` на
    `Result<(String, Vec<String>), Option<String>>`, чтобы каждый лист,
    загруженный параллельно (`parallel_map`), нёс свои заблокированные
    импорты независимо.
  - `page_pipeline.rs::build_page_cascade` — уже посчитанный `csp_policy`
    (срез 21) плюс новый `self_origin = base.origin()`, переданы в
    `inline_css_imports` top-level инлайновых `<style>`; возвращённый
    список подмешан в `blocked_by_style_src`, пришедший от
    `load_linked_stylesheets`, ДО общего `securitypolicyviolation`-dispatch
    (строка ~1282 этого файла, не тронута — уже итерирует весь вектор).
  - `frames.rs::fetch_frame_subresources` — та же схема, но политика
    ЧАДА (уже посчитанный `csp_gate`, срез 22), для его собственного
    инлайнового `<style>`.
- `self_origin` для `'self'`-источников — всегда origin ВЛАДЕЮЩЕГО
  документа (`base.origin()` снаружи `inline_css_imports`), не origin
  импортированного листа: тот же принцип, что `base_uri_allowed` (срез 32)
  уже применяет — политика документа решает про весь фетч, который он
  порождает, транзитивно.

Обнаружено попутно (не баг, задокументировано тестом
`inline_css_imports_style_src_none_blocks_file_import`): `style-src 'none'`
блокирует `@import` даже для `file://`-документов — `ResourceBase::
resolve_str` для File-базы возвращает абсолютный путь ОС (`D:\...\b.css`),
и WHATWG URL-парсер (`lumen_core::url::Url::parse`) успешно превращает
Windows-путь с буквой диска в `file:`-URL вместо ошибки, так что "не
парсится → fail-open" здесь не применяется — CSP действует на `file://`
так же, как на любую другую схему.

Тесты: +4 в `crates/shell/src/tests/page_resources.rs`
(`inline_css_imports_style_src_blocks_cross_origin_import`,
`inline_css_imports_style_src_self_allows_same_origin_target`,
`inline_css_imports_style_src_none_blocks_file_import` — три новых сценария
— плюс все 7 существующих `inline_css_imports_*`-тестов обновлены под
новую сигнатуру `(String, Vec<String>)` и передают `None` там, где CSP не
участвует, доказывая обратную совместимость). `cargo test -p lumen-shell
--features v8 --bin lumen` (1963 passed, 0 failed, 20 ignored) без
регрессий; `cargo clippy -p lumen-shell --all-targets --features v8 -- -D
warnings` чисто.

Не покрыто этим срезом: остальные пункты общего списка «не покрыто»
(`report-to`, честная независимая проверка заголовка/`<meta>`,
`ServiceWorker`-конструирование, `manifest-src`) — ни один из них не
пересекается с `@import`, отдельные задачи.

## Срез 39 (2026-09-19, `p6-gap-cspenf-srez39`) — `style-src-attr` против атрибута `style=""` внутри `<iframe>` после точечной DOM-мутации

Срез 37 дал top-level странице пере-проверку `style_attr_csp_blocked` после
ЛЮБОЙ мутации DOM (`dom_touched`), не только после смены `<style>`/`<link>`
(`scripts_changed_css`) — срез 23 сам называл этот пробел. Срез 24
распространил исходный, спавн-времени срез 23 на `<iframe>`, но пере-проверка
среза 37 осталась только у страницы: `Document::style_attr_csp_blocked`
ребёнка считался РОВНО ОДИН РАЗ, при спавне (`fetch_frame_subresources`, до
его собственных скриптов), и точечная мутация `style=""` уже существующего
узла (`setAttribute`/`style.cssText`, без единого `<style>`/`<link>`) доезжала
до layout ребёнка незаблокированной.

- `crates/shell/src/frames.rs::relayout_frame_content` — единственная
  функция, куда `about_to_wait.rs` зовёт per-фреймовый пост-скриптовый
  чекпойнт: `own_dirty || bridge_dirty` (тот же сигнал, что уже гейтит любую
  другую работу после тика скриптов ребёнка, включая `poll_dynamic_frames`)
  собирается в `frame_dirty`, и на каждый затронутый индекс зовётся
  `Lumen::refresh_frames(Some(idx))` → эта функция. Перед пересчётом layout
  здесь теперь пере-считывается `document_csp_policy` ребёнка (та же функция,
  что срез 8/22/23/24/25 уже используют для него) и `collect_style_attr_csp_blocked`
  (срез 23) — набор пишется прямо на `Document` тем же
  `set_style_attr_csp_blocked`, что использует спавн.
- Пересчёт БЕЗУСЛОВНЫЙ при каждом вызове (в отличие от страницы, где срез 37
  гейтит его на `dom_touched && !scripts_changed_css`): у фрейма нет аналога
  `scripts_changed_css` — его `FrameHandle::sheet` вообще не пересчитывается
  после спавна (CSSOM-1 сам называет это будущей работой), так что
  единственный вход `relayout_frame_content` УЖЕ означает «дерево ребёнка
  тронуто», а сам проход — тот же дешёвый обход дерева, что срез 23 уже
  делает один раз при спавне.
- Нативное переключение элемента формы (срез 18, `frame_forms.rs`) идёт тем же
  путём: оно тоже зовёт `relayout_frame_content` и получает ту же
  пере-проверку бесплатно, хотя обычно не меняет `style=""` — просто ещё одна
  причина не гейтить пересчёт отдельным флагом.
- Попутно исправлен дрейф doc-comment'а этого файла (`csp_enforce.rs`):
  список «не покрыто» ошибочно называл `ServiceWorker`-конструирование не
  покрытым — срезы 30/31 закрыли это раньше, чем список получил свою
  текущую форму, и он не был обновлён тогда.

Тест: +1 в `crates/shell/src/tests/scripts_and_frames.rs`
(`relayout_frame_content_rechecks_style_attr_csp_after_mutation` — фрейм с
`style-src-attr 'none'` и узлом БЕЗ атрибута `style=` на момент спавна;
атрибут добавляется точечно (`Attribute`/`NodeData::Element`, тот же приём,
что `forms::toggle_details_open` уже использует для мутации живого дерева
ребёнка без JS), `relayout_frame_content` зовётся напрямую, читается
`style.color` свежего layout-бокса — без фикса цвет доезжает
(`rgb(255, 0, 0)`, подтверждено откатом правки `frames.rs` через
`git stash` только по этому файлу — тест красный без него, зелёный с ним), с
фиксом блокируется).

`cargo test -p lumen-shell --features v8 --bin lumen` (1978 passed, 0
failed, 20 ignored) без регрессий; `cargo clippy -p lumen-shell
--all-targets --features v8 -- -D warnings` чисто.

Не покрыто этим срезом: `<style>`/`<link>` самого ребёнка по-прежнему не
пересчитываются после спавна (CSSOM-1); асинхронная мутация без
сопутствующего `own_dirty`/`bridge_dirty` (тот же сигнал, что скрипт ребёнка
уже обязан взвести для ЛЮБОЙ другой пост-скриптовой работы, поэтому отдельно
не сужается); остальные пункты общего списка «не покрыто» (`report-to`,
`manifest-src`, честная независимая проверка заголовка/`<meta>`).

## Срез 40 (2026-09-19, `p6-gap-cspenf-srez40`) — заголовок и `<meta>` теперь проверяются как независимые политики

Список «не покрыто» держал этот пункт с самого среза 1: заголовок
`Content-Security-Policy` и каждая `<meta http-equiv="Content-Security-Policy">`
по CSP3 §3.4 — независимые политики, нарушение ЛЮБОЙ из них — нарушение;
движок сливал их в одну строку через `;` перед единственным парсингом, а
`CspPolicy::directives` — `HashMap`, который при повторении одной директивы
(например, `script-src` и в заголовке, и в `<meta>`) хранит только ПОСЛЕДНЕЕ
встреченное значение. На практике это значило, что более поздняя из двух
политик (обычно `<meta>`, идёт в тексте документа после ответа сервера) могла
тихо ОСЛАБИТЬ более раннюю: строгий заголовок `script-src 'self'` плюс
`<meta>` со `script-src 'unsafe-inline'` — по спеке инлайн обязан остаться
заблокированным (заголовок его запрещает), а склеенная строка
`"script-src 'self'; script-src 'unsafe-inline'"` парсится в одну политику, где
`'unsafe-inline'` побеждает как последний записанный источник.

- [`csp_enforce::document_csp_policy`](../crates/shell/src/csp_enforce.rs) —
  единственная точка сборки политик документа — парсит заголовок и каждую
  `<meta>` ПО ОТДЕЛЬНОСТИ вместо склейки, возвращая `Vec<CspPolicy>` вместо
  одной `CspPolicy`. Каждая из 13 `_blocked`-функций этого файла
  (`inline_script_blocked`, `inline_style_blocked`, `style_attribute_blocked`,
  `script_src_blocked`, `img_src_blocked`, `style_src_blocked`,
  `frame_src_blocked`, `media_src_blocked`, `font_src_blocked`,
  `frame_ancestors_blocked`, `form_action_blocked`, `base_uri_blocked`,
  `navigate_to_blocked`) теперь принимает `&[CspPolicy]` и блокирует, если
  блокирует ХОТЯ БЫ ОДНА политика из списка (`policies.iter().any(...)`) —
  ровно CSP3 §3.4's "any policy forbids it".
- Изменение — механическое распространение нового типа через все точки
  потребления в `crates/shell/src/` (`doc_extract.rs`, `stylesheets.rs`,
  `subresources.rs`, `frames.rs`, `lumen/click.rs`, `lumen/form_submit.rs`,
  `lumen/frame_form_submit.rs`, `lumen/frame_links.rs`, `page_load.rs`,
  `page_pipeline.rs`, `relayout.rs`, `scripts.rs`) — компилятор нашёл каждый
  сайт несовпадения типов, ни один тест логики самих `_blocked`-функций не
  тронут (сигнатура сменилась, поведение для единственной политики — самый
  частый случай — идентично).
- НЕ мигрирован этим срезом: `lumen-network::HttpClient`'s
  `connect-src`/`worker-src`/`object-src`/`media-src` (`with_connect_src_policy`
  и три соседа, `crates/network/src/lib.rs`) — у `HttpClient` нет
  `&Document`/точки парсинга по месту, а поле хранит ровно одну `CspPolicy` на
  клиент; `page_pipeline.rs`'s единственный вызов, который их настраивает,
  теперь читает новый `csp_enforce::document_csp_policy_combined` — тот же
  склеенный-в-одну-строку способ, что был у ВСЕХ директив до этого среза,
  сохранённый специально для этих четырёх. Threading `Vec<CspPolicy>` через
  `HttpClient` — отдельная, более широкая работа (поле `Option<(CspPolicy,
  Option<Origin>, String)>` нужно менять на `Vec<(...)>` в четырёх местах,
  плюс порядка 20 юнит-тестов конструкторов `with_*_policy` в
  `crates/network/src/lib.rs`).

Новый юнит-тест `strict_header_is_not_loosened_by_a_lenient_meta_policy`
(`csp_enforce.rs`) — строгий `script-src 'self'` через один "источник" и
лояльный `script-src 'unsafe-inline'` через другой; `inline_script_blocked`
над списком из обеих политик обязано остаться `true`. `cargo test -p
lumen-shell --features v8 --bin lumen csp` (90 passed, 0 failed — было 89 до
нового теста) без регрессий; `cargo clippy -p lumen-shell --all-targets
--features v8 -- -D warnings` чисто.

Не покрыто этим срезом: `lumen-network::HttpClient`'s четыре gate'а (см.
выше), `report-to` (Reporting API, этот движок его не разбирает),
`manifest-src` (нечем фетчить манифест — гейтить нечего), честная
независимая проверка МЕЖДУ несколькими occurrences ОДНОГО И ТОГО ЖЕ
заголовка (`page_source.rs::content_security_policy_header` уже склеивает
повторный `Content-Security-Policy`-заголовок ответа в одну строку до того,
как этот срез вообще видит текст — CSP3 §3.4 тоже требует независимости и
здесь, но это отдельный, более редкий случай, не тронутый этим срезом).

## Срез 41 (2026-09-19, `p6-gap-cspenf-srez41`) — несколько occurrences ОДНОГО заголовка `Content-Security-Policy` теперь тоже независимые политики

Ровно тот пробел, что срез 40 сам назвал не покрытым в конце своего раздела
выше. Живая проба (`/tmp`-сервер, два экземпляра заголовка:
`Content-Security-Policy: script-src 'none'`, затем `Content-Security-Policy:
script-src 'unsafe-inline'`, инлайновый `<script>`, меняющий текст `<p
id="marker">`): до фикса `./target/dev-release/lumen.exe --dump-layout
http://127.0.0.1:<port>/` показывал `"INLINE_SCRIPT_RAN"` — инлайн выполнился,
хотя первая политика (`'none'`) обязана заблокировать его независимо от
второй. Контроль (тот же сервер, только строгий заголовок один раз) корректно
давал заблокированный текст `"before"`.

Причина, буквально повторяющая срез 40 на один уровень раньше:
`page_source::content_security_policy_header` фильтровала все вхождения
заголовка и делала `parts.join("; ")` ДО того, как `document_csp_policy`
вообще видело текст — то есть заголовок доезжал до `document_csp_policy` уже
как один готовый "part" наравне с каждой `<meta>`, и `CspPolicy::directives`
(`HashMap`) при повторении одной директивы в двух occurrences заголовка
хранил только последнее значение.

- [`page_source::content_security_policy_header`](../crates/shell/src/page_source.rs)
  теперь возвращает `Vec<String>` — один элемент на occurrence заголовка —
  вместо склеенного `Option<String>`.
- [`Document::csp_header`](../crates/engine/dom/src/lib.rs) — поле, геттер
  (`csp_header(&self) -> &[String]`) и сеттер (`set_csp_header(&mut self,
  Vec<String>)`) сменили тип с `Option<String>`/`Option<&str>` на
  `Vec<String>`/`&[String]`, той же механической заменой, что срез 40 уже
  дал `CspPolicy` → `Vec<CspPolicy>`.
- [`csp_enforce::document_csp_policy`](../crates/shell/src/csp_enforce.rs)/
  `document_csp_policy_combined` теперь берут `doc.csp_header().to_vec()`
  (весь список occurrences) вместо `doc.csp_header().map(str::to_owned)
  .into_iter().collect()` (один опциональный элемент) — остальная логика
  (добавить `<meta>`-части, распарсить каждую по отдельности) не изменилась.
- Изменение распространилось через `page_pipeline.rs::{render_bytes,
  parse_and_layout}` (параметр `csp_header: Option<&str>` →
  `csp_header: &[String]`) и все вызывающие сайты — `page_source.rs::load`,
  `app/user_event.rs`, `dump_mode.rs` (четыре точки: скриншот, PDF, PDF с
  опциями, `--dump-layout`/`--dump-display-list`) — компилятор нашёл каждый
  сайт несовпадения типов.
- Новый юнит-тест `repeated_response_header_stays_independent`
  (`csp_enforce.rs`) — два occurrences заголовка через `set_csp_header`,
  первый строгий (`script-src 'none'`), второй лояльный (`'unsafe-inline'`);
  `document_csp_policy` над результатом обязано остаться блокирующим. Плюс
  `page_source.rs::csp_header_repeated_stays_independent` (переименован из
  `csp_header_repeated_is_merged`, срез 5) проверяет сам список, а не
  склеенную строку. `cargo test -p lumen-shell --profile dev-release csp`
  (91 passed, 0 failed — было 90 до этого среза) без регрессий; `cargo
  clippy -p lumen-shell -p lumen-dom --all-targets -- -D warnings` чисто.

Не покрыто этим срезом: то же, что срез 40 оставил открытым —
`lumen-network::HttpClient`'s четыре gate'а, `report-to`, `manifest-src`.

## Срез 42 (2026-09-19, `p6-gap-cspenf-srez42`) — четыре gate'а `lumen-network::HttpClient` тоже независимые политики

Ровно тот пробел, что срезы 40/41 сами называли не покрытым: `connect-src`
(`fetch()`/`XMLHttpRequest`/WebSocket/EventSource/`sendBeacon`), `worker-src`
(`new Worker()`/`new SharedWorker()`), `object-src` (`<embed>`/`<object>`) и
`media-src` (`<video>`/`<audio>`/`<track>`) проверялись против одного
смёрженного `CspPolicy` — те же `document_csp_policy_combined`, что срез 40
завёл как временный костыль именно для этих четырёх директив, потому что у
`HttpClient` нет `&Document`/точки парсинга по месту.

- [`HttpClient::{connect_src,worker_src,object_src,media_src}_policy`](../crates/network/src/lib.rs)
  сменили тип поля с `Option<(CspPolicy, Option<Origin>, String)>` на
  `Option<(Vec<CspPolicy>, Option<Origin>, String)>` — та же механическая
  замена, что срез 40 уже дал `csp_enforce.rs`'s `_blocked`-функциям. Все
  четыре `with_*_policy`-билдера и четыре `*_gate`-функции обновлены: гейт
  блокирует, если нарушает ХОТЯ БЫ ОДНА политика из списка
  (`policies.iter().any(...)`) — ровно CSP3 §3.4's "any policy forbids it".
- `document_csp_policy_combined` (`crates/shell/src/csp_enforce.rs`) удалена —
  `page_pipeline.rs`'s единственный вызов, настраивающий все четыре gate'а,
  теперь зовёт [`document_csp_policy`] напрямую и передаёт весь `Vec<CspPolicy>`
  каждому из четырёх `with_*_policy`.
- `original_policy` остаётся одной склеенной строкой (не по одной на
  политику) — то же упрощение, что CSP3 §7.8 требует текст именно
  НАРУШЕННОЙ политики, а не всех сразу; этот пробел уже назван в doc-comment
  [`document_csp_policy`](../crates/shell/src/csp_enforce.rs) и не тронут этим
  срезом.

Новый юнит-тест `connect_src_strict_policy_is_not_loosened_by_a_lenient_one`
(`crates/network/src/lib.rs`) — строгий `connect-src 'none'` и лояльный
`connect-src example.com` как два элемента `Vec`; `fetch_request` на
`example.com` обязан остаться `CspConnectSrcBlocked`. Тот же
"strict-is-not-loosened" рисунок, что срез 40 уже проверяет для
`csp_enforce.rs`'s `_blocked`-функций.

`cargo test -p lumen-network connect_src` (10/10, было 9), `worker_src`
(6/6), `object_src` (5/5), `media_src` (5/5) — все без регрессий; `cargo
clippy -p lumen-network --all-targets -- -D warnings` (чисто); `cargo build
-p lumen-shell --profile dev-release --features v8` + `cargo clippy -p
lumen-shell --profile dev-release --all-targets --features v8 -- -D
warnings` (оба чисто); `cargo test -p lumen-shell --profile dev-release
--features v8 --bin lumen csp` (91 passed, 0 failed — без изменений, этот
срез не тронул `csp_enforce.rs`'s тестируемую логику).

Не покрыто (остаток общего списка, не изменился этим срезом): `report-to`
(Reporting API, этот движок его не разбирает), `manifest-src` (нечем
фетчить манифест — гейтить нечего), честная per-policy `originalPolicy`
вместо одной склеенной строки (см. выше).

## Срез 43 (2026-09-19, `p6-gap-cspenf-srez43`) — `upgrade-insecure-requests` переписывает `http://` картинки главного документа в `https://`

Первая директива этой дорожки, которая ничего не БЛОКИРУЕТ: она меняет сам
запрос. Поле `CspPolicy::upgrade_insecure_requests` парсится с самого начала
(`crates/network/src/csp.rs:165`/`:440`, юнит-тест
`parse_upgrade_insecure_requests`), но до этого среза не читалось нигде —
ровно то, на что заведён отдельный [BUG-692](BUG-692-OPEN.md) (`rg
upgrade_insecure_requests crates/` давал только сам `csp.rs`).

Живая проба (`.tmp/srez43/serve.py` — python-сервер на `127.0.0.1`,
логирующий каждую строку запроса; страница с `<meta
http-equiv="Content-Security-Policy" content="upgrade-insecure-requests">` и
`<img src="http://127.0.0.1:<port>/pixel.png">`; `./target/dev-release/
lumen.exe --dump-layout http://127.0.0.1:<port>/`):

- ДО фикса лог сервера: `HIT /` и `HIT /pixel.png`, `"GET /pixel.png
  HTTP/1.1" 200` — картинка уехала открытым `http://`, хотя политика
  требует апгрейда.
- ПОСЛЕ фикса `HIT /pixel.png` нет вовсе; вместо него сервер получает два
  TLS ClientHello (`code 400, message Bad request version ('\x16...')`) —
  по одному на каждого сработавшего producer'а, — а stderr браузера пишет
  `Пропуск картинки https://127.0.0.1:<port>/pixel.png: network error: TLS
  handshake…`. То есть запрос ушёл на `https://`, как требует спецификация
  (простой http-сервер пробы, естественно, TLS не терминирует).

Реализация:

- [`csp_enforce::upgrade_insecure_url(policies, url) -> Option<String>`](../crates/shell/src/csp_enforce.rs)
  — `Some(новый URL)`, если директива есть ХОТЯ БЫ в одной политике
  документа (CSP3 §3.4, тот же `policies.iter().any(...)`, что у всех
  гейтов файла с среза 40) И схема `url` — `http`. Порт руками не трогается:
  дефолтный `:80` WHATWG-парсер (`lumen_core::url::Url`) сворачивает сам,
  поэтому смена схемы даёт 443, а явный `:8080` спецификация сохраняет.
  Исключений для loopback/IP нет умышленно — [UIR] §4.1 шаг 5 смотрит
  только на схему, а «Should insecure requests be upgraded for client?» —
  только на наличие директивы; то, что живые движки не апгрейдят
  `http://localhost`, — их собственное решение, а не текст спецификации
  (и именно поэтому проба на `127.0.0.1` вообще что-то показывает).
- Порядок: апгрейд идёт ДО гейта `img-src`, а не после — Fetch §4.1 «main
  fetch» ставит upgrade шагом 5, а «should request be blocked by Content
  Security Policy» — шагом 6. Поэтому и `img-src`, и cross-origin-
  классификация (GAP-CANVASORIGIN), и сам фетч видят уже `https://`-адрес.
- Подключено во всех трёх producer'ах картинок ГЛАВНОГО документа — тех
  же трёх, что срезы 4/9 уже гейтят по `img-src`:
  [`subresources::fetch_and_decode_images`](../crates/shell/src/subresources.rs)
  (eager-пайплайн), [`page_load::spawn_image_requests`](../crates/shell/src/page_load.rs)
  (streaming/dynamic — стартует РАНЬШЕ eager-прохода, без него `http://`-
  байты успевали уйти в сеть первыми) и
  [`page_load::fetch_and_register_lazy_images`](../crates/shell/src/page_load.rs)
  (отложенный `loading="lazy"`).
- Ключ `IMAGE_CACHE`/реестра рендерера во всех трёх точках остаётся сырым
  `req.url` (его знают layout и рендерер) — апгрейд меняет только адрес
  запроса. Когда переписывать нечего, `upgrade_insecure_url` возвращает
  `None` и все три точки работают строго как раньше, по сырому URL.

Пять новых юнит-тестов (`csp_enforce.rs`): нет директивы — не переписывается;
`http` → `https` с сохранением пути/запроса/фрагмента; явный `:8080`
сохраняется, а `:80` сворачивается; `https`/`data:`/непарсящийся URL не
трогаются; директива в любой из двух независимых политик включает апгрейд.
`cargo test -p lumen-shell --profile dev-release --features v8 --bin lumen
csp` — 96 passed, 0 failed (было 91 до этого среза, +5 новых, регрессий нет);
`cargo clippy -p lumen-shell --profile dev-release --all-targets --features
v8 -- -D warnings` — чисто.

Не покрыто этим срезом (всё это — продолжение BUG-692, статус которого
остаётся OPEN): `upgrade-insecure-requests` для картинок внутри `<iframe>`
и для `background-image` (`frames.rs`, та же форма гейта — механическое
продолжение), для `<script src>`, `<link rel=stylesheet>`/`@import`,
`@font-face url()`, `<video>`/`<audio>`/`<track>`, `fetch()`/XHR/WebSocket
(`ws://` → `wss://`), для навигаций верхнего документа и `<iframe>`, а также
заголовок `Upgrade-Insecure-Requests: 1` на навигационном запросе и
`upgrade insecure navigations set` (UIR §4.1 шаги 1-2). Остаток общего
списка дорожки не изменился: `report-to`, `manifest-src`, честная
per-policy `originalPolicy`.

## Срез 44 (2026-09-19, `p6-gap-cspenf-srez44`) — `upgrade-insecure-requests` для картинок внутри `<iframe>`

Механическое продолжение среза 43, названное им же не покрытым: тот же
`csp_enforce::upgrade_insecure_url`, но применённый к обоим продюсерам
картинок ЧУЖОГО (child) документа `<iframe>` — `<img>` и
`background-image`/`cross-fade()` — против политики самого фрейма, а не
top-level документа (тот же принцип разделения политик, что уже применяют
срезы 6/8/25 к скриптам/стилям/шрифтам фрейма).

- [`crates/shell/src/frames.rs::fetch_frame_subresources`](../crates/shell/src/frames.rs)
  — цикл декодирования `<img>` теперь считает `upgrade_insecure_url(policy,
  &key)` ДО `img_src_blocked` (тот же порядок Fetch §4.1, что срез 43 дал
  странице), и передаёт апгрейженный адрес в `decode_image`. Ключ реестра
  (`frame_image_key` — резолвленный, но НЕ апгрейженный `req.url`) не
  меняется — апгрейд трогает только адрес фактического запроса, как и на
  странице.
- [`fetch_frame_background_images`](../crates/shell/src/frames.rs) — тот же
  приём для `background-image`: `upgrade_insecure_url` перед
  `img_src_blocked`, апгрейженный адрес идёт в `fetch_image_bytes`, а
  возвращаемый `(url, key)`-контракт для [`FrameHandle`] остаётся на сыром
  `url` без изменений.

Подтверждено живой пробой (`.tmp/srez44/serve.py`, простой HTTP-сервер на
`127.0.0.1`, логирующий каждую строку запроса; top-level документ и
`<iframe>` оба несут `<meta http-equiv="Content-Security-Policy"
content="upgrade-insecure-requests">`, фрейм содержит `<img
src="http://…/img.png">` и `background-image: url('http://…/bg.png')`):
после `GET /frame.html` сервер НЕ получает `GET /img.png`/`GET /bg.png` —
вместо них два TLS ClientHello (`code 400 … Bad request version`), а stderr
браузера пишет `GET https://…/img.png` и `GET https://…/bg.png` с
последующим `TLS handshake: received corrupt message` (простой http-сервер
пробы TLS не терминирует, ожидаемо) — то есть оба запроса фрейма реально
ушли на `https://`, как и у top-level документа среза 43.

`cargo test -p lumen-shell --profile dev-release --features v8 --bin lumen
csp` — 96 passed, 0 failed (без изменений числа — логика переиспользует уже
протестированный `upgrade_insecure_url`, новых юнит-тестов не потребовалось,
живая проба покрывает именно два новых call site); `cargo clippy -p
lumen-shell --profile dev-release --all-targets --features v8 -- -D
warnings` — чисто. `scripts/scoped-test.sh` не догнан до конца — тот же
известный сломанный гейт [BUG-805](BUG-805-OPEN.md), не регрессия этого
среза.

Не покрыто этим срезом (продолжение BUG-692, не изменилось): `<script
src>`, `<link rel=stylesheet>`/`@import`, `@font-face url()` (везде — и
top-level, и `<iframe>`), `<video>`/`<audio>`/`<track>`, `fetch()`/XHR/
WebSocket (`ws://` → `wss://`), навигации верхнего документа и `<iframe>`,
заголовок `Upgrade-Insecure-Requests: 1` на навигационном запросе и
`upgrade insecure navigations set` (UIR §4.1 шаги 1-2). Остаток общего
списка дорожки не изменился: `report-to`, `manifest-src`, честная per-policy
`originalPolicy`.

## Срез 45 (2026-09-20, `p6-gap-cspenf-srez45`) — `upgrade-insecure-requests` для `<script src>` top-level документа

Механическое продолжение среза 44, названное срезами 43/44 не покрытым:
тот же `csp_enforce::upgrade_insecure_url`, но применённый к единственному
оставшемуся *fetch*-производителю top-level документа, у которого его ещё
не было — внешним `<script src>` (`scripts.rs::resolve_script_sources`,
уже гейтящей `script-src` срезом 6).

- [`scripts::resolve_script_sources`](../crates/shell/src/scripts.rs) —
  `resolved_url` (сырой, резолвленный `base.resolve_str(src)`) сначала идёт
  через `upgrade_insecure_url`; апгрейженный адрес (`gate_url`) — то, что
  видит `script_src_blocked` (тот же порядок Fetch §4.1, что срезы 43/44
  уже дали картинкам: апгрейд — шаг 5, CSP-гейт — шаг 6). Ветка
  `ResolvedResource::File` апгрейд не видит (`file:` никогда не совпадёт со
  схемой `http`, менять там нечего); в ветке `ResolvedResource::Url`
  апгрейженный адрес заменяет сырой ДО `Url::parse`/фактического фетча —
  заблокированный или неапгрейженный URL не различить снаружи: апгрейд
  просто меняет то, что реально уходит в сеть, тем же принципом «ни одного
  исходящего байта», что и остальные гейты этого файла.
- Функция чистая и уже покрыта пятью юнит-тестами среза 43
  (`csp_enforce.rs`) — новых не потребовалось, как и в срезе 44; изменился
  только вызывающий код, не сама логика переписывания.

Подтверждено живой пробой (`.tmp/srez45/serve.py`, простой HTTP-сервер на
`127.0.0.1`; страница несёт `<meta http-equiv="Content-Security-Policy"
content="upgrade-insecure-requests">` и `<script src="http://…/
listener.js">`): сервер получает ровно `GET /frame.html`, а на
`listener.js` — только TLS ClientHello (`code 400 … Bad request version`) и
`code 400 … Bad HTTP/0.9 request type`; stderr браузера пишет `GET
https://127.0.0.1:8791/listener.js` с последующим `TLS handshake: received
corrupt message` (простой http-сервер пробы TLS не терминирует, ожидаемо)
— запрос скрипта реально ушёл на `https://`, тем же рисунком, что срезы
43/44 уже показали для картинок.

`cargo test -p lumen-shell --profile dev-release --features v8 --bin lumen
csp` — 96 passed, 0 failed (без изменений числа, как и в срезе 44 —
переиспользуется уже протестированная `upgrade_insecure_url`); `cargo
clippy -p lumen-shell --profile dev-release --all-targets --features v8 --
-D warnings` — чисто. `scripts/scoped-test.sh` не догнан до конца — тот же
известный сломанный гейт [BUG-805](BUG-805-OPEN.md), не регрессия этого
среза.

Не покрыто этим срезом (продолжение BUG-692, не изменилось): `<link
rel=stylesheet>`/`@import`, `@font-face url()` (везде — и top-level, и
`<iframe>`), `<video>`/`<audio>`/`<track>`, `fetch()`/XHR/WebSocket
(`ws://` → `wss://`), навигации верхнего документа и `<iframe>`,
заголовок `Upgrade-Insecure-Requests: 1` на навигационном запросе и
`upgrade insecure navigations set` (UIR §4.1 шаги 1-2). Остаток общего
списка дорожки не изменился: `report-to`, `manifest-src`, честная
per-policy `originalPolicy`.

## Срез 46 (2026-09-20, `p6-gap-cspenf-srez46`) — доки-уточнение: `<script src>` внутри `<iframe>` уже покрыт срезом 45

Без нового кода. Срез 45 назвал `<script src>` внутри `<iframe>`
непокрытым по аналогии с картинками (срез 44 добавлял отдельный call
site в `frames.rs::fetch_frame_subresources` для чужого документа) — но
у скриптов, в отличие от картинок, никогда не было раздельных
top-level/frame реализаций: `resolve_script_sources`
(`crates/shell/src/scripts.rs`) — одна и та же функция, которую
`page_pipeline.rs` зовёт с top-level `&Document`, а `frames.rs:2024-2025`
— с `child_doc` фрейма (то же разделение политик, что срез 6 дал
`script-src`). Апгрейд, добавленный срезом 45 внутри этой функции,
поэтому применился к обоим вызывающим кодам одновременно, без отдельной
правки.

Подтверждено живым пробой (`.tmp/srez46/serve.py`, простой HTTP-сервер
на `127.0.0.1:8792`; `top.html` содержит `<iframe src=".../frame.html">`,
`frame.html` несёт `<meta http-equiv="Content-Security-Policy"
content="upgrade-insecure-requests">` и `<script
src="http://127.0.0.1:8792/listener.js">`): сервер получает `GET
/top.html` и `GET /frame.html`, но не `GET /listener.js` — stderr
браузера пишет `GET https://127.0.0.1:8792/listener.js` с последующим
`TLS handshake: received corrupt message` (простой http-сервер пробы TLS
не терминирует, ожидаемо) — запрос скрипта фрейма реально ушёл на
`https://` без какой-либо правки кода в этом срезе.

Код не менялся, тестами подтверждать нечего; `cargo clippy -p
lumen-shell --profile dev-release --all-targets --features v8 -- -D
warnings` — чисто (без изменений в исходниках).

## Срез 47 (2026-09-20, `p6-gap-cspenf-srez47`) — `upgrade-insecure-requests` для `<link rel=stylesheet>` и `@import`

Механическое продолжение срезов 43-46, названное срезом 45 не покрытым:
`csp_enforce::upgrade_insecure_url`, применённая к обеим точкам фетча
CSS в [`stylesheets.rs`](../crates/shell/src/stylesheets.rs) — внешнему
`<link rel=stylesheet>` (`load_linked_stylesheets`) и `@import`
(`inline_css_imports`), которые, как и `<script src>`, не имеют
раздельных top-level/`<iframe>` реализаций: обе вызываются одинаково из
`page_pipeline.rs` и `frames.rs`, поэтому один срез закрыл сразу оба
контекста.

- [`fetch_stylesheet_text`](../crates/shell/src/stylesheets.rs) — общая
  точка фактического фетча для `<link>` и `@import` — получила новый
  параметр `csp_gate: Option<(&[CspPolicy], Option<&Origin>)>` (та же пара,
  что уже принимает `inline_css_imports`); в ветке `ResolvedResource::Url`
  сырой резолвленный адрес (`raw_url`) сначала идёт через
  `upgrade_insecure_url`, апгрейженный — то, что реально уходит в
  `PREFETCH_CACHE`/сеть и что возвращается как база для вложенных
  `@import`. Ветка `ResolvedResource::File` апгрейд не видит по той же
  причине, что и в `scripts.rs` (`file:` не совпадёт со схемой `http`).
- `load_linked_stylesheets` — гейт `style_src_blocked` для каждого `<link>`
  теперь смотрит на апгрейженный `gate_url`, а не на сырой
  `resolved_url` (тот же порядок Fetch §4.1, что и `<script src>`
  среза 45) — иначе гейт видел бы `http://`, а фактический фетч уже
  `https://`, и хост мог бы обойти `style-src`-политику, отличающуюся для
  двух схем.
- `inline_css_imports` — тот же приём для цели каждого `@import`: `key`
  (сырой резолв, используемый и для `seen`-дедупа циклов) апгрейдится
  только для гейта/фетча, дедуп циклов остаётся на сыром адресе — это не
  вопрос безопасности, а защита от бесконечной рекурсии.
- [`build_stylesheet_node_registry`](../crates/shell/src/stylesheets.rs)
  (CSSOM `document.styleSheets`, отдельный проход по тем же `<link>`,
  читающий `PREFETCH_CACHE` второй раз) тоже получила тот же `csp_gate`:
  без него она резолвила бы `href` заново без апгрейда, промахивалась
  мимо кэш-записи под апгрейженным ключом и тихо повторно фетчила бы
  `http://` — открытый небезопасный запрос ради одного только CSSOM,
  которого сам каскад уже не делает.
- Пять юнит-тестов `upgrade_insecure_url` (срез 43) переиспользованы без
  изменений — новых не потребовалось, менялись только вызывающие коды.

Подтверждено живой пробой (`.tmp/srez47/serve.py`, простой HTTP-сервер на
`127.0.0.1:8793`; страница несёт `<meta http-equiv="Content-Security-Policy"
content="upgrade-insecure-requests">` и `<link rel="stylesheet"
href="http://…/style.css">`, лист несёт собственный `@import`): stderr
браузера показывает `GET https://127.0.0.1:8793/style.css` с последующим
`TLS handshake: received corrupt message` (простой http-сервер пробы TLS
не терминирует, ожидаемо), сервер получает только `GET /top.html` — запрос
листа реально ушёл на `https://` и не долетел до сервера ни разу по
`http://`, тем же рисунком, что срезы 43-46 уже показали для картинок и
скриптов.

`cargo test -p lumen-shell --profile dev-release --features v8 --bin lumen
csp` — 96 passed, 0 failed (без изменений числа, как и в срезах 44-46);
`cargo clippy -p lumen-shell --profile dev-release --all-targets --features
v8 -- -D warnings` — чисто. `scripts/scoped-test.sh` не догнан до конца —
тот же известный сломанный гейт [BUG-805](BUG-805-OPEN.md), не регрессия
этого среза.

Не покрыто этим срезом (продолжение BUG-692, не изменилось): `@font-face
url()` (везде — и top-level, и `<iframe>`), `<video>`/`<audio>`/`<track>`,
`fetch()`/XHR/WebSocket (`ws://` → `wss://`), навигации верхнего документа
и `<iframe>`, заголовок `Upgrade-Insecure-Requests: 1` на навигационном
запросе и `upgrade insecure navigations set` (UIR §4.1 шаги 1-2). Остаток
общего списка дорожки не изменился: `report-to`, `manifest-src`, честная
per-policy `originalPolicy`.

## Срез 48 (2026-09-20, `p6-gap-cspenf-srez48`) — `upgrade-insecure-requests` для `@font-face url()`

Механическое продолжение срезов 43-47, названное срезом 47 не покрытым:
`csp_enforce::upgrade_insecure_url` применена к обоим существующим
производителям `@font-face`-фетча — top-level (`page_load.rs`, async-поток
FOUT-паттерна PH3-19) и `<iframe>` (`frames.rs::load_frame_fonts`,
синхронный путь FRAME-5) — оба уже гейтили `font-src` срезами 19/25, но
`http://`-адрес, который они гейтили и фетчили, был сырым.

- [`page_load.rs`](../crates/shell/src/page_load.rs) — `resolved` (сырой
  `base.resolve_str(&pf.url)`) сначала идёт через `upgrade_insecure_url`;
  апгрейженный `gate_url` — то, что видит `font_src_blocked`, что несёт
  `fire_csp_violation` при блокировке и что реально уходит в
  `fetch_font_bytes` внутри спавненного потока (тот же порядок Fetch §4.1,
  что срезы 43-47 уже дали остальным производителям).
- [`frames.rs::load_frame_fonts`](../crates/shell/src/frames.rs) — тот же
  приём: `gate_url` вместо `resolved` и в `font_src_blocked`, и в
  `blocked_by_font_src`, и в `fetch_font_bytes`.
- `local()`-источники (уже резолвленные `load_font_faces` до этого цикла,
  системный шрифт по имени) апгрейд не видит — там нет URL со схемой
  `http`, менять нечего, как и в предыдущих срезах.
- Пять юнит-тестов `upgrade_insecure_url` (срез 43) переиспользованы без
  изменений; `cargo test -p lumen-shell --profile dev-release --features
  v8 --bin lumen csp` — 96 passed, 0 failed.

Подтверждено двумя живыми пробами (простой HTTP-сервер на `127.0.0.1`,
`<meta http-equiv="Content-Security-Policy" content="upgrade-insecure-requests">`
+ `@font-face { src: url("http://…/font.woff2") }`): top-level страница —
stderr показывает `GET https://127.0.0.1:8794/font.woff2` с последующим
`TLS handshake: received corrupt message`, сервер получает только `GET
/top.html`; `<iframe>`-вариант (`top.html` → `<iframe src="frame.html">`,
CSP и `@font-face` во фрейме) — та же картина: `GET /top.html`, `GET
/frame.html`, TLS ClientHello на `/font.woff2`, ни одного `http://`-запроса
шрифта не долетело до сервера ни в одном из двух случаев.

`cargo clippy -p lumen-shell --profile dev-release --all-targets --features
v8 -- -D warnings` — чисто. `scripts/scoped-test.sh` не догнан до конца —
тот же известный сломанный гейт [BUG-805](BUG-805-OPEN.md), не регрессия
этого среза.

Не покрыто этим срезом (продолжение BUG-692, не изменилось):
`<video>`/`<audio>`/`<track>`, `fetch()`/XHR/WebSocket (`ws://` → `wss://`),
навигации верхнего документа и `<iframe>`, заголовок
`Upgrade-Insecure-Requests: 1` на навигационном запросе и `upgrade
insecure navigations set` (UIR §4.1 шаги 1-2). Остаток общего списка
дорожки не изменился: `report-to`, `manifest-src`, честная per-policy
`originalPolicy`.

## Срез 49 (2026-09-20, `p6-gap-cspenf-srez49`) — `upgrade-insecure-requests` для `fetch()`/`XMLHttpRequest`/WebSocket/EventSource

Реализовано (`crates/network/src/lib.rs`): срез 48 назвал этот пункт
непокрытым — переписывался только parser-driven трафик (`script src`/`link
rel=stylesheet`/`@import`/`img src`/`@font-face url()`, срезы 43-48), а
JS-инициированный сетевой слой (`HttpClient`) апгрейд не видел вовсе, хотя
это тот же самый Fetch §4.1 шаг 5.

- `HttpClient::upgrade_insecure_requests_url(url: Url) -> Url` — та же
  схема, что `lumen_shell::csp_enforce::upgrade_insecure_url` уже применяет
  parser-driven подресурсам (директивы нет → не трогать; схема не
  `http`/`ws` → не трогать), продублированная здесь, а не переиспользованная:
  `lumen-network` лежит ниже `lumen-shell` в графе крейтов и не может его
  импортировать. Источник политик — уже существующий `connect_src_policy`
  (тот же `Vec<CspPolicy>`, что `connect_src_gate` проверяет), новых полей
  не потребовалось. В отличие от prose-версии в `csp_enforce.rs`, здесь два
  исхода схемы: `http` → `https` и `ws` → `wss` — WebSocket-хендшейк тоже
  `connect-src`-гейтится (CSP3 §6.7.2) и попадает под тот же алгоритм
  апгрейда Mixed Content, на который ссылается UIR.
- Вызывается сразу после `Url::parse`, до соответствующего гейта (UIR §4.1
  шаг 5 обязан идти раньше блокирующей проверки, шаг 6) в четырёх точках:
  `fetch_request_impl` (общее тело `fetch()`/XHR — синхронных, cancellable
  и async вариантов), `check_connect_src` (I/O-free пре-чек
  `sendBeacon` — сам беакон переиспользует `fetch_with_body_sync`, значит и
  он проходит через `fetch_request_impl` без отдельной правки),
  `JsWebSocketProvider::connect` и `JsSseProvider::connect_sse`.

Не тронуто этим срезом: `check_worker_src`/`check_object_src`/
`check_media_src` (`new Worker()`/`<embed>`/`<object>`/`<video>`/`<audio>`/
`<track>` — свои директивы, свои сетевые пути, не связаны с
`connect_src_policy`) — остаются в списке непокрытого наравне с
навигациями и `report-to`.

5 юнит-тестов на `upgrade_insecure_requests_url` (http→https, ws→wss,
https/wss не трогаются, без директивы не трогает, без политики вовсе не
трогает) в `crates/network/src/lib.rs`; `cargo test -p lumen-network --lib`
(2285 passed, 0 failed) и `cargo test -p lumen-shell --features v8 --bin
lumen csp` (96 passed) не регрессировали. `cargo clippy --workspace
--all-targets -- -D warnings` — чисто.

Не покрыто этим срезом (продолжение BUG-692): `<video>`/`<audio>`/`<track>`
(отдельные директивы/пути, см. выше); навигации верхнего документа и
`<iframe>`; заголовок `Upgrade-Insecure-Requests: 1` на навигационном
запросе и `upgrade insecure navigations set` (UIR §4.1 шаги 1-2). Остаток
общего списка дорожки не изменился: `report-to`, `manifest-src`, честная
per-policy `originalPolicy`.

## Срез 50 (2026-09-20, `p6-gap-cspenf-srez50`) — `upgrade-insecure-requests` для `<track src>`

Срезы 48/49 назвали `<video>`/`<audio>`/`<track>` непокрытыми. Из троих
только `<track>` фетчится по пути, у которого есть `&Document` и живая
CSP-политика документа до того, как этот фетч случится
([`page_pipeline.rs`](../crates/shell/src/page_pipeline.rs)`::parse_and_layout`,
замыкание для `tracks::load_video_tracks`) — тот же путь, что срез 17 уже
гейтит `media-src`/`default-src`. `<video>`/`<audio>` фетчатся из
JS-шима (`__lumen_video_load`/`__lumen_audio_load`) через собственные
загрузчики, не через это замыкание — вне скоупа этого среза.

- [`page_pipeline.rs`](../crates/shell/src/page_pipeline.rs) — тот же
  приём, что срезы 43-48: `gate_url = upgrade_insecure_url(policy, &abs)`
  (или сам `abs`, если апгрейдить нечего) идёт и в `media_src_blocked`, и
  в `fetch_vtt_text`, а не резолвленный, но не апгрейженный `abs`. Порядок
  сохранён — Fetch §4.1 шаг 5 (апгрейд) раньше шага 6 (гейт).
- `<track>` также фетчится вторым путём — JS-шима `readTrackBody`
  (`video_element.js`), который вызывает голый `fetch()`; тот уже апгрейдит
  сам, потому что срез 49 переписал `HttpClient::fetch_request_impl`
  целиком, а не только native-путь — отдельной правки не потребовалось.

Подтверждено живой пробой (простой HTTP-сервер на `127.0.0.1:8795`,
`<meta http-equiv="Content-Security-Policy" content="upgrade-insecure-requests">`
+ `<video><track src="http://127.0.0.1:8795/cap.vtt"></video>`,
`--screenshot`): до правки — `GET /cap.vtt` долетает до сервера как есть
(`404`, но байты ушли); после — `GET https://127.0.0.1:8795/cap.vtt` рвётся
на `TLS handshake: received corrupt message`, сервер получает только
`GET /top.html`.

Два юнит-теста `media_src_blocked`, что уже покрывали срез 17
(`media_src_none_blocks_track_fetch`, `no_media_src_allows_track_fetch`),
переиспользованы без изменений; `cargo test -p lumen-shell --profile
dev-release --features v8 --bin lumen csp` — 96 passed, `... track` — 27
passed, 0 failed. `cargo clippy --workspace --all-targets -- -D warnings`
— чисто. `scripts/scoped-test.sh` не догнан до конца — тот же известный
сломанный гейт [BUG-805](BUG-805-OPEN.md), не регрессия этого среза.

Не покрыто этим срезом (продолжение BUG-692): `<video src>`/`<audio src>`
через `__lumen_video_load`/`__lumen_audio_load` (свои HttpClient-загрузчики
вне `connect_src_policy`, не native-путь этого среза); навигации верхнего
документа и `<iframe>`; заголовок `Upgrade-Insecure-Requests: 1` на
навигационном запросе и `upgrade insecure navigations set` (UIR §4.1 шаги
1-2). Остаток общего списка дорожки не изменился: `report-to`,
`manifest-src`, честная per-policy `originalPolicy`.

## Срез 51 (2026-09-20, `p6-gap-cspenf-srez51`) — `upgrade-insecure-requests` для `<video src>` и `<audio src>`

Продолжение среза 50: закрывает оба пути, названных им как непокрытые
(`<track>` уже был закрыт срезом 50).

- **`<video src="*.gif">`** — GIF-фетч реально фетчится не там, где JS
  запускает загрузку (`startGifLoad` только queue-ит `(nid, src)` в
  `__lumen_video_load`), а на следующем тике рендер-цикла:
  `Lumen::tick_video_gifs` (`page_load.rs`), вызываемый из
  `redraw_requested`. Этот тик — единственная точка на этом пути с живым
  `&Document`, поэтому апгрейд сделан там же, тем же приёмом, что срезы 43/50:
  `upgrade_insecure_url(policy, &base.resolve_str(&src))` перед
  `fetch_image_bytes`. `media-src`-гейт (срез 17, `_lumen_check_media_src`)
  не тронут — он уже отработал JS-стороной до постановки в очередь.
- **`<audio src>`** — здесь ровно наоборот: `PlatformAudioPlayer::load`
  (`lumen-shell/platform/audio_player.rs`) — `fn load(&self, handle: u64,
  url: &str)` — вообще не получает `&Document`/CSP-политику, и его
  `fetch_audio_bytes` строит голый `HttpClient::new()` без единой
  `with_*_policy`. Апгрейдить там нечем, а значит апгрейд должен случиться
  ДО того, как этот `url` покинул JS. Новый метод трейта
  `JsFetchProvider::upgrade_insecure_request_url(&self, url: &str) -> String`
  (`lumen-core/src/ext.rs`, default no-op) — реализация на `HttpClient`
  (`lumen-network/src/lib.rs`) переиспользует уже существующий
  `upgrade_insecure_requests_url` через `connect_src_policy` (тот же
  `Vec<CspPolicy>`, что и `media_src_policy` — оба выставляются одним
  вызовом `with_connect_src_policy`/`with_media_src_policy` в
  `page_pipeline.rs`, так что политика видна методу вне зависимости от
  того, под каким именем её читают). Новый нативный биндинг
  `_lumen_upgrade_insecure_url` (`net.rs`, рядом с `_lumen_check_media_src`)
  вызывается в `startLoad` (`audio_element.rs`) на `_abs` до
  `_lumen_check_media_src` и до `__lumen_audio_load` — апгрейд раньше гейта,
  UIR §4.1 шаг 5 перед шагом 6, тот же порядок, что везде в этой дорожке.
  Побочный эффект: `__lumen_audio_load` теперь получает разрешённый
  абсолютный URL вместо сырого атрибута — старое ограничение «loader
  получает `url` как есть» (комментарий среза 17) снято попутно, раз апгрейд
  всё равно требует абсолютного адреса.
- `<track>`'s JS-fetch путь (`readTrackBody`, `video_element.js`) не тронут:
  срез 49 уже переписал `fetch_request_impl` целиком, апгрейд там бесплатный.

Подтверждено живой пробой (сырой TCP-сервер на `127.0.0.1`, `<meta
http-equiv="Content-Security-Policy" content="upgrade-insecure-requests">` +
`<video src="http://.../movie.gif">` + `<audio src="http://.../sound.mp3">`):
`<audio>` апгрейдится сразу — лог показывает `GET
https://127.0.0.1:PORT/sound.mp3`, рвущийся на `TLS handshake: received
corrupt message`. `<video>`'s GIF-фетч требует, чтобы `redraw_requested`
хотя бы раз выполнился ПОСЛЕ того, как JS поставил `nid` в очередь (окно
без анимации/скролла/ресайза может не перерисовываться сколько угодно долго
после первого кадра — таймеры и `about_to_wait` тикают независимо от этого
и в пробу не попались бы); после принудительного `Resized`-события лог
показывает тот же переход `GET http://.../movie.gif` →
`https://127.0.0.1:PORT/movie.gif` → тот же обрыв TLS-рукопожатия.
`cargo test -p lumen-shell --profile dev-release --features v8 --bin lumen
csp` — 96 passed; `... track` — 27 passed, 0 failed; `cargo test -p
lumen-network upgrade_insecure` — 6 passed. `cargo clippy -p lumen-shell
--all-targets --features v8`, `-p lumen-core --all-targets`, `-p
lumen-network --all-targets -- -D warnings` — чисто.

Не покрыто этим срезом: навигации верхнего документа и `<iframe>`,
заголовок `Upgrade-Insecure-Requests: 1` на навигационном запросе и
«upgrade insecure navigations set» (UIR §4.1 шаги 1-2) — это исчерпывает
названный срезом 50 список путей UIR не покрывает. Остаток общего списка
дорожки не изменился: `report-to`, `manifest-src`, честная per-policy
`originalPolicy`.

## Срез 52 (2026-09-20, `p6-gap-cspenf-srez52`) — `upgrade-insecure-requests` для навигаций (шаг 5 UIR §4.1)

Продолжение среза 51: закрывает URL-часть (UIR §4.1 шаг 5, «upgrade the
request's URL» — переписать `http:` в `https:` ДО того, как навигационный
гейт того же документа увидит адрес) для четырёх навигационных путей,
каждый из которых уже читает `document_csp_policy` для СВОЕЙ директивы
(`navigate-to`/`form-action`/`frame-src`):

- **Клик по ссылке** (`click.rs::navigate_to_link_blocked`) — гейт теперь
  сравнивает АПГРЕЙЖЕННЫЙ адрес, а не сырой `resolve_href`; новый метод
  `Self::resolve_and_upgrade_href` переиспользуется во всех трёх ветках
  диспатча ниже (`_blank`, именованный target/новая вкладка, обычная
  навигация в том же документе) — без него проверка и реальный переход
  смотрели бы на разные URL.
- **JS-навигация** (`location.href=`/`.assign()`/`.replace()`,
  `about_to_wait.rs`) — `js_navigate_to_blocked` переименован в
  `js_navigate_to_gate` и вместо `bool` возвращает `Option<String>`:
  `None` — заблокировано (`securitypolicyviolation` уже отправлен),
  `Some(url)` — адрес, апгрейженный ДО гейта, которым и вызывается
  `resolve_js_navigation`. `window.open()`
  (`window_open_navigate_to_blocked`) этим срезом не тронут — отдельный,
  пятый навигационный гейт, вне скоупа.
- **Отправка формы** (`form_submit.rs`) — обе ветки (`get`/POST) апгрейжают
  `resolved` перед `form_action_navigation_blocked`.
- **`<iframe>` вставка/навигация** (`frames.rs::spawn_frame`) — новая
  свободная функция `maybe_upgrade_frame_src` резолвит и апгрейжит
  `src`/`href` ДО `frame_src_check`, но нарочно НЕ трогает пустой/
  `about:`/`data:`/`javascript:` src: резолв пустой строки против базы
  родителя вернул бы адрес самого родителя (RFC 3986 §5.3 — пустая ссылка
  резолвится в саму базу), и фрейм без содержимого начал бы сетеваться на
  страницу-хозяина вместо пустого документа.

Общая переиспользуемая часть — `csp_enforce::upgrade_navigation_url`,
обёртка над `upgrade_insecure_url` (срез 43) над уже прочитанным
`document_csp_policy`-гейтом; три из четырёх мест выше вызывают её
напрямую, `js_navigate_to_gate` — `upgrade_insecure_url` без обёртки (уже
держит `policy` расплетённым из гейта).

Не покрыто этим срезом: `window.open()` (`window_open_navigate_to_blocked`)
и навигация ссылки ВНУТРИ фрейма (`frame_links.rs::frame_link_click`,
срез 36) — оба читают ту же политику для `navigate-to`, но остаются
пятым/шестым навигационным гейтом со своим деревом ветвления, не
покрытым этим срезом; заголовок `Upgrade-Insecure-Requests: 1` на
навигационном запросе и «upgrade insecure navigations set» (UIR §4.1
шаги 1-2) — тоже, как и назвал срез 51. Остаток общего списка дорожки не
изменился: `report-to`, `manifest-src`, честная per-policy
`originalPolicy`.

`cargo test -p lumen-shell --profile dev-release --features v8 --bin lumen
csp` — 99 passed; `... frame` — 115 passed; `... navigate` — 15 passed;
`... form` — 151 passed (все три полных набора без единого нового
падения). `cargo clippy -p lumen-shell --all-targets --features v8 -- -D
warnings` — чисто. `scripts/scoped-test.sh` (14 крейтов, обратные
зависимости `lumen-shell`) — один красный тест,
`lumen-driver::snapshot_cpu::cpu_snapshots_match_references`, тот же
7-файловый байт-дельта-набор, что и известный несвязанный дрейф
[BUG-1008](BUG-1008-OPEN.md) (`text-rendering`/`canvas-2d`/
`list-markers`/`forms`/`multiple-backgrounds`/`scrollbar-rendering`/
`final`) — воспроизводится и на чистом `main` без этой ветки, не
регрессия этого среза.

## Срез 53 (2026-09-20, `p6-gap-cspenf-srez53`) — `upgrade-insecure-requests` для `window.open()` и ссылки внутри `<iframe>`

Продолжение среза 52: закрывает оба навигационных гейта, названных им как
непокрытые, — пятый и шестой из дерева ветвления `navigate-to`.

- **`window.open(url)`** (`about_to_wait.rs`) —
  `window_open_navigate_to_blocked` (`bool`) переименован в
  `window_open_navigate_to_gate` и, тем же приёмом, что срез 34 уже даёт
  `js_navigate_to_gate`, возвращает `Option<String>`: `None` — заблокировано
  (`securitypolicyviolation` уже отправлен), `Some(url)` — адрес,
  апгрейженный `upgrade_insecure_url` ДО гейта `navigate_to_blocked`.
  Вызывающая сторона в цикле обработки popup-запросов передаёт этот
  апгрейженный адрес в `resolve_js_navigation`, а не исходный сырой `url`.
- **Ссылка ВНУТРИ фрейма** (`frame_links.rs::frame_link_click`, тот путь,
  что срез 36 гейтил, но не апгрейжал) — новая функция модуля
  `resolve_and_upgrade_frame_href` (зеркало `click.rs::
  resolve_and_upgrade_href` среза 52, но резолвит против `nav_base`
  РЕБЁНКА и читает CSP-гейт РЕБЁНКА, не страницы — та же пара источников,
  что уже различает срез 36 у `frame_navigate_to_link_blocked`) заменяет
  каждый сырой `nav_base.resolve_str` на пути от гейта до реальной
  навигации: сам гейт, ветку `_blank`/именованная вкладка
  (`LinkTarget::NewWindow`), `navigate_page_from_frame` (`_top`/`_parent`
  глубины 0) и `navigate_frame_from_link` (`_self`/именованный фрейм) — все
  три функции навигации получили параметр `csp_gate` для этого. Апгрейженный
  адрес передаётся уже АБСОЛЮТНЫМ в `navigate_frame_to`; повторный резолв
  внутри него против той же базы идемпотентен для `http`/`https`
  (`ResourceBase::resolve`, ранний `return` на `starts_with("http://"
  ) || starts_with("https://")`), так что двойного резолва без апгрейда
  через эту ветку больше нет.
- Не тронуто: `same_document_fragment`/`fragment_only` проверки — они
  сравнивают/извлекают СЫРОЙ `href` (фрагмент вроде `#id` не резолвится в
  абсолютный URL до апгрейда, иначе `fragment_only` перестал бы его узнавать
  как фрагмент) и остаются на исходном значении, апгрейд применяется только
  к ветке, которая реально уходит в сеть.

Два новых юнит-теста на `resolve_and_upgrade_frame_href`
(`resolve_and_upgrade_frame_href_rewrites_when_directive_present`,
`resolve_and_upgrade_frame_href_no_gate_leaves_scheme_alone`) в
`crates/shell/src/lumen/frame_links.rs`, зеркало пары тестов
`click.rs::resolve_and_upgrade_href` среза 52. `cargo test -p lumen-shell
--profile dev-release --features v8 --bin lumen csp` — 99 passed; `...
frame` — 115 passed; `... navigate` — 15 passed (все три без нового
падения). `cargo clippy -p lumen-shell --all-targets --features v8 -- -D
warnings` — чисто. `scripts/scoped-test.sh` (14 крейтов) — тот же
единственный красный `lumen-driver::snapshot_cpu::
cpu_snapshots_match_references`, тот же 7-файловый набор, что срез 52 уже
подтвердил как несвязанный дрейф [BUG-1008](BUG-1008-OPEN.md), не
регрессия.

Живой пробой на реальном окне этот срез не проверен (нет готового
автоматизационного клика по кнопке/ссылке под рукой в этой сессии) —
корректность опирается на: 1) идентичность приёма с уже живьём
подтверждёнными срезами 43/50-52 (`upgrade_insecure_url`/
`upgrade_navigation_url` не менялись, только точки вызова); 2) полный
зелёный прогон `csp`/`frame`/`navigate` наборов; 3) новые юнит-тесты на
саму добавленную функцию с реальными `CspPolicy`/`ResourceBase`. Отдельная
проверка живьём остаётся желательной, но не блокирует этот срез.

Не покрыто (продолжение BUG-692): заголовок `Upgrade-Insecure-Requests: 1`
на навигационном запросе и «upgrade insecure navigations set» (UIR §4.1
шаги 1-2) — это исчерпывает список UIR-путей навигации, названный срезами
50-52. Остаток общего списка дорожки не изменился: `report-to`,
`manifest-src`, честная per-policy `originalPolicy`.

[UIR]: https://w3c.github.io/webappsec-upgrade-insecure-requests/

## Срез 54 (2026-09-20, `p6-gap-cspenf-srez54`) — заголовок `Upgrade-Insecure-Requests: 1` на навигационном запросе

Закрывает хвост, названный срезом 53: UIR §4.1 шаги 1-2 требуют, чтобы
навигационный запрос от документа, объявившего `upgrade-insecure-requests`,
нёс заголовок `Upgrade-Insecure-Requests: 1` — независимо от того,
переписала ли уже схему `upgrade_insecure_url` (заголовок это подсказка
серверу, а не запись о свершившемся апгрейде: он ставится и тогда, когда
URL и так был `https:`).

- **`lumen-network::HttpClient`** — `fetch_page`/`fetch_page_streaming`
  (`crates/network/src/lib.rs`) получили новый параметр
  `send_uir_header: bool`; при `true` строка `"Upgrade-Insecure-Requests:
  1\r\n"` подмешивается в уже существующий, ранее всегда пустой на этом
  пути слот `extra_request_headers` `fetch_with_redirect` (тот же канал, что
  несёт conditional-GET заголовки HTTP-кэша при cache revalidation —
  `format!("{uir_header}{}", snap.conditional_headers)` для этой ветки,
  просто `uir_header` для прямого запроса). Ни `http1/request.rs`, ни
  сериализация заголовков не тронуты — оба уже собирают `extra_headers` в
  общий блок.
- **`crate::csp_enforce::navigation_wants_uir_header`** (новая функция,
  `crates/shell/src/csp_enforce.rs`) — `true`, если хоть одна политика в уже
  вычисленном `csp_gate` несёт `upgrade_insecure_requests`; сестра
  `upgrade_navigation_url`, читает тот же tuple, что каждый `navigate-to`
  гейт этой дорожки уже держит в скоупе.
- **`PageSource::Url`** (`crates/shell/src/page_source.rs`) получил новое
  поле `upgrade_insecure_requests: bool`; `load_bytes`/
  `load_bytes_streaming` передают его в `fetch_page`/`fetch_page_streaming`
  как `send_uir_header`. Новый метод `PageSource::with_uir_header(flag)` —
  no-op на любом другом варианте — позволяет каждому call site дописать
  флаг поверх уже собранного источника одной цепочкой, не трогая
  `PageSource::url`/`from_arg`/`resolve_js_navigation` (десяток
  call-site-ов без CSP-контекста — адресная строка, история, автоматизация
  — не меняются, флаг остаётся `false` по умолчанию).
- Точки вызова (только TOP-LEVEL навигация; `<iframe>`-навигация в этот срез
  не входит — см. «Не покрыто» ниже): `click.rs` — все четыре ветки
  `<a href>` (`_blank`, именованная вкладка/фрейм, обычная), `form_submit.rs`
  — GET- и POST-ветки form-action, `about_to_wait.rs` —
  `js_navigate_to_gate`/`window_open_navigate_to_gate` (обе поменяли
  сигнатуру `Option<String>` → `Option<(String, bool)>`, второе поле — тот
  же флаг, вычисленный на месте из уже читаемого `policy` вместо повторного
  вызова гейта) и их три caller-а (`location.href=`/`.replace()`,
  `window.open()`).

`cargo test -p lumen-shell --profile dev-release --features v8 --bin lumen
-- csp` — 99 passed, `... frame navigate form_post_nav` — 136 passed,
`... click` — 22 passed (все без нового падения). `cargo test -p
lumen-network --profile dev-release --tests -- nav_post_body` — 2 passed.
`cargo clippy -p lumen-network -p lumen-shell --all-targets --features v8
-- -D warnings` — чисто.

Не покрыто: `<iframe>`-навигация (инициирующий документ там — дочерний,
`frames.rs`/`frame_links.rs` уже читают свой `csp_gate`, но не прокинуты в
этот срез — тот же список путей, что срезы 50-52 разделили на top-level и
frame), «upgrade insecure navigations set» как постоянное состояние
браузингового контекста (здесь заголовок решается заново на каждый вызов
из уже читаемой в момент клика/навигации политики — расхождение с
постоянным флагом возможно только если политика документа меняется между
объявлением и навигацией, что вне сферы этого среза). Остаток общего
списка дорожки не изменился: `report-to`, `manifest-src`, честная
per-policy `originalPolicy`.

## Срез 55 (2026-09-20, `p6-gap-cspenf-srez55`) — заголовок `Upgrade-Insecure-Requests: 1` на навигации `<iframe>`

Закрывает хвост, названный срезом 54: `<iframe>`/`<frame>`-навигация — сеть,
идущая мимо `HttpClient::fetch_page`, до этого среза никогда не несла
заголовок независимо от того, объявляла ли инициирующая политика
`upgrade-insecure-requests`.

- **`lumen-network::HttpClient::fetch_subresource_document`** (новый
  публичный метод) — `RequestDestination::Document` фикс + опциональный
  `send_uir_header: bool`, тем же приёмом среза 54 (слот
  `extra_request_headers` внутри `fetch_subresource_inner`, которая получила
  третий параметр `send_uir_header`). Существующие `fetch_subresource`/
  `fetch_subresource_with_content_type` не тронуты по контракту (зовут
  `fetch_subresource_inner` с `false`) — у них нет CSP-контекста, и раздувать
  их сигнатуру ради единственного вызывающего (`frames.rs`) было бы чужой
  болью для образов/скриптов/стилей/шрифтов.
- **`frames::fetch_iframe_source`** получил параметр `send_uir_header: bool`,
  доходящий до `fetch_subresource_document`. **`frames::spawn_frame`**
  получил параметр `uir_override: Option<bool>`: `None` — решает `csp_gate`
  ХОЗЯИНА целевого `<iframe>` (тот же `csp_gate`, что уже читает
  `maybe_upgrade_frame_src` срез 52) — корректно для первичной вставки,
  `<a target=имя_фрейма>` со страницы (`click.rs`) и переприсваивания `.src`
  скриптом (`frame_dynamic.rs`): во всех трёх хозяин цели и есть инициатор.
  `Some(flag)` — вызывающая сторона уже прочитала политику НАСТОЯЩЕГО
  инициатора, который с хозяином цели не совпадает: единственный сегодняшний
  случай — ссылка/форма ВНУТРИ самого фрейма (`frame_links.rs`), где `navigate-to`
  решает РЕБЁНОК, а не хозяин цели, той же причиной, что срез 53 уже разводит
  источники для апгрейда схемы (`resolve_and_upgrade_frame_href`).
  `run_frame_navigation`/`replace_frame_document`/`navigate_frame_to`
  прокидывают этот `Option<bool>` без интерпретации; `traverse_frame` (шаг
  истории) передаёт `None` — документ уже показывался раньше, повторная CSP-политика
  инициатора неприменима.
- **`frame_links.rs`** — все четыре точки навигации, которыми уже управляет
  `csp_gate` РЕБЁНКА (`navigate_frame_from_link` → `Some(uir)`;
  `navigate_page_from_frame`, обе ветки `NewWindow` — существующая вкладка и
  новая) получили `.with_uir_header(navigation_wants_uir_header(csp_gate))`/
  `Some(uir)` тем же способом, что срез 54 уже даёт top-level `<a>`/форме/
  `window.open()`.

`cargo test -p lumen-shell --profile dev-release --features v8 --bin lumen --
csp frame navigate form click` — 370 passed. `cargo test -p lumen-network
--profile dev-release --tests -- fetch_subresource` — 9 passed. `cargo
clippy -p lumen-network -p lumen-shell --all-targets --features v8 -- -D
warnings` — чисто. `scripts/scoped-test.sh` — один красный тест,
`lumen-driver::snapshot_cpu::cpu_snapshots_match_references`, тот же
7-файловый класс дрейфа, что и известный несвязанный
[BUG-1008](BUG-1008-OPEN.md) (`text-rendering`/`canvas-2d`/`list-markers`/
`forms`/`multiple-backgrounds`/`scrollbar-rendering`/`final`) — не
регрессия этого среза.

Живым пробой этот срез не проверен (нет готовой автоматизации клика по
ссылке внутри `<iframe>`/загрузки самого `<iframe>` под рукой в этой
сессии) — корректность опирается на: 1) идентичность приёма с уже живьём
подтверждённым срезом 54 (`send_uir_header`/`extra_request_headers` — тот же
код, только третий вызывающий); 2) полный зелёный прогон `csp`/`frame`/
`navigate`/`form`/`click` наборов; 3) существующие юнит-тесты на
`navigation_wants_uir_header`/`resolve_and_upgrade_frame_href`, которые эта
проводка переиспользует без изменений их собственной логики.

Попутно найден и заведён отдельным дефектом (не в скоупе этого среза):
[BUG-1067](BUG-1067-OPEN.md) — form GET-отправка ИЗ `<iframe>` с
`target="_top"`/`_parent` (`frame_form_submit.rs::frame_submit_navigate`,
ветка `LinkTarget::Page`) резолвит `action` без апгрейда схемы и без UIR-
заголовка вовсе, в отличие от соседней ветки `LinkTarget::Frame` той же
функции (она идёт через `spawn_frame` и оба получает автоматически).

Не покрыто: BUG-1067 (см. выше). «upgrade insecure navigations set» как
постоянное состояние браузингового контекста — тот же принятый компромисс,
что и у срезов 50-54 (заголовок решается заново на каждый вызов из уже
читаемой в момент навигации политики). Остаток общего списка дорожки не
изменился: `report-to`, `manifest-src`, честная per-policy `originalPolicy`.

## Срез 56 (2026-09-20, `p6-gap-cspenf-srez56`) — честная per-policy `originalPolicy`

Закрыл дрейф, который срез 40 (§3.4, независимые политики) сам оставил
открытым и который прошлый список «не покрыто» держал с тех пор: каждая
точка диспетчеризации `securitypolicyviolation` несла ЕГО объединённый
(`"; "`-joined) текст ВСЕХ политик документа как `originalPolicy`, даже когда
нарушила ровно одна — CSP3 §7.8 хочет текст именно нарушенной политики.

- **`lumen_network::csp::CspPolicy`** (`crates/network/src/csp.rs`) получил
  поле `raw: String` — сырой текст, из которого распарсена именно эта
  политика; `parse_csp_header`/`parse_csp_report_only_header` заполняют его
  из уже принятого параметра `header`, без нового прохода.
- **`csp_enforce.rs`** получил четыре `violating_*` функции
  (`violating_fetch_policy`, `violating_fetch_policy_via_child_src` —
  `frame-src`/`worker-src`-фолбэк, `violating_inline_policy`,
  `violating_base_uri_policy`): каждая ищет первую политику из `&[CspPolicy]`,
  которая ФАКТИЧЕСКИ нарушена данной проверкой, и возвращает `Some(&её.raw)`
  вместо `bool`. Старые `_blocked` функции с производственным вызовом только
  через один call site (`inline_script_blocked`, `script_src_blocked`,
  `frame_src_blocked`, `media_src_blocked`, `base_uri_blocked`) стали
  `#[cfg(test)]`-only — их предикат теперь целиком выражен через
  `violating_*`, а старые unit-тесты остались как регресс-проверка самого
  предиката.
- Каждый call site, что диспатчит `securitypolicyviolation` и имел под рукой
  `self_origin`/URL/тело в момент диспатча (или мог получить их дёшево),
  переключён на `violating_*` вместо `document_csp_policy`'s объединённого
  текста: `scripts.rs` (внешний и инлайновый `<script src>`/классический и
  модульный — `ResolvedScript.csp_blocked` сменил тип `bool` → `Option<String>`,
  захватывая текст ИМЕННО в `resolve_script_sources`, где `self_origin` ещё в
  скоупе — `run_scripts_with_dom` его не имеет), `frames.rs` (`frame-src` на
  навигации `<iframe>`, и img-src/style-src/font-src/bg-img-src в агрегате
  среза 25), `page_load.rs` (img-src в стриминговом/lazy пути, font-src),
  `page_pipeline.rs` (img-src, media-src — `<track>` захватывает текст сразу
  внутри `load_video_tracks`'s замыкания вместо счётчика URL, style-src,
  bg-img-src — `fetch_and_decode_background_images`/
  `subresources::fetch_and_decode_background_images` сменили `Vec<String>`
  заблокированных на `Vec<(String, String)>` с текстом политики), `base-uri`
  (`base_uri_href_blocked` теперь возвращает `(resolved, policy_text)` одним
  проходом вместо повторного `document_csp_policy` в точке диспатча).
- **Не покрыто этим срезом**: `blocked_inline_style_count`/
  `blocked_style_attr_nodes` (`page_pipeline.rs`, `frames.rs`, `relayout.rs`)
  — все три давно свернули список нарушений инлайновых `<style>`/атрибутов
  `style=""` в СЧЁТЧИК до диспетчеризации; тело конкретного блока/атрибута к
  моменту диспатча уже потеряно, поэтому они продолжают нести объединённый
  текст. Починка требует сначала пронести тела через `doc_extract`'s API
  (`walk_style_blocks`/`walk_style_attrs`/`collect_style_attr_csp_blocked`),
  не только счётчик — отдельный, более узкий срез. CSP3 §7.8 также хочет
  ОТДЕЛЬНЫЙ отчёт на КАЖДУЮ нарушенную политику при одновременном нарушении
  нескольких — этот срез даёт текст ПЕРВОЙ нарушившей, не список всех;
  одновременное нарушение одного ресурса несколькими политиками сразу
  встречается редко и остаётся отдельным пробелом.

`cargo test -p lumen-shell --profile dev-release --features v8 --bin lumen --
csp frame navigate form click` — 370 passed (без нового падения). `cargo test
-p lumen-network --profile dev-release --lib` — 2294 passed. `cargo clippy -p
lumen-network -p lumen-shell --all-targets --features v8 -- -D warnings` —
чисто. `scripts/scoped-test.sh` дважды дал разные, невоспроизводимые красные
(`lumen-driver::cases::snapshot_cpu::cpu_snapshots_match_references` — тот же
известный несвязанный класс дрейфа, что [BUG-1008](BUG-1008-OPEN.md); во
второй прогон вместо него — таймингово-чувствительные `lumen-js::
dom::tests::v8_webworker::*`/`v8_runtime::tests::dom_suspend_focus::
bounded_document_lock_waits_out_another_thread`, ни разу не тот же набор
между прогонами и ни один не в затронутых этим срезом файлах) — не
регрессия этого среза.

**Срез 57 (2026-09-20, `p6-gap-cspenf-srez57`)** закрыл остаток, названный
срезом 56 не покрытым: `blocked_inline_style_count`/`blocked_style_attr_nodes`
(`page_pipeline.rs`/`frames.rs`/`relayout.rs`) больше не сворачивают список
нарушений инлайновых `<style>`/атрибутов `style=""` в счётчик до
диспетчеризации — тело узла проверяется прямо в `doc_extract`'s обходе
(`extract_style_blocks`/`collect_style_attr_csp_blocked`), пока ещё в скоупе,
через новые `csp_enforce::violating_style_attr_policy` (аналог
`violating_inline_policy` для атрибута — своя цепочка фолбэка/`'unsafe-hashes'`
гейт) и уже существующий `violating_inline_policy`. Обе функции теперь
возвращают `Vec<String>` — текст ИМЕННО нарушенной политики на каждый
заблокированный узел, в порядке документа, вместо `usize`; поля
`PageCascade`/`FrameSubresourceOutcomes` переименованы в
`blocked_inline_style_policies`/`blocked_style_attr_policies`, каждый call
site диспетчеризации (`page_pipeline.rs`, `frames.rs`, `relayout.rs`) отдаёт
эту политику напрямую, а не объединённый текст `document_csp_policy`. Старые
`inline_style_blocked`/`style_attribute_blocked`/`inline_directive_blocked`
стали `#[cfg(test)]`-only (та же судьба, что срез 56 уже дал
`inline_script_blocked`/`script_src_blocked` — производственных вызовов не
осталось).

`cargo test -p lumen-shell --profile dev-release --features v8 --bin lumen --
csp frame navigate form click` — 370 passed (без нового падения, тот же
счётчик, что срез 56). `cargo clippy -p lumen-shell --all-targets --profile
dev-release --features v8 -- -D warnings` — чисто. `scripts/scoped-test.sh` дал
один красный (`lumen-driver::cases::snapshot_cpu::cpu_snapshots_match_references`,
7 файлов) — тот же известный несвязанный класс дрейфа, что
[BUG-1008](BUG-1008-OPEN.md); не регрессия этого среза (срез не трогает
paint/display list).

Остаток общего списка дорожки не изменился: `report-to` (нужны группы
эндпоинтов из `Report-To`, этот движок его не разбирает), `manifest-src`
(движок не фетчит веб-манифест вовсе — гейтить нечего), а также
многополитийный отчёт CSP3 §7.8 (текст первой нарушившей политики, не список
всех при одновременном нарушении несколькими).

## Срез 58 (2026-09-20, `p6-gap-cspenf-srez58`) — многополитийный отчёт CSP3 §7.8

Закрыл ровно тот пробел, что срез 56 сам назвал не покрытым: CSP3 §7.8 хочет
ОТДЕЛЬНЫЙ `securitypolicyviolation` на КАЖДУЮ нарушенную политику, когда один
и тот же ресурс нарушает несколько независимых политик документа одновременно
(CSP3 §3.4, независимые политики — срез 40). До этого среза все пять
`violating_*` функций `csp_enforce.rs` (`violating_fetch_policy`,
`violating_fetch_policy_via_child_src`, `violating_inline_policy`,
`violating_base_uri_policy`, `violating_style_attr_policy`) возвращали
`Option<&str>` — текст ПЕРВОЙ нарушившей политики (`.find(...).map(...)`).

- Все пять функций сменили `Option<&str>` на `Vec<&'a str>` — `.find(...)` →
  `.filter(...)`, пустой `Vec` — то же самое, что раньше `None`.
- Каждая из ~15 точек диспетчеризации (`doc_extract.rs`, `frames.rs`,
  `page_pipeline.rs`, `page_load.rs`, `scripts.rs`, `subresources.rs`)
  переключена с `if let Some(text) = violating_X(...) { fire_Y(..., text); }`
  на `for text in violating_X(...) { fire_Y(..., text); }`. Там, где у той же
  точки есть ОТДЕЛЬНОЕ решение «блокировать ли ресурс» (не единообразный
  паттерн — проверено индивидуально в каждом месте), блокировка осталась
  ОДНОКРАТНОЙ, только событий стало по одному на нарушенную политику:
  - `page_pipeline.rs::base_uri_href_blocked` сменил возврат с
    `Option<(String, String)>` на `Option<(String, Vec<String>)>` —
    `effective_base` (реальный блок) по-прежнему смотрит только на
    `.is_none()`, а точка диспатча в `parse_and_layout` теперь итерирует
    `policy_texts`.
  - `page_pipeline.rs`'s `load_video_tracks`-замыкание (`media-src` для
    `<track>`) собирает `Vec` нарушенных политик и пушит по паре
    `(gate_url, text)` в `blocked` на каждую, прежде чем однократно вернуть
    `None` (не фетчить).
  - `frames.rs`'s `frame_src_check` (`frame-src` для навигации `<iframe>`)
    собирает `Vec`, `is_empty()` решает «пускать ли» (`return None`), иначе
    диспатчит по одному событию на текст и один раз возвращает
    `Some(FetchError)`.
  - `page_load.rs`'s lazy-image и web-font пути — та же форма: собрать
    `Vec<String>` (владеющий, до `move`-замыкания в `route_task_js`),
    `is_empty()` решает «продолжать фетч или `continue`», иначе один
    `fire_image_error`/ничего лишнего плюс цикл по текстам.
  - `subresources.rs::fetch_and_decode_background_images` — `Err`-ветка
    замыкания `parallel_map` сменила тип с `(String, String)` на
    `(String, Vec<String>)`; внешний цикл, который разбирает `outcomes`,
    разворачивает это в несколько записей `(url.clone(), text)` — дальше по
    цепочке (`page_pipeline.rs`'s дисптач `blocked_by_bg_img_src`) уже ничего
    менять не пришлось, там и раньше был цикл по записям `Vec`.
  - `scripts.rs`'s `ResolvedScript::csp_blocked` сменил тип `Option<String>` →
    `Vec<String>` (`blocked_by_csp` конструктор принимает `Vec` вместо
    одного текста) — оба call site (классический и модульный
    `<script src>`) итерируют его при диспатче `error`+`securitypolicyviolation`.
  - `frames.rs`'s агрегатный блок среза 8/25 (`blocked_by_img_src`/
    `blocked_by_style_src`/`blocked_by_font_src`/`blocked_by_bg_img_src` —
    списки URL, где `violating_fetch_policy` пересчитывается заново в точке
    диспатча с фолбэком на `original_policy`) — фолбэк остался (на случай,
    если пересчёт ничего не находит), но при непустом результате теперь
    цикл по каждому тексту, а не один `.unwrap_or(...)`.
- `doc_extract.rs`'s `walk_style_blocks`/`walk_style_attrs` (инлайновые
  `<style>`/`style=""`, срез 57) теперь пушат по одной записи на каждую
  нарушенную политику того же узла в `blocked`/`policies`, вместо одной — сам
  диспатч в `page_pipeline.rs`/`frames.rs` не менялся, он уже был циклом по
  `Vec`.
- Обновлены doc-комментарии, явно называвшие многополитийный отчёт «ещё не
  сделано»: модульный doc-комментарий `csp_enforce.rs` (был "CSP3 §7.8 также
  хочет ОТДЕЛЬНЫЙ отчёт... остаётся отдельным пробелом") и doc-комментарий
  `document_csp_policy` (был "that distinction is still open").
- Добавлены юнит-тесты на все пять `violating_*` функций с ДВУМЯ
  одновременными политиками: случай, когда обе блокируют (возвращается
  `Vec` длины 2), и случай, когда блокирует только одна (возвращается `Vec`
  длины 1 с текстом именно этой политики).

`cargo test -p lumen-shell --profile dev-release --features v8 --bin lumen --
csp frame navigate form click` — 380 passed (370 из среза 57 + 10 новых
многополитийных тестов, без нового падения). `cargo clippy -p lumen-shell
--all-targets --profile dev-release --features v8 -- -D warnings` — чисто.

Остаток общего списка дорожки не изменился: `report-to` (нужны группы
эндпоинтов из `Report-To`, этот движок его не разбирает), `manifest-src`
(движок не фетчит веб-манифест вовсе — гейтить нечего).

**Срез 59 (2026-09-20, `p6-gap-cspenf-srez59`):** первый шаг к `report-to` —
разбор заголовка `Report-To` (Reporting API v0) и доставка групп эндпоинтов
до `Document`. `report-to <group>` в самой CSP-директиве несёт только имя
группы, а адреса, на которые слать отчёт, приходят отдельным заголовком
ответа (в отличие от `report-uri`, чей URL сидит прямо в тексте CSP-заголовка
— поэтому срез 14 смог обойтись без расширения границы Rust/JS). Новая
`page_source::report_to_endpoints` разбирает каждый экземпляр `Report-To` в
`{имя группы -> список URL}`; повтор с тем же именем группы заменяет список
целиком (у Reporting API v0 нет определённого правила слияния, в отличие от
RFC 8941 Structured Fields). Карта прокинута тем же путём, что и
`csp_header`, — `RawPage` → `render_bytes`/`parse_and_layout` →
`Document::report_to_endpoints`, доступна из любой точки enforcement, у
которой есть только `&Document`. 11 новых юнит-тестов на
`report_to_endpoints`/`parse_report_to_header` (именованная группа,
дефолтная группа `"default"`, несколько URL в одной группе, регистронезависимое
имя заголовка, отсутствие заголовка, битый JSON, пустой/отсутствующий
`endpoints`, эндпоинт без `url`, повтор той же группы «последний побеждает»,
две разные группы одновременно).

Не покрыто этим срезом: сама доставка отчётов на эти группы — JS-часть
среза 14 (`_lumen_send_csp_reports` в `crates/js/src/csp.rs`) всё ещё
реэкстрагирует только `report-uri` из `originalPolicy` и не знает про эту
карту. Остаток общего списка дорожки не изменился: `manifest-src`.

**Срез 60 (2026-09-20, `p6-gap-cspenf-srez60`):** доставка отчётов на группы
эндпоинтов `report-to` — второй шаг после среза 59. В отличие от
`report-uri`, чей список URL сидит прямо в тексте CSP-директивы, `report-to
<group>` несёт только ИМЯ группы: адреса резолвятся через карту
`Document::report_to_endpoints`, которую срез 59 уже наполняет из
`Report-To`. Расширять сигнатуру каждой из полутора десятков точек
`fire_*_violation`/`_lumen_dispatch_csp_violation` шестым аргументом (как
`originalPolicy` уже прокинут пятым) означало бы задеть весь список срезов
1-59 разом — вместо этого новая нативная привязка
`_lumen_get_report_to_endpoints_json` (`install_document_meta`,
`crates/js/src/v8_runtime/install/dom_core.rs`) читает
`doc.report_to_endpoints()` через уже существующий `Arc<Mutex<Document>>` у
`install_dom` и отдаёт JSON одной строкой; `_lumen_send_csp_reports`
(`crates/js/src/csp.rs`) зовёт её лениво, по одному разу на нарушение,
только когда в политике вообще есть `report-to`. Разбор группы —
`/report-to\s+(\S+)/i` (в отличие от `report-uri`, у директивы `report-to`
может быть только ОДНО имя группы — CSP3 §3.1/Reporting API v0, не список
через пробел); `report-uri` и `report-to` в одной политике доставляются оба,
независимо друг от друга. Неизвестное имя группы (нет в карте, либо карта
пуста — заголовка `Report-To` не было) молча не шлёт ничего, тем же
принципом «нет цели — нет отчёта», что уже был у пустого `report-uri`.

4 новых юнит-теста в `crates/js/src/csp.rs`: доставка на именованную группу,
неизвестная группа не шлёт ничего, `report-uri`+`report-to` вместе шлют оба.
`cargo test -p lumen-js --profile dev-release --features v8-backend -- csp`
— 18 passed (14 из среза 14 + 4 новых). `cargo clippy -p lumen-js
--all-targets --profile dev-release --features v8-backend -- -D warnings` и
`cargo clippy -p lumen-shell --all-targets --profile dev-release --features
v8 -- -D warnings` — чисто.

Остаток общего списка дорожки не изменился: `manifest-src`.

**Срез 61 (2026-09-20, `p6-gap-cspenf-srez61`) — финал, дорожка закрыта:**
разведка `manifest-src` (последний пункт списка) подтвердила диагноз,
записанный ещё в срезе, давшем строки 1575-1577: тип-система готова —
`CspDirective::ManifestSrc` (`crates/network/src/csp.rs:118`) разбирается
(`parse_into`, `crates/network/src/csp.rs:496`) и честно участвует в
`effective_sources`/`fetch_directive_allows` с обычным фолбэком на
`default-src`. Гейтить нечего не из-за CSP-кода, а из-за отсутствия
producer'а: `<link rel="manifest">` нигде не парсится (grep по `crates/dom`,
`crates/loader`, `crates/network`, `crates/shell` — ноль совпадений;
единственное "manifest" в шелле — `crates/shell/src/update.rs:261`
`MANIFEST_URL`, self-update приложения, к Web App Manifest отношения не
имеет), и JSON манифеста никогда не фетчится. `crates/storage/src/web_manifest.rs`
(таблица `web_manifests`) существует, но `WebManifests::` не вызывается
нигде за пределами своего файла — мёртвая заготовка под будущий
PWA-install UX. WPT-категория `appmanifest` целиком ручная по дизайну
спеки (`WPT-VENDOR-appmanifest` в ROADMAP.md, DONE 2026-07-24) — все тесты
`-manual(.tentative).html`, автоматизированного WPT-давления на реализацию
Web App Manifest нет и не будет. Это переводит `manifest-src` из
«доработка в один срез по образцу script-src/img-src» (директива
распознана, не хватает `_blocked`-гейта на существующем producer'е) в
«блокировано отсутствием целой фичи вне CSP» — тот же принцип, что
нереализованная функциональность не дефект CSP-дорожки
(`feedback_feature_gap_is_not_a_bug`). Web App Manifest fetch (парсинг
`<link rel=manifest>`, сетевой запрос, заполнение уже существующей
`web_manifests`) отдельной строкой ROADMAP.md не заводится: единственный
потребитель — сам гейт `manifest-src`, автоматизированного теста нет,
приоритета без PWA-install UX не появилось; если он материализуется,
`manifest_src_blocked`/`violating_fetch_policy(..., &CspDirective::ManifestSrc, ...)`
добавляется по образцу `font_src_blocked` (`crates/shell/src/csp_enforce.rs:629`)
без затрагивания CSP-кода.

**Итог дорожки:** GAP-CSPENF закрывается срезом 61. Все директивы с
реальным producer'ом в движке (`script-src`, `style-src`, `img-src`,
`connect-src`, `frame-src`/`frame-ancestors`, `worker-src`, `font-src`,
`object-src`, `media-src`, `default-src`, `trusted-types`,
`require-trusted-types-for`) гейтятся и диспатчат
`securitypolicyviolation`/шлют `report-uri`+`report-to`; `manifest-src`
остаётся инертной директивой не по недоработке CSP, а потому что ресурс,
который она должна фильтровать, в движке не существует.
