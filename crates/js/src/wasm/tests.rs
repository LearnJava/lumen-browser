//! Unit tests for the pure-Rust WASM engine (parser + interpreter), driven by
//! hand-assembled module images so they need no external `.wasm` fixtures and
//! no JS context.

use std::rc::Rc;

use super::interp::{HostImports, Instance, Trap};
use super::parser::parse_module;
use super::value::Value;

/// A recording host: returns a fixed value and remembers the args it saw.
struct RecordHost {
    ret: Vec<Value>,
    last_args: Vec<Value>,
}
impl HostImports for RecordHost {
    fn call_host(&mut self, _idx: usize, args: &[Value]) -> Result<Vec<Value>, Trap> {
        self.last_args = args.to_vec();
        Ok(self.ret.clone())
    }
}

struct NoHost;
impl HostImports for NoHost {
    fn call_host(&mut self, _idx: usize, _args: &[Value]) -> Result<Vec<Value>, Trap> {
        Err(Trap("no host".into()))
    }
}

// ── Module-image builders (compute section sizes automatically) ────────────

/// Append an unsigned LEB128 value.
fn leb_u(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

/// Wrap section `content` with its id + LEB size prefix.
fn section(id: u8, content: Vec<u8>) -> Vec<u8> {
    let mut s = vec![id];
    leb_u(&mut s, content.len() as u64);
    s.extend(content);
    s
}

/// Assemble a full module image (magic + version + sections).
fn module(sections: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    for s in sections {
        out.extend(s);
    }
    out
}

/// Build the code section (id 10) from already-encoded function bodies
/// (each body = locals-encoding followed by its instruction bytes).
fn code_section(bodies: Vec<Vec<u8>>) -> Vec<u8> {
    let mut content = Vec::new();
    leb_u(&mut content, bodies.len() as u64);
    for b in bodies {
        leb_u(&mut content, b.len() as u64);
        content.extend(b);
    }
    section(10, content)
}

/// Helper: instantiate a module and call an exported function by name.
fn run(bytes: &[u8], func: &str, args: &[Value]) -> Result<Vec<Value>, String> {
    let m = parse_module(bytes).map_err(|e| format!("parse: {e}"))?;
    let mut inst = Instance::new(Rc::new(m), Vec::new())?;
    let idx = inst
        .export_func_index(func)
        .ok_or_else(|| format!("no export {func}"))?;
    let mut host = NoHost;
    inst.invoke(idx, args, &mut host, 0).map_err(|t| t.0)
}

// ── add(i32,i32)->i32 ──────────────────────────────────────────────────────
const ADD_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7F, 0x7F, 0x01,
    0x7F, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00, 0x0A, 0x09,
    0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6A, 0x0B,
];

#[test]
fn parses_add_module() {
    let m = parse_module(ADD_WASM).unwrap();
    assert_eq!(m.types.len(), 1);
    assert_eq!(m.funcs.len(), 1);
    assert_eq!(m.exports.len(), 1);
    assert_eq!(m.exports[0].name, "add");
}

#[test]
fn executes_add() {
    let r = run(ADD_WASM, "add", &[Value::I32(40), Value::I32(2)]).unwrap();
    assert_eq!(r[0].as_i32(), 42);
}

#[test]
fn executes_add_negative() {
    let r = run(ADD_WASM, "add", &[Value::I32(-5), Value::I32(3)]).unwrap();
    assert_eq!(r[0].as_i32(), -2);
}

// ── fac(i32)->i32 with a loop + locals (factorial, iterative) ──────────────
// (func (export "fac") (param i32) (result i32) (local i32)
//   i32.const 1 local.set 1
//   block
//     loop
//       local.get 0 i32.eqz br_if 1
//       local.get 1 local.get 0 i32.mul local.set 1
//       local.get 0 i32.const 1 i32.sub local.set 0
//       br 0
//     end
//   end
//   local.get 1)
fn fac_wasm() -> Vec<u8> {
    let ty = section(1, vec![0x01, 0x60, 0x01, 0x7F, 0x01, 0x7F]); // (i32)->i32
    let func = section(3, vec![0x01, 0x00]); // func 0 : type 0
    let export = section(7, vec![0x01, 0x03, b'f', b'a', b'c', 0x00, 0x00]);
    let body = vec![
        0x01, 0x01, 0x7F, // 1 local decl: 1 × i32
        0x41, 0x01, 0x21, 0x01, // i32.const 1 ; local.set 1
        0x02, 0x40, // block (void)
        0x03, 0x40, // loop (void)
        0x20, 0x00, 0x45, 0x0D, 0x01, // local.get 0 ; i32.eqz ; br_if 1
        0x20, 0x01, 0x20, 0x00, 0x6C, 0x21, 0x01, // local1; local0; mul; set1
        0x20, 0x00, 0x41, 0x01, 0x6B, 0x21, 0x00, // local0; const1; sub; set0
        0x0C, 0x00, // br 0
        0x0B, // end (loop)
        0x0B, // end (block)
        0x20, 0x01, // local.get 1
        0x0B, // end (func)
    ];
    module(vec![ty, func, export, code_section(vec![body])])
}

