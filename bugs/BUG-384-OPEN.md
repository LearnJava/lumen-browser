# BUG-384 — named access on Window не реализован: `id="x"` не даёт ни `window.x`, ни голого `x`, и `'x' in window` === false

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM`: именованные свойства
на глобале не заводятся ни при парсинге документа, ни при `appendChild`;
`window` при этом уже является настоящим глобальным объектом —
`globalThis === window` истинно, [BUG-280](BUG-280-FIXED.md) исправлен)
**Найден:** P2, WPT-VENDOR-focus (2026-07-28), тесты `focus/focus-double-sync-calls.html`,
`focus/focus-sync-when-blur.html`, `focus/nested-focus-within-iframe-focus-event.html`
+ проба `--dump-layout` (`.tmp/probe-named.html`)

## Симптом

Документ содержит `<input id="input1">` и `<div id="named2">`:

```
window.input1                              = undefined
input1            (голым идентификатором)  = undefined  (в strict-режиме — ReferenceError)
window.named2                              = undefined
named2                                     = undefined
Object.prototype.hasOwnProperty.call(window,'input1') = false
'input1' in window                         = false
globalThis === window                      = true
globalThis.input1                          = undefined
```

Последние три строки важны: свойства нет **вовсе** — это не сломанный геттер и
не расхождение `window` с реальным глобалом (эта причина была бы естественной
гипотезой, но `globalThis === window` истинно с момента исправления BUG-280).

В прогоне WPT баг убивает 3 из 7 тестов категории, которые вообще доходят до
исполнения:

```
FAIL Element.focus() in focus listener when focus has moved away - input1 is not defined
FAIL Element.focus() in blur listener when focus has moved away - input1 is not defined
FAIL dialog.focus() in navigable's focus handler          - iframe is not defined
```

Все три написаны в обычном для WPT стиле — элемент размечен с `id`, скрипт
обращается к нему голым именем, без `getElementById`.

## Причина

HTML LS §7.3.3 (`Window` named properties object) требует, чтобы объект
именованных свойств `Window` отдавал элементы с подходящим `id`/`name`
(`<img name>`, `<form name>`, `<iframe name>`, `<embed>`, `<object>`) как
свойства глобала, разрешая имя после обычных свойств `Window` и переменных
скрипта. В шиме такого слоя нет: глобал — обычный объект, наполняемый только
самим шимом и `var`-объявлениями страницы.

Гэп был замечен и записан походя в [BUG-360](BUG-360-OPEN.md) («Тем же тестом
вскрыт соседний гэп: named access on Window»), но не заведён отдельно; причина
у него другая (не диспатч событий, а разрешение имён на глобале), и чинится он
в другом месте, поэтому выделен сюда.

## Влияние

* Реальные страницы: приём `<div id="app">` + голое `app` в скрипте — древний,
  но живой (особенно в разметке без сборщиков, в рекламных вставках и в коде,
  сгенерированном визуальными редакторами). Такой скрипт падает на первой же
  строке с `ReferenceError`, унося с собой всё остальное на странице.
* Тестовые наборы: WPT пользуется этим приёмом систематически, поэтому баг
  бьёт по многим категориям сразу и **маскирует** настоящие находки — тест
  умирает до своего утверждения. В категории `focus` из-за него не измерены три
  сценария, которые иначе показали бы BUG-381.
* Диагностика: `'x' in window === false` означает, что и защитная проверка
  `if (window.x)` не спасает — код просто идёт по ветке «элемента нет».

## Как чинить

Правильная реализация — Proxy/interceptor на глобальном объекте, разрешающий
имя в порядке: собственные свойства `Window` → переменные скрипта → именованные
элементы документа. У V8 для этого есть штатный механизм
(`NamedPropertyHandlerConfiguration` на шаблоне глобального объекта), поэтому
чинить следует в `v8_runtime.rs`, а не строкой в `WEB_API_SHIM`; ветка QuickJS
— по остаточному принципу (ADR-018: новые силы в rquickjs не вкладываем).

Дешёвый промежуточный вариант — заводить свойство глобала при парсинге на
каждый элемент с `id`/`name` и снимать при удалении из документа — даёт
правильный результат на 95% страниц, но расходится со спекой в порядке
разрешения (переменная скрипта должна побеждать элемент) и требует аккуратной
инвалидации; годится как срез, если полноценный interceptor дорог.

Регрессия проверяется без WPT: страница с `<div id="probe">` и утверждением
`typeof probe === 'object' && 'probe' in window`.

## Связанные

* [BUG-360](BUG-360-OPEN.md) — где этот гэп был впервые замечен как побочный.
* [BUG-280](BUG-280-FIXED.md) — `window` уже стал настоящим глобалом; этот баг
  — следующий слой той же темы.
* [BUG-381](BUG-381-OPEN.md) — измерение которого этот баг закрывает на трёх
  тестах категории `focus`.

**WPT-RUN-3 срез 9 (`css/css-backgrounds`, 2026-08-02):** ещё один harness
TIMEOUT-кейс того же класса — `inheritance.sub.html` объявляет
`<div id="container">` и синхронно читает `container.style.color = ...` как
голый идентификатор; скрипт падает на первой же ссылке до регистрации
любого `test()`, харнес зависает с нулём сабтестов. `.ini`:
`tests/wpt/metadata/css/css-backgrounds/inheritance.sub.html.ini`
(`expected: TIMEOUT`).

**WPT-RUN-3 срез 10 (`css/css-variables`, 2026-08-02):** пять новых файлов,
тот же механизм — голый идентификатор совпадает с `id` элемента,
объявленного через `<tag id=x>`:
`css-variable-change-style-001.html`/`-002.html` (`outer` — `<div id="outer">`),
`revert-layer-in-fallback.html` (`child` — `<div id=child>`),
`variable-cycles.html` (`main` — `<main id=main></main>`),
`variables-substitute-guaranteed-invalid.html` (`target1`), `var-ident-function.html`
(`target`). Two of these mask a *second* bug in a way worth noting for
whoever unblocks BUG-384 next: `variable-cycles.html`'s underlying
assertions (`getComputedStyle(element).getPropertyValue('--sanity')` etc.)
all read custom properties, so once BUG-384 is fixed these tests will still
fail via [BUG-499](BUG-499-OPEN.md) (custom properties never reach
`getComputedStyle`), not pass outright; conversely
`variables-substitute-guaranteed-invalid.html`'s three subtests all expect
`""` for a guaranteed-invalid custom property, which BUG-499 already
produces unconditionally — so those three would *pass by coincidence* the
moment BUG-384 is fixed, without the engine actually having detected
cycle/reference invalidity. Unlike `inheritance.sub.html` (bare reference at
top level, harness-wide TIMEOUT), all five files here reference the bare
identifier *inside* a `test()` callback, so `testharness.js`'s per-test
try/catch contains the `ReferenceError` to that one subtest — harness status
is `OK` with the affected subtests individually `FAIL`, not a file-wide
TIMEOUT. `.ini` for all five: `expected: FAIL` per affected subtest.

**WPT-RUN-3 срез 11 (`css/css-overflow`, 2026-08-02):** 18 files, same bare
`id`-as-global-identifier pattern (`ref`, `div`, `container`, `container1`,
`container2`, `target`, `scroller`, `horizontal`, `vertical`), all inside
`test()`/`promise_test()` callbacks so each is an isolated `FAIL`, not a
harness-wide `TIMEOUT`. `.ini`: `expected: FAIL` per affected subtest.

**WPT-RUN-3 срез 12 (`css/css-logical`, 2026-08-02):** back to the
harness-wide TIMEOUT shape (like `inheritance.sub.html` in срез 9) —
`inheritance.html` reads `getComputedStyle(reference).borderTopWidth` as a
bare top-level statement (line 45, before any `test()` call), referencing
the markup element `<div id="reference">` by bare identifier;
`ReferenceError` aborts the whole script before a single subtest
registers. `.ini`: `tests/wpt/metadata/css/css-logical/inheritance.html.ini`
(`expected: TIMEOUT`).

**WPT-RUN-3 срез 16 (`css/css-forms`, 2026-08-02):** 3 files, the bare
identifier referenced *inside* a `test()` callback (each an isolated `FAIL`,
harness stays `OK`) — `checkbox-checkmark-animation.html` and
`radio-checkmark-animation.html` both write `#checkbox`/`#radio` in the
markup then reference the bare `checkbox`/`radio` identifier synchronously
inside their single `test()` body (`ReferenceError: checkbox/radio is not
defined`). `input-text-base-appearance-computed-style.html` is a distinct
variant worth flagging: the test source itself has an upstream typo —
`const intial = document.getElementById('initial');` (missing the second
`i`) declares an unused, misspelled local, while the function below
(`testProperties`) references the *correctly*-spelled `initial` as a bare
identifier, relying entirely on named access to resolve it (real browsers
paper over the typo via this mechanism; Lumen has no such fallback, so the
typo is fully exposed as `ReferenceError: initial is not defined`). `.ini`
under `tests/wpt/metadata/css/css-forms/` for all 3 files, `expected: FAIL`
per affected subtest.

