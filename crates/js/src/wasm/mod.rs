//! WebAssembly MVP execution engine for Lumen (U-4 stage 1).
//!
//! A pure-Rust interpreter for the WASM 1.0 core instruction set (plus a few
//! common post-MVP ops: saturating truncation, `memory.copy`/`fill`, sign
//! extension, reference-null) and the complete **fixed-width SIMD** proposal
//! (`v128`, the `0xFD` prefix — see [`simd`]). No external WASM runtime
//! dependency — consistent with Lumen's "lightweight custom engine" principle.
//!
//! The [`webassembly`](crate::webassembly) JS shim drives this engine through
//! native `__lumen_wasm_*` bindings, so `WebAssembly.instantiate(...).exports`
//! produces functions that actually execute bytecode (previously empty stubs).
//!
//! ## Bridge model
//! * Modules and instances live in a thread-local [`REGISTRY`] keyed by id.
//! * Linear memory is authoritative in Rust; JS reads/writes it via copy
//!   helpers (`__lumen_wasm_mem_read`/`write`/`mem_read_all`). The exported
//!   `Memory.buffer` is a single, stable JS `ArrayBuffer` synchronized with
//!   Rust-owned memory at WASM call boundaries (JS → Rust before each export
//!   call, Rust → JS in place after), so the emscripten
//!   `HEAP32 = new Int32Array(memory.buffer)` pattern is **coherent** (U-4b):
//!   writes in either engine become visible to the other across calls, and a
//!   captured `HEAP*` view stays valid because the buffer identity is reused.
//!   The sync is exact for the single-agent model (ADR-014) — WASM and JS never
//!   run concurrently — though a host import still cannot observe writes made
//!   earlier in the same in-flight call.
//! * Imported functions are JS callables stored as [`Persistent`] and invoked
//!   from the interpreter through [`interp::HostImports`]. Numeric arguments and
//!   results cross the boundary by type: `i64` rides as a JS `BigInt` (full
//!   64-bit precision, per the W3C WebAssembly JS Interface), the rest as
//!   `Number`. The same typed marshalling applies to exported functions and
//!   globals (see [`wasm_value_to_js`] / [`js_value_to_wasm`]).

pub mod interp;
pub mod parser;
pub mod simd;
pub mod value;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rquickjs::function::Args;
use rquickjs::{Ctx, Function, Persistent};

use interp::{HostImports, Instance, Trap};
use parser::{ExportKind, ImportKind, Module};
use value::{FuncType, ValType, Value};

/// A live instance plus the JS functions resolving its imports (in func-import
/// order).
struct InstanceEntry {
    instance: Instance,
    host_funcs: Vec<Persistent<Function<'static>>>,
}

/// Thread-local store of compiled modules and live instances.
#[derive(Default)]
struct Registry {
    next_module: u32,
    modules: HashMap<u32, Rc<Module>>,
    next_instance: u32,
    instances: HashMap<u32, InstanceEntry>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

/// `true` if `bytes` decode as a valid module this engine can run.
pub fn validate(bytes: &[u8]) -> bool {
    parser::parse_module(bytes).is_ok()
}

/// Decode and store a module; returns its registry id.
pub fn compile(bytes: &[u8]) -> Result<u32, String> {
    let m = parser::parse_module(bytes)?;
    Ok(REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let id = r.next_module;
        r.next_module += 1;
        r.modules.insert(id, Rc::new(m));
        id
    }))
}

/// Look up a compiled module by id.
fn with_module<T>(id: u32, f: impl FnOnce(&Rc<Module>) -> T) -> Option<T> {
    REGISTRY.with(|r| r.borrow().modules.get(&id).map(f))
}

/// Drop all compiled modules and live instances on this thread, releasing the
/// [`Persistent`] JS handles held for function imports.
///
/// Must be called before the owning JS [`rquickjs::Runtime`] is dropped:
/// otherwise the leaked `Persistent`s keep GC objects alive and QuickJS aborts
/// with a `list_empty(&rt->gc_obj_list)` assertion on teardown (BUG-222 tracks
/// wiring this into shell context teardown).
pub fn clear_registry() {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        r.modules.clear();
        r.instances.clear();
    });
}

/// JSON descriptor of a module's exports (consumed by the JS shim to build the
/// `exports` object).
pub fn module_exports_json(id: u32) -> String {
    let Some(items) = with_module(id, |m| {
        m.exports
            .iter()
            .map(|e| {
                let kind = match e.kind {
                    ExportKind::Func => "function",
                    ExportKind::Table => "table",
                    ExportKind::Memory => "memory",
                    ExportKind::Global => "global",
                };
                serde_json::json!({ "name": e.name, "kind": kind, "index": e.index })
            })
            .collect::<Vec<_>>()
    }) else {
        return "[]".into();
    };
    serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
}

