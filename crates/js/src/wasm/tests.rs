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

// ── SIMD (v128 / 0xFD) ──────────────────────────────────────────────────────

/// `v128.const` with four little-endian i32 lanes.
fn v128_i32(lanes: [i32; 4]) -> Vec<u8> {
    let mut v = vec![0xFD, 0x0C];
    for l in lanes {
        v.extend_from_slice(&l.to_le_bytes());
    }
    v
}

/// `v128.const` with four little-endian f32 lanes.
fn v128_f32(lanes: [f32; 4]) -> Vec<u8> {
    let mut v = vec![0xFD, 0x0C];
    for l in lanes {
        v.extend_from_slice(&l.to_le_bytes());
    }
    v
}

/// `v128.const` from raw bytes.
fn v128_bytes(bytes: [u8; 16]) -> Vec<u8> {
    let mut v = vec![0xFD, 0x0C];
    v.extend_from_slice(&bytes);
    v
}

/// `0xFD` prefix + LEB-encoded sub-opcode (for sub-opcodes ≥ 128).
fn fd(sub: u64) -> Vec<u8> {
    let mut v = vec![0xFD];
    leb_u(&mut v, sub);
    v
}

/// Build a no-param module exporting `f` with the given result types and body
/// instruction bytes (the trailing `end` is appended automatically). A 1-page
/// memory is always present so load/store tests work.
fn simd_module(results: &[u8], code: Vec<u8>) -> Vec<u8> {
    let mut ty = vec![0x01, 0x60, 0x00];
    leb_u(&mut ty, results.len() as u64);
    ty.extend_from_slice(results);
    let ty = section(1, ty);
    let func = section(3, vec![0x01, 0x00]);
    let mem = section(5, vec![0x01, 0x00, 0x01]);
    let export = section(7, vec![0x01, 0x01, b'f', 0x00, 0x00]);
    let mut body = vec![0x00];
    body.extend(code);
    body.push(0x0B);
    module(vec![ty, func, mem, export, code_section(vec![body])])
}

/// Run a v128-returning body and return the 16 result bytes.
fn run_v128(code: Vec<u8>) -> [u8; 16] {
    run(&simd_module(&[0x7B], code), "f", &[]).unwrap()[0].as_v128()
}

/// Extract four i32 lanes from raw v128 bytes.
fn lanes_i32(v: [u8; 16]) -> [i32; 4] {
    let mut out = [0i32; 4];
    for (i, o) in out.iter_mut().enumerate() {
        *o = i32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap());
    }
    out
}

#[test]
fn simd_module_decodes_without_error() {
    // The decoder used to reject any 0xFD opcode; now it parses.
    let mut body = v128_i32([1, 2, 3, 4]);
    body.push(0x1A); // drop the const, leaving an empty result
    assert!(parse_module(&simd_module(&[], body)).is_ok());
}

#[test]
fn simd_i32x4_add() {
    let mut code = v128_i32([1, 2, 3, 4]);
    code.extend(v128_i32([10, 20, 30, 40]));
    code.extend(fd(174)); // i32x4.add
    assert_eq!(lanes_i32(run_v128(code)), [11, 22, 33, 44]);
}

#[test]
fn simd_i32x4_mul_wraps() {
    let mut code = v128_i32([i32::MAX, 2, -3, 0]);
    code.extend(v128_i32([2, 2, 2, 7]));
    code.extend(fd(181)); // i32x4.mul
    assert_eq!(lanes_i32(run_v128(code)), [-2, 4, -6, 0]);
}

#[test]
fn simd_i8x16_splat() {
    // i32.const 60 ; i8x16.splat  (60 < 64, single-byte signed LEB)
    let mut code = vec![0x41, 60]; // i32.const 60
    code.extend(fd(15)); // i8x16.splat
    assert_eq!(run_v128(code), [60u8; 16]);
}

#[test]
fn simd_i32x4_extract_lane() {
    let mut code = v128_i32([5, 6, 7, 8]);
    code.extend(fd(27)); // i32x4.extract_lane
    code.push(0x02); // lane 2
    let r = run(&simd_module(&[0x7F], code), "f", &[]).unwrap();
    assert_eq!(r[0].as_i32(), 7);
}

#[test]
fn simd_i32x4_replace_lane() {
    let mut code = v128_i32([5, 6, 7, 8]);
    code.push(0x41);
    code.push(30); // i32.const 30  (30 < 64, single-byte signed LEB)
    code.extend(fd(28)); // i32x4.replace_lane
    code.push(0x01); // lane 1
    assert_eq!(lanes_i32(run_v128(code)), [5, 30, 7, 8]);
}

