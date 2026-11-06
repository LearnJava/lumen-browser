use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_core::JsValue;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        make_doc(),
        "",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        None,
    )
    .unwrap();
    rt
}

const ADD_BYTES: &str = "[0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,3,2,1,0,7,7,1,3,97,100,100,0,0,10,9,1,7,0,32,0,32,1,106,11]";

#[test]
fn v8_wasm_native_module_accepts_anyref() {
    let rt = rt();
    let ok = rt
        .eval(
            "var bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,5,1,96,1,110,0,3,2,1,0,10,4,1,2,0,11]);\
             WebAssembly.validate(bytes) && new WebAssembly.Module(bytes) instanceof WebAssembly.Module",
        )
        .unwrap();
    assert_eq!(ok, JsValue::Bool(true));
}

#[test]
fn v8_wasm_native_module_accepts_simd() {
    let rt = rt();
    let ok = rt
        .eval(
            "var bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,5,1,96,0,1,123,3,2,1,0,10,22,1,20,0,253,12,1,0,0,0,2,0,0,0,3,0,0,0,4,0,0,0,11]);\
             WebAssembly.validate(bytes) && new WebAssembly.Module(bytes) instanceof WebAssembly.Module",
        )
        .unwrap();
    assert_eq!(ok, JsValue::Bool(true));
}

#[test]
fn v8_wasm_native_instance_calls_export() {
    let rt = rt();
    let result = rt
        .eval(&format!(
            "var module = new WebAssembly.Module(new Uint8Array({ADD_BYTES}));\
             new WebAssembly.Instance(module).exports.add(40, 2)"
        ))
        .unwrap();
    assert_eq!(result, JsValue::Number(42.0));
}

#[test]
fn v8_wasm_native_compilation_ignores_trusted_types_script_enforcement() {
    let rt = rt();
    let result = rt
        .eval(&format!(
            "_lumen_tt_set_require_script(true);\
             var module = new WebAssembly.Module(new Uint8Array({ADD_BYTES}));\
             new WebAssembly.Instance(module).exports.add(20, 22)"
        ))
        .unwrap();
    assert_eq!(result, JsValue::Number(42.0));
}

// WASM-V8-NATIVE-S3: what the deleted self-made interpreter covered, asserted
// against the native V8 implementation (memory object + growth semantics).

/// Minimal module exporting a 1-page memory as `mem` (sections: Memory, Export;
/// export external-kind 2 = Memory).
const MEM_BYTES: &str = "[0,97,115,109,1,0,0,0,5,3,1,0,1,7,7,1,3,109,101,109,2,0]";

#[test]
fn v8_wasm_native_memory_grows_pages() {
    let rt = rt();
    let ok = rt
        .eval(&format!(
            "var mem = new WebAssembly.Instance(\
                 new WebAssembly.Module(new Uint8Array({MEM_BYTES}))).exports.mem;\
             mem.buffer.byteLength === 65536 && mem.grow(1) === 1 && \
             mem.buffer.byteLength === 131072"
        ))
        .unwrap();
    assert_eq!(ok, JsValue::Bool(true));
}

/// Module importing `env.f: () -> i32` and exporting `run` calling it
/// (sections: Type, Import, Function, Export, Code).
const HOST_IMPORT_BYTES: &str = "[0,97,115,109,1,0,0,0,1,5,1,96,0,1,127,2,9,1,3,101,110,118,1,102,0,0,3,2,1,0,7,7,1,3,114,117,110,0,1,10,6,1,4,0,16,0,11]";

#[test]
fn v8_wasm_native_host_function_import() {
    let rt = rt();
    let result = rt
        .eval(&format!(
            "var module = new WebAssembly.Module(new Uint8Array({HOST_IMPORT_BYTES}));\
             new WebAssembly.Instance(module, {{env: {{f: function () {{ return 42; }} }}}})\
                 .exports.run()"
        ))
        .unwrap();
    assert_eq!(result, JsValue::Number(42.0));
}
