//! BUG-1049: `_lumen_run_classic_script(text)` — исполнение тела классического
//! `<script>`, вставленного через DOM, как настоящего Script, а не индиректного
//! `eval`.
//!
//! Индиректный `eval` кладёт верхнеуровневые `let`/`const`/`class` в
//! декларативную среду, которая умирает вместе с вызовом, тогда как Script
//! (ECMA-262 §16.1.7, GlobalDeclarationInstantiation) — в глобальную
//! лексическую среду, общую для всех классических скриптов и модулей
//! документа. Поэтому `const x = 1` из подгруженного `loadScript()`-ом скрипта
//! не был виден модулю как свободная переменная (`ReferenceError`).
//!
//! Исключение (в том числе `SyntaxError` компиляции) пробрасывается вызывающему:
//! репортит его shim (`_lumen_script_execute_classic`).

pub(crate) fn run_classic_script(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    _rv: &mut v8::ReturnValue,
) {
    let Some(text) = args.get(0).to_string(scope) else {
        return;
    };
    v8::tc_scope!(tc, scope);
    let compiled = v8::Script::compile(tc, text, None);
    if let Some(compiled) = compiled {
        compiled.run(tc);
    }
    if tc.has_caught() {
        tc.rethrow();
    }
}
