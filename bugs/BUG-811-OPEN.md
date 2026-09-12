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
