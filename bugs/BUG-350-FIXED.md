# BUG-350 — `<script type="module">` silently degrades to classic `eval()` on the default V8 build: no module semantics at all

**Статус:** FIXED 2026-07-29
**Компонент:** js (`crates/js/src/v8_runtime.rs`, `crates/core/src/ext.rs::JsRuntime::eval_module`)
**Найден:** P2, WPT-VENDOR-device-bound-session-credentials (2026-07-26), while triaging why the vendored category's sole non-`.https.` test (`not-secure-connection.html`) TIMEOUT-ed: it loads `helper.js` via both `<script src="helper.js" type="module">` and an inline `<script type="module">` doing `import {...} from "./helper.js"`, and the browser log showed `module error: JS runtime error: Unexpected token 'export'` / `Cannot use import statement outside a module`.

## Симптом

Any `<script type="module">` — external or inline, with or without imports, even a
single module with zero `import`/`export` statements — fails with a V8 classic-script
parse error the moment its body contains top-level `export`/`import` syntax. Confirmed:

1. `not-secure-connection.html`'s module scripts throw `Unexpected token 'export'`
   (from `helper.js`'s top-level `export function ...` declarations) and
   `Cannot use import statement outside a module` (from the inline module's
   `import { ... } from "./helper.js"`), then the test harness never registers a
   result — the page hangs until wptrunner's TIMEOUT fires.
2. A repo-wide grep finds 80 vendored `tests/wpt/**/*.html` files using
   `type="module"` — every one of them is equally broken on the shipping engine.
3. `graphic_tests/` has zero `type="module"` coverage, which is why this has never
   been caught by the graphic-test gate.

## Причина

`crates/core/src/ext.rs:868`, the `JsRuntime` trait's default `eval_module`:

```rust
fn eval_module(&self, source: &str) -> JsResult<()> {
    self.eval(source).map(|_| ())
}
```

`crates/js/src/v8_runtime.rs`'s `impl JsRuntime for V8JsRuntime` (declared at line 4281)
never overrides `eval_module` or `register_module_source` — confirmed by grep, no
matches in the file. So every `<script type="module">` body, external or inline, is
routed through `V8JsRuntime::eval()` — plain classic-script evaluation — which
rejects top-level `export`/`import` outright. `crates/shell/src/main.rs`'s script
classification (`collect_scripts_ordered`/`is_classic_script_type`, ~line 6613-6688)
correctly buckets `type="module"` scripts into the `modules` list, and the V8 module
execution loop (`run_scripts_with_dom`, line 7074) does call `rt.eval_module(src)` for
each one, with a dedicated `JsError::NotImplemented` arm (line 7077) meant to print a
clean `"module: engine=v8, выполнение пропущено"` skip line — mirroring the same
pattern used for classic `<script>` on engines missing a feature. The comment right
above this block (`main.rs:7039-7042`) says as much: "module scripts fall back to
`JsRuntime::eval_module`'s `NotImplemented` default until ESM lands" — i.e. the call
site was written assuming the trait's default `eval_module` returns `NotImplemented`.

It doesn't. `ext.rs:868`'s actual default is `self.eval(source).map(|_| ())` — it
*executes* the source as a classic script and maps any success value away, it never
returns `NotImplemented`. So the `NotImplemented` arm at `main.rs:7077` is dead code
for V8, and every module script instead falls into the generic `Err(e) => eprintln!
("module error: {e}")` arm with a raw V8 parse error — exactly the observed
`module error: JS runtime error: Unexpected token 'export'` /
`Cannot use import statement outside a module` output. The bug is therefore two-fold:
(a) module scripts have no real semantics on V8, and (b) the failure mode is a
confusing raw syntax error instead of the clean "not implemented, skipped" message
the call site was designed to show.

The real, correct ESM stack already exists — `crates/js/src/esm.rs`
(`LumenLoader`/`LumenResolver`, import maps, `Module::evaluate`) plus
`QuickJsRuntime`'s own `eval_module`/`register_module_source`
(`crates/js/src/lib.rs:633`, delegated to via the trait impl at `lib.rs:2106-2112`) —
and is unit-tested (`eval_module_simple_export`, `eval_module_side_effects_visible`,
`eval_module_imports_registered_module`, `eval_module_syntax_error_returns_error`,
`eval_module_dynamic_import_resolves`, `crates/js/src/lib.rs:2460-2498`). All of it is
compiled only under the non-default `--features quickjs` rollback path
(`crates/shell/Cargo.toml:26`, `default = [..., "v8"]`, ADR-018 S12 cutover) — dead
code for the actual shipping binary. `docs/tasks/ph3-v8-migration.md:150` already
lists `eval_module`/`register_module_source` as a known, un-ported gap for the V8
side — this bug documents the concrete, observed failure mode that gap produces,
not a fresh discovery of the gap's existence.

## Масштаб

Broad. Every `<script type="module">` on the default (V8) engine — the one every
graphic test, WPT run, and real site actually exercises since the 2026-07-14 cutover
— fails identically, independent of whether it has any `import`/`export` statements
that reference other modules or not; a lone module script with a single top-level
`export` is enough. Not narrowed to WPT, not narrowed to this category, not a
double-fetch/mis-typing artifact of loading the same module both via `<script src>`
and via `import` — plain classic-eval rejection of `export`/`import` syntax on first
parse.

## Предлагаемый фикс (not attempted here — filing only, P2 does not own `crates/js`)

Port `eval_module`/`register_module_source` to `V8JsRuntime`, using V8's own
`Module::Compile`/`Module::InstantiateModule`/`Module::Evaluate` machinery (mirroring
what `crates/js/src/esm.rs`'s `LumenLoader`/`LumenResolver` already do for the
QuickJS path) — tracked as part of the broader V8-migration module-support work in
`docs/tasks/ph3-v8-migration.md`.

## Исправление (2026-07-29, P1, ветка `p1-v8-s12b-23-esm`, срез P3-v8-s12b-23)

Реализовано по предложенному плану. Новый модуль `crates/js/src/v8_esm.rs`:
`script_compiler::compile_module` → `instantiate_module` → `evaluate` +
`perform_microtask_checkpoint` (аналог `execute_pending_job`-цикла QuickJS).
`impl JsRuntime for V8JsRuntime` теперь переопределяет `eval_module` и
`register_module_source`, так что трейтовый classic-eval дефолт (`ext.rs`) на
дефолтной сборке больше не достигается; добавлен `V8JsRuntime::set_import_map`,
и шелл (`main.rs`, V8-ветка `run_scripts_with_dom`) его зовёт — комментарий
«module scripts fall back to `NotImplemented` until ESM lands» снят.

Ключевое отличие от rquickjs-плумбинга: `ResolveModuleCallback` у V8 — это
`extern "C" fn` без захвата, поэтому реестры (исходники, скомпилированные
модули, `identity_hash → specifier`, page URL, import map) живут в
`thread_local!` на JS-потоке изолята, а не в `Arc<Mutex<…>>`, разделяемых с
`Loader`/`Resolver`. Разрешение спецификаторов при этом общее для обоих
движков — вынесено в `esm::resolve_specifier_with`, `LumenResolver` делегирует
туда же, так что import maps, относительные URL и виртуальная база
`lumen://inline-N` ведут себя одинаково.

Что V8 умеет сам и потому не эмулируется: import attributes
(`with { type: 'json' }`) приходят в callback готовым `FixedArray` — Phase-0
препроцессор `import_attributes.rs` на V8-пути не нужен; динамический
`import()` подключён изолятным хуком `set_host_import_module_dynamically_callback`.
`import.meta` остался на общем строковом преобразователе `import_meta.rs` —
форма `.url`/`.resolve()`/`.env` там политика Lumen, а не возможность движка.

Также включены inline-модули в headless-пути драйвера
(`crates/driver/src/session.rs::run_page_scripts` — теперь `eval_module` после
классических скриптов, HTML LS §8.1.3.1); ни одна страница `graphic_tests/` не
использует `type="module"`, так что CPU-снапшоты не затронуты.

Проверка: 19 новых тестов в `v8_esm.rs` (экспорт, побочные эффекты, импорт
зарегистрированного модуля, синтаксическая ошибка, отсутствующий модуль, throw
в теле, «ромб» с однократным исполнением, top-level await, динамический import
и его reject, относительный импорт, import map, json/невалидный json/чужой
тип, `import.meta.url`/`resolve`/`env`) + живой прогон
`lumen.exe --dump-layout` на странице с инлайновым `<script type="module">`,
мутирующим DOM: текст меняется (`before` → `module-works`) вместо
`Unexpected token 'export'`.

**Остаток вне этого среза:** страница не может подгрузить импортируемый модуль
по сети — шелл нигде не зовёт `register_module_source`, поэтому
`import './helper.js'` со страницы падает как `module '…/helper.js' not found`.
Это не регрессия (на QuickJS было ровно так же) и не то, что описывал этот баг,
но именно оно нужно 80 вендоренным WPT-файлам — заведено отдельно как
[BUG-446](BUG-446-FIXED.md).