## Срез 19 (`css/css-nesting`, 2026-08-03)

6 files, 13 subtests — the largest single-slice cluster of this bug so far
outside `css-overflow`. All follow the same shape: a `<div id=target>` /
`<style id=style>` / `<div id=p>` referenced by its bare identifier inside a
top-level `<script>` or a `test()` body, with no `document.getElementById`
anywhere in the file. `block-skipping.html` is notable because it's testing
an unrelated engine bug (`https://crbug.com/362674384`, "block ignored after
lack of whitespace" in an external stylesheet) that this bug fully masks —
the test never gets far enough to exercise its own subject.
`set-selector-text.html` is the interesting case: the file-level status is
`ERROR` (not `FAIL`), because `t.add_cleanup(() => { style.textContent = ''
})` *also* references the same bare `style` identifier and throws in
cleanup — wptrunner reports `Test named '...' specified 1 'cleanup' function,
and 1 failed`, and the parameterized loop's remaining 6 `test()` calls never
run at all (`NOTRUN`, not `FAIL`) once the first one's exception propagates.
`.ini` under `tests/wpt/metadata/css/css-nesting/` for all 6 files.

## Срез 21 (`css/css-device-adapt` + `css/css-env` + `css/css-overscroll-behavior`, 2026-08-03)

3 files, 4 subtests, all inside a `test()`/`promise_test()` callback (isolated
`FAIL`, harness stays `OK`):
`css-device-adapt/documentElement-clientWidth-on-minimum-scale-size.tentative.html`
(`<div id="reference">`, 1 subtest); `css-env/env-in-custom-properties.tentative.html`
(`<div id=child>`, 2 subtests); `css-overscroll-behavior/overscroll-behavior-keyboard-scroll-child-frame.html`
(`<iframe id="iframe">`, 1 subtest — the file's 5 sibling tests in this
category instead die on an unvendored `testdriver-vendor.js` helper
(`waitForCompositorReady`/`waitForCompositorCommit` not defined anywhere in
vendored resources — infra gap, not attributed, see `docs/wpt-status.md` →
`css` row), so this is the only one of the six that reaches far enough to hit
BUG-384 instead). `.ini` under
`tests/wpt/metadata/css/css-device-adapt/`, `tests/wpt/metadata/css/css-env/`,
`tests/wpt/metadata/css/css-overscroll-behavior/` for all 3 files, `expected:
FAIL` per affected subtest.

## Срез 22 (`css/css-mixins` + `css/css-color-adjust`, 2026-08-03)

~55 subtests across 12 files, largest cluster of this bug in the track so
far. `css-mixins`: `mixin-layers.html` (`e1`/`e2`/`e3`/`e4`, 4),
`function-container-dynamic/-self/-style.html` (bare `target`, 4),
`function-in-media.html`/`function-media-dynamic.html` (bare `iframe`
inside a rejected promise, 2), `function-invalidation.html` (bare
`mutable_style`, 4), `function-shadow-cache.html` (bare `host1`, 1),
`mixins/apply-nested-declarations.html`/`contents-nested-declarations(-
fallback).html` (bare `e1`/`e2`, 6), `functions/function-attr.html` — same
shape as `set-selector-text.html` in срез 19: the first `test()`'s cleanup
references bare `main`, so `t.add_cleanup` throws, wptrunner reports the
file `ERROR` instead of `FAIL`, and the remaining 17 parameterized `test()`
calls never run (`NOTRUN`). `css-color-adjust`: bare `dark`/`light` (2,
`color-scheme-color-initial-affected-by-color-scheme-property.html`), bare
`meta` (2, `…-affected-by-meta-color-scheme-dynamic.html`), bare `frm` (1
`FAIL` + `ERROR` file status on `color-scheme-iframe-dynamic.html`'s own
cleanup, same masking shape as `function-attr.html` above, 3 subtests
NOTRUN as a result; 1 more `FAIL` on `…-preferred-change-same-origin.html`),
bare `main` (12, `color-scheme-iframe-preferred-nested.sub.html` — every
subtest in the file), bare `light` again (1,
`color-scheme-system-colors.html`). `.ini` under
`tests/wpt/metadata/css/css-mixins/` and
`tests/wpt/metadata/css/css-color-adjust/` for all 12 files.

## Срез 24 (`css/css-scroll-anchoring` + `css/css-content`, 2026-08-03)

13 more subtests: `css-scroll-anchoring` (12 subtests/2 files — the
`contenteditable-near-cursor.tentative.html` `editable`/
`scroll-padding-affects-anchoring.html` `scroller` bare-id cases, plus the
`scroll-anchoring-editing-content-change.html` cluster where the bare-id
`anchor`/`first` ReferenceError inside a `t.add_cleanup` callback turns the
whole file `ERROR` and NOTRUNs its two remaining parameterized cases — same
masking shape as `set-selector-text.html` (срез 19) and
`function-attr.html` (срез 22)) and `css-content`
(`element-replacement-dynamic-002.html`'s bare-id `wrapper`, 1 subtest).
`.ini` under `tests/wpt/metadata/css/css-scroll-anchoring/` and
`tests/wpt/metadata/css/css-content/`.

## Срез 26 (`css/css-ruby` + `css/css-page` + `css/css-highlight-api`, 2026-08-03)

`css-ruby/line-break-around-ruby-001.html` — bare-id `container` referenced
inside `runTests()`, itself called at top level before any `test()`
registers (`getComputedStyle(container.firstElementChild)` throws first) —
TIMEOUT, 1 file.

`css-page/parsing/margin-rules-002.html` — four `<style id="...">` elements
(`nothing`/`propertiesAfter`/`propertiesBefore`/`propertiesBetween`)
referenced as bare identifiers, 4 subtests;
`parsing/page-orientation-computed.tentative.html` — bare-id `elm`, 1
subtest. `css-highlight-api/highlight-pseudo-computed.html` — bare-id
`target`/`h1`/`h2` (12 subtests, the file's entire failure set — the
`::highlight()` pseudo-element computed-style logic these tests actually
target is never reached); `highlight-pseudo-from-font-computed.html` — same
bare-id pattern (`h1`) but hit at **top level** (`r1.setStart(h1, 0)` before
the first `test()`), so this one is TIMEOUT rather than FAIL. `.ini` under
`tests/wpt/metadata/css/css-page/` and
`tests/wpt/metadata/css/css-highlight-api/`.

## Срез 27 (`css/css-transitions`, 2026-08-03)

Ten files, one bare-id subtest each (`box`/`outer`/`target`/`div`/`inner`
depending on the file's own markup): `after-change-style-inherited-try
-fallback.html`, `display-none-no-animations.html`, `inherit-height
-transition.html`, `pseudo-elements-002.html`, `retargetted-transition-with
-box-sizing.html`, `starting-style-name-defining-rules.html` (1 of its 2
subtests — the other is BUG-536), `transition-base-response-002.html`,
`transition-important.html`, `transition-reparented.html`,
`transitions-retarget.html`. `.ini` under
`tests/wpt/metadata/css/css-transitions/`.

## Срез 29 (`css/css-shadow` + `css/css-scroll-snap` + `css/css-animations`, 2026-08-03)

~85 subtests (plus a large share of the 216 `promise_test: Unhandled
rejection with value: object "ReferenceError: … is not defined"` cluster —
the same mechanism wrapped by a `promise_test`'s rejection handler instead
of surfacing as a synchronous throw). `css-shadow`: `host` (shadow host
`<div id="host">`, the single most common id in this category —
`font-face-*.html`, `host-dom-001.html`,
`host-functional-descendant-invalidation.html`, and others), same masking
shape as prior slices — the test's *actual* subject (`::part`, `:host()`,
`@font-face` in a shadow tree) is never reached. `css-scroll-snap`: `target`
(`multicol-002.html`, both fragment-geometry subtests) and `scroller`
(`ignore-snap-points-orthogonal-to-snap-axis.html`,
`no-snap-position.html`). `css-animations`: `test_sheet` (a `<style
id=test_sheet>` referenced bare in `parsing/keyframe-selectors.html`'s 14
selector-validity subtests) and `target`/`target1`/`target2`-shaped ids in
`animation-base-response-00{2,4}.html`/`computed-style-animation-parsing
.html`. `.ini` under `tests/wpt/metadata/css/css-shadow/`,
`tests/wpt/metadata/css/css-scroll-snap/`,
`tests/wpt/metadata/css/css-animations/`.