/// JSON descriptor of a module's imports (consumed by the JS shim to resolve
/// the `importObject`).
pub fn module_imports_json(id: u32) -> String {
    let Some(items) = with_module(id, |m| {
        m.imports
            .iter()
            .map(|imp| {
                let kind = match imp.kind {
                    ImportKind::Func(_) => "function",
                    ImportKind::Table { .. } => "table",
                    ImportKind::Memory(_) => "memory",
                    ImportKind::Global { .. } => "global",
                };
                serde_json::json!({ "module": imp.module, "name": imp.name, "kind": kind })
            })
            .collect::<Vec<_>>()
    }) else {
        return "[]".into();
    };
    serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
}

/// Instantiate a compiled module.
///
/// `host_funcs` are the JS callables resolving function imports (func-import
/// order); `imported_globals` are the numeric values for imported globals
/// (global-import order). Returns the new instance id, or an error string
/// (surfaced as `LinkError`).
pub fn instantiate(
    ctx: &Ctx,
    module_id: u32,
    host_funcs: Vec<Persistent<Function<'static>>>,
    imported_globals: Vec<f64>,
) -> Result<u32, String> {
    let module = with_module(module_id, Rc::clone).ok_or("unknown module")?;

    // Map imported global f64s onto typed values.
    let mut g_iter = imported_globals.into_iter();
    let mut typed_globals: Vec<Value> = Vec::new();
    for imp in &module.imports {
        if let ImportKind::Global { ty, .. } = imp.kind {
            let raw = g_iter.next().unwrap_or(0.0);
            typed_globals.push(f64_to_value(ty, raw));
        }
    }

    let mut instance = Instance::new(module.clone(), typed_globals)?;
    {
        // The `start` function may call imports — run it with the JS host.
        let mut host = JsHost {
            ctx,
            funcs: &host_funcs,
            module: module.clone(),
        };
        instance.run_start(&mut host).map_err(|t| t.0)?;
    }

    Ok(REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let id = r.next_instance;
        r.next_instance += 1;
        r.instances.insert(
            id,
            InstanceEntry {
                instance,
                host_funcs,
            },
        );
        id
    }))
}

/// Bridge implementing [`HostImports`] by calling stored JS functions.
struct JsHost<'a, 'js> {
    ctx: &'a Ctx<'js>,
    funcs: &'a [Persistent<Function<'static>>],
    module: Rc<Module>,
}

impl<'a, 'js> HostImports for JsHost<'a, 'js> {
    fn call_host(&mut self, import_index: usize, args: &[Value]) -> Result<Vec<Value>, Trap> {
        let pf = self
            .funcs
            .get(import_index)
            .ok_or_else(|| Trap(format!("unresolved import {import_index}")))?;
        let func = pf
            .clone()
            .restore(self.ctx)
            .map_err(|e| Trap(format!("import restore failed: {e}")))?;
        let mut call_args = Args::new(self.ctx.clone(), args.len());
        for v in args {
            // Each argument carries its own WASM type, so an `i64` crosses the
            // boundary as a JS `BigInt` (not a lossy `f64`).
            call_args
                .push_arg(wasm_value_to_js(self.ctx, *v))
                .map_err(|e| Trap(format!("arg marshal failed: {e}")))?;
        }
        let ret: rquickjs::Value = call_args
            .apply(&func)
            .map_err(|e| Trap(format!("import call threw: {e}")))?;

        let rtypes = self
            .module
            .func_type(import_index as u32)
            .map(|t| t.results.clone())
            .unwrap_or_default();
        match rtypes.first() {
            None => Ok(Vec::new()),
            // An `i64` result is read back exactly from a returned `BigInt`.
            Some(ValType::I64) => Ok(vec![Value::I64(js_value_to_i64(&ret))]),
            Some(ty) => Ok(vec![f64_to_value(*ty, js_value_to_f64(&ret))]),
        }
    }
}

/// Parameter and result value types of an exported function (by its function
/// index) of a live instance. Returns `None` if the instance or function index
/// is unknown. Used by the JS bridge to marshal each argument to its declared
/// type (so `i64` survives the boundary as a `BigInt`).
pub fn func_signature(instance_id: u32, func_idx: u32) -> Option<(Vec<ValType>, Vec<ValType>)> {
    REGISTRY.with(|r| {
        let r = r.borrow();
        let e = r.instances.get(&instance_id)?;
        let ft = e.instance.module.func_type(func_idx)?;
        Some((ft.params.clone(), ft.results.clone()))
    })
}