#[test]
fn executes_factorial_loop() {
    let w = fac_wasm();
    assert_eq!(run(&w, "fac", &[Value::I32(5)]).unwrap()[0].as_i32(), 120);
    assert_eq!(run(&w, "fac", &[Value::I32(0)]).unwrap()[0].as_i32(), 1);
    assert_eq!(run(&w, "fac", &[Value::I32(1)]).unwrap()[0].as_i32(), 1);
    assert_eq!(run(&w, "fac", &[Value::I32(10)]).unwrap()[0].as_i32(), 3628800);
}

// ── if/else: max(i32,i32) ──────────────────────────────────────────────────
// (func (export "max") (param i32 i32) (result i32)
//   local.get 0 local.get 1 i32.gt_s
//   if (result i32) local.get 0 else local.get 1 end)
const MAX_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7F, 0x7F, 0x01,
    0x7F, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x6D, 0x61, 0x78, 0x00, 0x00, // export max
    0x0A, 0x11, 0x01, 0x0F, 0x00, // code: body size 0x0F
    0x20, 0x00, 0x20, 0x01, 0x4A, // local.get0; local.get1; i32.gt_s
    0x04, 0x7F, // if (result i32)
    0x20, 0x00, // local.get 0
    0x05, // else
    0x20, 0x01, // local.get 1
    0x0B, // end if
    0x0B, // end func
];

#[test]
fn executes_if_else_max() {
    assert_eq!(run(MAX_WASM, "max", &[Value::I32(3), Value::I32(7)]).unwrap()[0].as_i32(), 7);
    assert_eq!(run(MAX_WASM, "max", &[Value::I32(9), Value::I32(2)]).unwrap()[0].as_i32(), 9);
}

// ── memory: store then load ────────────────────────────────────────────────
// (module (memory 1) (export "mem" (memory 0))
//   (func (export "rw") (param i32 i32) (result i32)
//     local.get 0 local.get 1 i32.store
//     local.get 0 i32.load))
fn mem_wasm() -> Vec<u8> {
    let ty = section(1, vec![0x01, 0x60, 0x02, 0x7F, 0x7F, 0x01, 0x7F]); // (i32,i32)->i32
    let func = section(3, vec![0x01, 0x00]);
    let mem = section(5, vec![0x01, 0x00, 0x01]); // 1 memory, flags 0, min 1
    let export = section(
        7,
        vec![
            0x02, // 2 exports
            0x03, b'm', b'e', b'm', 0x02, 0x00, // "mem" memory 0
            0x02, b'r', b'w', 0x00, 0x00, // "rw" func 0
        ],
    );
    let body = vec![
        0x00, // 0 local decls
        0x20, 0x00, 0x20, 0x01, 0x36, 0x02, 0x00, // local0; local1; i32.store align=2 off=0
        0x20, 0x00, 0x28, 0x02, 0x00, // local0; i32.load align=2 off=0
        0x0B,
    ];
    module(vec![ty, func, mem, export, code_section(vec![body])])
}

#[test]
fn executes_memory_store_load() {
    let r = run(&mem_wasm(), "rw", &[Value::I32(16), Value::I32(0x12345678)]).unwrap();
    assert_eq!(r[0].as_i32(), 0x12345678);
}

#[test]
fn memory_export_present() {
    let m = parse_module(&mem_wasm()).unwrap();
    assert!(m.memories.len() == 1);
    assert!(m.exports.iter().any(|e| e.name == "mem"));
}

// ── imported function: callImport() calls env.h() and returns its value+1 ──
// (module (import "env" "h" (func (result i32)))
//   (func (export "f") (result i32) call 0 i32.const 1 i32.add))
const IMPORT_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
    0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7F, // type ()->i32
    0x02, 0x09, 0x01, 0x03, 0x65, 0x6E, 0x76, 0x01, 0x68, 0x00, 0x00, // import env.h func type0
    0x03, 0x02, 0x01, 0x00, // func 1 (defined) type 0
    0x07, 0x05, 0x01, 0x01, 0x66, 0x00, 0x01, // export "f" func 1
    0x0A, 0x09, 0x01, 0x07, 0x00, // code body size 7
    0x10, 0x00, 0x41, 0x01, 0x6A, // call 0 ; i32.const 1 ; i32.add
    0x0B,
];

