//! WebAssembly MVP execution engine for Lumen (U-4 stage 1).
//!
//! A pure-Rust interpreter for the WASM 1.0 core instruction set (plus a few
//! common post-MVP ops: saturating truncation, `memory.copy`/`fill`, sign
//! extension, reference-null). No external WASM runtime dependency — consistent
//! with Lumen's "lightweight custom engine" principle.
//!
//! The [`webassembly`](crate::webassembly) JS shim drives this engine through
//! native `__lumen_wasm_*` bindings, so `WebAssembly.instantiate(...).exports`
//! produces functions that actually execute bytecode (previously empty stubs).
//!
//! ## Bridge model
//! * Modules and instances live in a thread-local [`REGISTRY`] keyed by id.
//! * Linear memory is authoritative in Rust; JS reads/writes it via copy
//!   helpers (`__lumen_wasm_mem_read`/`write`). The live-aliasing emscripten
//!   `HEAP32 = new Int32Array(memory.buffer)` pattern is therefore *not*
//!   coherent in this MVP (documented limitation).
//! * Imported functions are JS callables stored as [`Persistent`] and invoked
//!   from the interpreter through [`interp::HostImports`]. Numeric arguments
//!   cross the boundary as `f64` (so `i64` values beyond 2^53 lose precision —
//!   another documented MVP limitation).

pub mod interp;
pub mod parser;
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
            call_args
                .push_arg(value_to_f64(*v))
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
        if rtypes.is_empty() {
            Ok(Vec::new())
        } else {
            let n = ret.as_number().or_else(|| ret.as_int().map(f64::from)).unwrap_or(0.0);
            Ok(vec![f64_to_value(rtypes[0], n)])
        }
    }
}

/// Call an exported function by its function index.
///
/// `args` are positional `f64`s coerced to the function's parameter types; the
/// returned `f64`s are the coerced result values. Errors are runtime traps,
/// surfaced as `RuntimeError`.
pub fn call_f64(
    ctx: &Ctx,
    instance_id: u32,
    func_idx: u32,
    args: Vec<f64>,
) -> Result<Vec<f64>, String> {
    // Take the entry out so re-entrant calls into a *different* instance work;
    // re-entry into the same instance returns an error rather than panicking.
    let mut entry = REGISTRY
        .with(|r| r.borrow_mut().instances.remove(&instance_id))
        .ok_or("unknown or busy instance")?;

    let module = entry.instance.module.clone();
    let ftype = module.func_type(func_idx).cloned();
    let typed_args: Vec<Value> = match &ftype {
        Some(ft) => ft
            .params
            .iter()
            .enumerate()
            .map(|(i, ty)| f64_to_value(*ty, args.get(i).copied().unwrap_or(0.0)))
            .collect(),
        None => args.iter().map(|&v| Value::I32(v as i32)).collect(),
    };

    let result = {
        let mut host = JsHost {
            ctx,
            funcs: &entry.host_funcs,
            module: module.clone(),
        };
        entry.instance.invoke(func_idx, &typed_args, &mut host, 0)
    };

    // Reinsert before propagating any error.
    REGISTRY.with(|r| {
        r.borrow_mut().instances.insert(instance_id, entry);
    });

    match result {
        Ok(vals) => Ok(vals.iter().map(|v| value_to_f64(*v)).collect()),
        Err(t) => Err(t.0),
    }
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

/// Read an exported global's current value as `f64`.
pub fn global_get(instance_id: u32, index: u32) -> f64 {
    REGISTRY.with(|r| {
        r.borrow()
            .instances
            .get(&instance_id)
            .and_then(|e| e.instance.globals.get(index as usize).copied())
            .map(value_to_f64)
            .unwrap_or(0.0)
    })
}

/// Set a mutable exported global's value (coerced to its type). Returns `false`
/// if the index is invalid or the global is immutable.
pub fn global_set(instance_id: u32, index: u32, v: f64) -> bool {
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
        e.instance.globals[idx] = f64_to_value(ty, v);
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
    }
}

/// Coerce an incoming `f64` to a typed value for `ty`.
fn f64_to_value(ty: ValType, v: f64) -> Value {
    match ty {
        ValType::I32 => Value::I32(v as i64 as i32),
        ValType::I64 => Value::I64(v as i64),
        ValType::F32 => Value::F32(v as f32),
        ValType::F64 => Value::F64(v),
        ValType::FuncRef => Value::FuncRef(if v < 0.0 { None } else { Some(v as u32) }),
        ValType::ExternRef => Value::ExternRef(if v < 0.0 { None } else { Some(v as u32) }),
    }
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