/// Call an exported function with already-typed arguments, returning typed
/// results.
///
/// The caller (the JS bridge) coerces each JS argument to its declared
/// parameter type before invoking, so `i64` values keep full 64-bit precision
/// instead of being squeezed through an `f64`. Errors are runtime traps,
/// surfaced as `RuntimeError`.
pub fn call_typed(
    ctx: &Ctx,
    instance_id: u32,
    func_idx: u32,
    args: &[Value],
) -> Result<Vec<Value>, String> {
    // Take the entry out so re-entrant calls into a *different* instance work;
    // re-entry into the same instance returns an error rather than panicking.
    let mut entry = REGISTRY
        .with(|r| r.borrow_mut().instances.remove(&instance_id))
        .ok_or("unknown or busy instance")?;

    let module = entry.instance.module.clone();
    let result = {
        let mut host = JsHost {
            ctx,
            funcs: &entry.host_funcs,
            module: module.clone(),
        };
        entry.instance.invoke(func_idx, args, &mut host, 0)
    };

    // Reinsert before propagating any error.
    REGISTRY.with(|r| {
        r.borrow_mut().instances.insert(instance_id, entry);
    });

    result.map_err(|t| t.0)
}

/// Current memory size of an instance, in 64 KiB pages.
pub fn mem_size(instance_id: u32) -> u32 {
    REGISTRY.with(|r| {
        r.borrow()
            .instances
            .get(&instance_id)
            .map(|e| e.instance.mem_pages())
            .unwrap_or(0)
    })
}

/// Grow an instance's memory by `delta` pages; previous size or -1 on failure.
pub fn mem_grow(instance_id: u32, delta: u32) -> i32 {
    REGISTRY.with(|r| {
        r.borrow_mut()
            .instances
            .get_mut(&instance_id)
            .map(|e| e.instance.mem_grow(delta))
            .unwrap_or(-1)
    })
}

/// Copy `len` bytes of an instance's linear memory starting at `offset`.
pub fn mem_read(instance_id: u32, offset: u32, len: u32) -> Vec<u8> {
    REGISTRY.with(|r| {
        let r = r.borrow();
        let Some(e) = r.instances.get(&instance_id) else {
            return Vec::new();
        };
        let start = offset as usize;
        let end = start.saturating_add(len as usize).min(e.instance.memory.len());
        if start >= e.instance.memory.len() {
            Vec::new()
        } else {
            e.instance.memory[start..end].to_vec()
        }
    })
}

/// Write `bytes` into an instance's linear memory at `offset`. Returns `false`
/// if the write would exceed the current memory size.
pub fn mem_write(instance_id: u32, offset: u32, bytes: &[u8]) -> bool {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let Some(e) = r.instances.get_mut(&instance_id) else {
            return false;
        };
        let start = offset as usize;
        let end = start.saturating_add(bytes.len());
        if end > e.instance.memory.len() {
            return false;
        }
        e.instance.memory[start..end].copy_from_slice(bytes);
        true
    })
}

/// Full linear-memory snapshot of an instance (every page). Returns an empty
/// vector for an unknown instance. Used by the JS bridge to (re)build the stable
/// exported `Memory.buffer` and to sync Rust → JS after each call — a single
/// bulk copy instead of element-wise `mem_read` round-trips.
pub fn mem_read_all(instance_id: u32) -> Vec<u8> {
    REGISTRY.with(|r| {
        r.borrow()
            .instances
            .get(&instance_id)
            .map(|e| e.instance.memory.clone())
            .unwrap_or_default()
    })
}

/// Read an exported global's current value (typed). Returns `None` if the
/// instance or global index is unknown. The JS bridge maps an `i64` global to a
/// `BigInt` and the others to `Number`.
pub fn global_value(instance_id: u32, index: u32) -> Option<Value> {
    REGISTRY.with(|r| {
        r.borrow()
            .instances
            .get(&instance_id)
            .and_then(|e| e.instance.globals.get(index as usize).copied())
    })
}

/// Set a mutable exported global from a typed value (coerced to its declared
/// type, preserving `i64` precision). Returns `false` if the index is invalid
/// or the global is immutable.
pub fn global_set_value(instance_id: u32, index: u32, v: Value) -> bool {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let Some(e) = r.instances.get_mut(&instance_id) else {
            return false;
        };
        let idx = index as usize;
        if idx >= e.instance.globals.len() || !e.instance.global_mut.get(idx).copied().unwrap_or(false)
        {
            return false;
        }
        let ty = e.instance.globals[idx].val_type();
        e.instance.globals[idx] = coerce_value(ty, v);
        true
    })
}

// ── Value marshalling ──────────────────────────────────────────────────────