#[test]
fn calls_imported_function() {
    let m = parse_module(IMPORT_WASM).unwrap();
    assert_eq!(m.num_imported_funcs, 1);
    let mut inst = Instance::new(Rc::new(m), Vec::new()).unwrap();
    let idx = inst.export_func_index("f").unwrap();
    let mut host = RecordHost {
        ret: vec![Value::I32(41)],
        last_args: Vec::new(),
    };
    let r = inst.invoke(idx, &[], &mut host, 0).unwrap();
    assert_eq!(r[0].as_i32(), 42); // 41 from host + 1
}

// ── trap: division by zero ─────────────────────────────────────────────────
// (func (export "d") (param i32 i32) (result i32) local.get 0 local.get 1 i32.div_s)
const DIV_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7F, 0x7F, 0x01,
    0x7F, 0x03, 0x02, 0x01, 0x00, 0x07, 0x05, 0x01, 0x01, 0x64, 0x00, 0x00, 0x0A, 0x09, 0x01, 0x07,
    0x00, 0x20, 0x00, 0x20, 0x01, 0x6D, 0x0B,
];

#[test]
fn divide_works_and_traps_on_zero() {
    assert_eq!(run(DIV_WASM, "d", &[Value::I32(20), Value::I32(5)]).unwrap()[0].as_i32(), 4);
    let err = run(DIV_WASM, "d", &[Value::I32(1), Value::I32(0)]).unwrap_err();
    assert!(err.contains("divide by zero"), "got: {err}");
}

// ── f64 arithmetic: mul(f64,f64)->f64 ──────────────────────────────────────
const FMUL_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7C, 0x7C, 0x01,
    0x7C, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x6D, 0x75, 0x6C, 0x00, 0x00, 0x0A, 0x09,
    0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0xA2, 0x0B,
];

#[test]
fn executes_f64_mul() {
    let r = run(FMUL_WASM, "mul", &[Value::F64(2.5), Value::F64(4.0)]).unwrap();
    assert!((r[0].as_f64() - 10.0).abs() < 1e-12);
}

// ── nested blocks + multi-level branch carrying a value ────────────────────
// (func (export "sel") (param i32) (result i32)
//   block (result i32)
//     block (result i32)
//       i32.const 10
//       local.get 0
//       br_if 1      ;; if param != 0 → branch to OUTER carrying 10
//       drop
//       i32.const 20
//     end
//   end)
fn sel_wasm() -> Vec<u8> {
    let ty = section(1, vec![0x01, 0x60, 0x01, 0x7F, 0x01, 0x7F]);
    let func = section(3, vec![0x01, 0x00]);
    let export = section(7, vec![0x01, 0x03, b's', b'e', b'l', 0x00, 0x00]);
    let body = vec![
        0x00, // no locals
        0x02, 0x7F, // block (result i32)  — outer
        0x02, 0x7F, // block (result i32)  — inner
        0x41, 0x0A, // i32.const 10
        0x20, 0x00, // local.get 0
        0x0D, 0x01, // br_if 1 (to outer)
        0x1A, // drop
        0x41, 0x14, // i32.const 20
        0x0B, // end inner
        0x0B, // end outer
        0x0B, // end func
    ];
    module(vec![ty, func, export, code_section(vec![body])])
}

#[test]
fn nested_branch_carries_value() {
    assert_eq!(run(&sel_wasm(), "sel", &[Value::I32(1)]).unwrap()[0].as_i32(), 10);
    assert_eq!(run(&sel_wasm(), "sel", &[Value::I32(0)]).unwrap()[0].as_i32(), 20);
}

#[test]
fn rejects_bad_header() {
    assert!(parse_module(&[0, 0, 0, 0]).is_err());
    assert!(parse_module(b"\0asm\x02\0\0\0").is_err()); // wrong version
}

#[test]
fn validate_via_bridge() {
    assert!(super::validate(ADD_WASM));
    assert!(!super::validate(&[1, 2, 3]));
}

#[test]
fn bridge_compile_and_introspect() {
    let id = super::compile(ADD_WASM).unwrap();
    let exports = super::module_exports_json(id);
    assert!(exports.contains("\"add\""));
    assert!(exports.contains("\"function\""));
}
