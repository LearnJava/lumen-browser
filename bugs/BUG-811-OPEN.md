# BUG-811 — CSP разбирается, но не применяется: ни одна директива ничего не блокирует и событие `securitypolicyviolation` не диспатчится никогда

**Статус:** OPEN
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