/// Convert a runtime value to the `f64` carried across the JS boundary.
fn value_to_f64(v: Value) -> f64 {
    match v {
        Value::I32(x) => x as f64,
        Value::I64(x) => x as f64,
        Value::F32(x) => x as f64,
        Value::F64(x) => x,
        Value::FuncRef(r) | Value::ExternRef(r) => r.map(f64::from).unwrap_or(-1.0),
        // v128 has no numeric JS representation (the spec rejects it at the
        // boundary); collapse to 0.0 — never reached by a spec-valid call.
        Value::V128(_) => 0.0,
    }
}

/// Coerce an incoming `f64` to a typed value for `ty`.
fn f64_to_value(ty: ValType, v: f64) -> Value {
    match ty {
        ValType::I32 => Value::I32(v as i64 as i32),
        ValType::I64 => Value::I64(v as i64),
        ValType::F32 => Value::F32(v as f32),
        ValType::F64 => Value::F64(v),
        // v128 cannot be constructed from a JS number; yield a zero vector.
        ValType::V128 => Value::V128([0; 16]),
        ValType::FuncRef => Value::FuncRef(if v < 0.0 { None } else { Some(v as u32) }),
        ValType::ExternRef => Value::ExternRef(if v < 0.0 { None } else { Some(v as u32) }),
    }
}

/// Coerce a typed value to type `ty`, preserving `i64` exactly (the `f64` path
/// would round-trip a 64-bit integer through a 53-bit mantissa).
fn coerce_value(ty: ValType, v: Value) -> Value {
    match (ty, v) {
        (ValType::I64, Value::I64(x)) => Value::I64(x),
        (ValType::I64, other) => Value::I64(value_to_f64(other) as i64),
        _ => f64_to_value(ty, value_to_f64(v)),
    }
}

// ── JS ↔ WASM value bridge (shared by the export-call and global paths) ──────

/// Convert a runtime WASM value to the JS value carried across the boundary.
/// `i64` becomes a JS `BigInt` (W3C WebAssembly JS Interface §i64-to-BigInt);
/// all other types become `Number`.
pub(crate) fn wasm_value_to_js<'js>(ctx: &Ctx<'js>, v: Value) -> rquickjs::Value<'js> {
    match v {
        Value::I32(x) => rquickjs::Value::new_int(ctx.clone(), x),
        Value::I64(x) => rquickjs::Value::new_big_int(ctx.clone(), x),
        Value::F32(x) => rquickjs::Value::new_float(ctx.clone(), x as f64),
        Value::F64(x) => rquickjs::Value::new_float(ctx.clone(), x),
        Value::FuncRef(r) | Value::ExternRef(r) => {
            rquickjs::Value::new_float(ctx.clone(), r.map(f64::from).unwrap_or(-1.0))
        }
        // v128 has no JS Number/BigInt mapping; surface 0 rather than throw.
        Value::V128(_) => rquickjs::Value::new_float(ctx.clone(), 0.0),
    }
}

/// Coerce an incoming JS value to a typed WASM value for `ty`. An `i64`
/// parameter accepts a `BigInt` (read exactly) and tolerates a plain `Number`;
/// other types read the JS value as `f64`.
pub(crate) fn js_value_to_wasm(v: &rquickjs::Value, ty: ValType) -> Value {
    match ty {
        ValType::I64 => Value::I64(js_value_to_i64(v)),
        _ => f64_to_value(ty, js_value_to_f64(v)),
    }
}

/// Read a JS value as `i64`, accepting a `BigInt` exactly and falling back to
/// numeric truncation for a plain `Number`.
pub(crate) fn js_value_to_i64(v: &rquickjs::Value) -> i64 {
    if v.is_big_int()
        && let Some(b) = v.clone().into_big_int()
        && let Ok(x) = b.to_i64()
    {
        return x;
    }
    js_value_to_f64(v) as i64
}

/// Read a JS value as `f64`, tolerating a `BigInt` (down-converted, may lose
/// precision — the caller only takes this path for non-`i64` types).
pub(crate) fn js_value_to_f64(v: &rquickjs::Value) -> f64 {
    if let Some(n) = v.as_number() {
        return n;
    }
    if let Some(i) = v.as_int() {
        return f64::from(i);
    }
    if v.is_big_int()
        && let Some(b) = v.clone().into_big_int()
        && let Ok(x) = b.to_i64()
    {
        return x as f64;
    }
    0.0
}

/// Number of parameters for an exported function index (used by the shim to
/// size argument arrays if needed).
pub fn func_param_count(module_id: u32, func_idx: u32) -> u32 {
    with_module(module_id, |m| {
        m.func_type(func_idx).map(|t| t.params.len() as u32).unwrap_or(0)
    })
    .unwrap_or(0)
}

/// Helper kept for symmetry / external typing; converts a [`FuncType`] result
/// arity to a count.
#[allow(dead_code)]
fn result_count(ft: &FuncType) -> usize {
    ft.results.len()
}

#[cfg(test)]
mod tests;
