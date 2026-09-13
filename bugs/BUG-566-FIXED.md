# BUG-566: `<meta http-equiv="refresh">` is entirely unimplemented — no timed navigation ever fires

**Статус:** FIXED 2026-09-13
**Дата:** 2026-08-04
**Компонент:** engine (no owner yet — reflects as a DOM attribute in the JS
shim, `crates/js/src/dom.rs:13882` (`['httpEquiv', 'http-equiv', 'string']`),
but nothing consumes the `refresh` pragma to schedule a navigation; candidate
homes are `crates/engine/html-parser/src/` (parse-time pragma processing,
matching how `http-equiv="Content-Type"` already drives charset detection in
`crates/engine/encoding/src/detect.rs`) or the shell's navigation/timer
plumbing)
**Найден:** P2, WPT-VENDOR-html-semantics-document-metadata, 2026-08-04

## Симптом

`<meta http-equiv="refresh" content="N; url=...">` never navigates the page,
whether present at parse time or inserted/mutated dynamically. Every test in
`html/semantics/document-metadata/the-meta-element/pragma-directives/
attr-meta-http-equiv-refresh/` that waits on the resulting navigation times
out (`Test TIMEOUT, expected OK`) — `allow-scripts-flag-changing-1.html`,
`allow-scripts-flag-changing-2.html`, `dynamic-append.html`,
`not-in-shadow-tree.html`, `remove-from-document.html`.

## Причина

Confirmed by direct source read, not inferred from the WPT run alone (the
run's own signal is muddied by two unrelated infra gaps — see Оговорка
below): `http-equiv` is registered only as a plain reflected string IDL
attribute (`dom.rs:13882`), and a repo-wide grep for any pragma-directive
processing of the `refresh` value finds nothing —

```
grep -rn "http.equiv|HttpEquiv|pragma" crates/engine/html-parser/src/*.rs crates/shell/src/*.rs   # zero hits
grep -rln '"refresh"' crates/engine/ crates/js/                                                    # zero hits
```

— compare to `http-equiv="Content-Type"`, which *is* consumed pragmatically
(drives charset detection, `crates/engine/encoding/src/detect.rs`). No
navigation timer is ever armed from the `content` attribute's `time; url=`
syntax, whether the `<meta>` is present in the initial parse, appended later,
or its `content`/`http-equiv` attributes are mutated after the fact — there
is no code path that would do any of the three.

## Оговорка про WPT-сигнал

The five directly executing tests above don't cleanly isolate this defect on
their own: `dynamic-append.html`/`not-in-shadow-tree.html`/
`allow-scripts-flag-changing-*.html` route through `<iframe>` +
`contentDocument`/`onload`, which never fires because `<iframe>` has no real
browsing context (the pre-existing, documented `<iframe>` limitation,
BUG-381/383's class) — so their TIMEOUT would reproduce even if refresh
navigation worked. The 14 `parsing.html?N-M` variants (parse-only, no
`<iframe>`) time out for an unrelated reason: they load
`/common/subset-tests.js`, which is not vendored (`ls tests/wpt/common/` has
no `subset-tests.js`) → `network error: HTTP 404` →
`ReferenceError: subsetTest is not defined`, an infra gap of the same class
as other categories' missing `/common/*.js` helpers, not a Lumen bug. The
finding here rests on the source-code absence, per the backlog's "probe even
without signal" rule (see `eyedropper`/`fedcm` precedent in
[[reference_wpt_run_report_invocation_recipe]]) — a real `refresh` engine
would need its own dedicated exercise (e.g. `--dump-layout` on a
minimal `<meta http-equiv=refresh content="0;url=...">` page, or vendoring
`/common/subset-tests.js` to unblock `parsing.html`) to get a clean pass/fail
per sub-case, which is out of scope for this triage pass.

## Исправление

**Срез P3 2026-09-13:** реализована статическая (parse-time) ветка спецификации
HTML LS §4.2.5.3 "shared declarative refresh steps":

- `parse_meta_refresh()` (`crates/engine/html-parser/src/tree_builder.rs`) —
  парсит `content="N[.frac]; url=..."`/`"N, url"`/голое `"N"` по грамматике
  спеки (ведущие пробелы, целые секунды, отбрасываемая дробная часть,
  разделитель `;`/`,`, опциональные `url`/`=`, значение в кавычках или без).
  Вызывается из того же хука в `create_element_with_attrs`, что и
  `parse_viewport_meta` — только первое вхождение в порядке документа
  побеждает (`Document::set_meta_refresh` — no-op, если поле уже занято),
  результат хранится в новом `Document::meta_refresh` /
  `lumen_dom::MetaRefresh { delay_seconds, url }`.
- `crates/shell/src/scripts.rs` — при создании JS-рантайма страницы (до
  исполнения первого скрипта, тот же one-shot-push, что у CSP/opener/window.name
  чуть выше) читает `doc.meta_refresh()` и, если задан, инжектирует
  `setTimeout(function() { location.replace(<url>)|location.reload(); },
  N*1000)`. Переиспользует уже существующий и протестированный маршрут
  `location.replace`/`.reload` (`_lumen_navigate_or_fragment`,
  `web_api_shim_mid_b.js`) вместо нового таймерного механизма в шелле — JS
  здесь используется как единственный «тикающий» движок процесса (`about_to_wait.rs`
  зовёт `tick_timers()` на одном живом `js_ctx`), так что отдельный
  shell-side таймер не нужен.

**Не покрыто этим срезом:** динамическая мутация — `<meta http-equiv=refresh>`,
вставленный/изменённый скриптом уже после первой раскладки, не перехватывается
(нет "attribute changed"-хука в Rust; такой путь потребовал бы отдельной
логики в JS-шиме на `setAttribute`/`appendChild`). Ни один из пяти
WPT-тестов, перечисленных в «Симптом», от этого не зеленеет — все пять либо
идут через `<iframe>`+`contentDocument` (блокировано отдельным, уже
задокументированным гэпом отсутствующего browsing context у `<iframe>`,
класс BUG-381/383), либо через невендоренный `/common/subset-tests.js` — оба
ограничения не связаны с этим багом (см. «Оговорка про WPT-сигнал» выше).
Живой WPT-прогон поэтому не проводился; статус основан на 7 юнит-тестах
парсера (`crates/engine/html-parser/src/tree_builder.rs::tests::meta_refresh_*`,
`cargo test -p lumen-html-parser` зелёный) и разборе кода инжекции в
`scripts.rs`, которая переиспользует уже покрытые тестами примитивы
(`location.replace`, `setTimeout`). `cargo clippy -p lumen-dom -p
lumen-html-parser --all-targets -- -D warnings` чист (`lumen-shell` не
проверяется отдельно — общий workspace-clippy гейт сломан
[BUG-1053](BUG-1053-OPEN.md), не связан с этим срезом, подтверждено то же
падение на `main` без моих изменений).