#[test]
fn simd_f32x4_add() {
    let mut code = v128_f32([1.5, 2.5, 3.5, 4.5]);
    code.extend(v128_f32([0.5, 0.5, 0.5, 0.5]));
    code.extend(fd(228)); // f32x4.add
    let v = run_v128(code);
    let lanes: Vec<f32> = (0..4)
        .map(|i| f32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap()))
        .collect();
    assert_eq!(lanes, vec![2.0, 3.0, 4.0, 5.0]);
}

#[test]
fn simd_i32x4_eq_mask() {
    let mut code = v128_i32([1, 2, 3, 4]);
    code.extend(v128_i32([1, 9, 3, 9]));
    code.extend(fd(55)); // i32x4.eq
    assert_eq!(lanes_i32(run_v128(code)), [-1, 0, -1, 0]); // all-ones = -1
}

#[test]
fn simd_v128_store_then_load() {
    // i32.const 0 ; v128.const ... ; v128.store off 0 ; i32.const 0 ; v128.load off 0
    let mut code = vec![0x41, 0x00]; // i32.const 0
    code.extend(v128_i32([7, 8, 9, 10]));
    code.extend(fd(11)); // v128.store
    code.push(0x00); // align
    code.push(0x00); // offset
    code.extend([0x41, 0x00]); // i32.const 0
    code.extend(fd(0)); // v128.load
    code.push(0x00); // align
    code.push(0x00); // offset
    assert_eq!(lanes_i32(run_v128(code)), [7, 8, 9, 10]);
}

#[test]
fn simd_shuffle() {
    let mut code = v128_bytes([0x11; 16]);
    code.extend(v128_bytes([0x22; 16]));
    code.extend(fd(13)); // i8x16.shuffle
    // lanes 0..7 from a (0x11), lanes 16..23 from b (0x22)
    code.extend([0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23]);
    let mut expected = [0x11u8; 16];
    for e in expected.iter_mut().skip(8) {
        *e = 0x22;
    }
    assert_eq!(run_v128(code), expected);
}

#[test]
fn simd_bitselect() {
    let mut code = v128_bytes([0xFF; 16]); // v1
    code.extend(v128_bytes([0x00; 16])); // v2
    code.extend(v128_bytes([0xF0; 16])); // control
    code.extend(fd(82)); // bitselect
    assert_eq!(run_v128(code), [0xF0; 16]); // (0xFF & 0xF0) | (0x00 & 0x0F)
}

#[test]
fn simd_extend_low_i16x8_s() {
    // first four i16 lanes = -1, 1000, -2, 32767
    let mut bytes = [0u8; 16];
    bytes[0..2].copy_from_slice(&(-1i16).to_le_bytes());
    bytes[2..4].copy_from_slice(&1000i16.to_le_bytes());
    bytes[4..6].copy_from_slice(&(-2i16).to_le_bytes());
    bytes[6..8].copy_from_slice(&32767i16.to_le_bytes());
    let mut code = v128_bytes(bytes);
    code.extend(fd(165)); // i32x4.extend_low_i16x8_s
    assert_eq!(lanes_i32(run_v128(code)), [-1, 1000, -2, 32767]);
}

#[test]
fn simd_dot_i16x8_s() {
    // a = [1,2,3,4,5,6,7,8] i16, b = [1,1,1,1,1,1,1,1]
    let mut a = [0u8; 16];
    let mut b = [0u8; 16];
    for i in 0..8 {
        a[i * 2..i * 2 + 2].copy_from_slice(&((i as i16) + 1).to_le_bytes());
        b[i * 2..i * 2 + 2].copy_from_slice(&1i16.to_le_bytes());
    }
    let mut code = v128_bytes(a);
    code.extend(v128_bytes(b));
    code.extend(fd(186)); // i32x4.dot_i16x8_s
    // pairwise: (1+2),(3+4),(5+6),(7+8)
    assert_eq!(lanes_i32(run_v128(code)), [3, 7, 11, 15]);
}

#[test]
fn simd_i8x16_add_sat_s() {
    let mut code = v128_bytes([100u8; 16]); // 100 as i8
    code.extend(v128_bytes([100u8; 16]));
    code.extend(fd(111)); // i8x16.add_sat_s -> saturates to 127
    assert_eq!(run_v128(code), [127u8; 16]);
}

#[test]
fn simd_i32x4_trunc_sat_f32x4_s() {
    let mut code = v128_f32([3.9, -2.1, 1e30, f32::NAN]);
    code.extend(fd(248)); // i32x4.trunc_sat_f32x4_s
    assert_eq!(lanes_i32(run_v128(code)), [3, -2, i32::MAX, 0]);
}
