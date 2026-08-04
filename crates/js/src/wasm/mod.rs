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
//! * Compiled modules live in a thread-local [`REGISTRY`] keyed by id, shared
//!   by both JS-engine backends (module data carries no backend-specific
//!   state). Live instances are backend-specific — see [`v8_bridge`], the
//!   only backend left after the S12b rquickjs removal.
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
//! * Imported functions are JS callables stored as a `v8::Global<v8::Function>`
//!   GC root and invoked from the interpreter through [`interp::HostImports`]
//!   (see `v8_bridge::JsHost`). Numeric arguments and results cross the
//!   boundary by type: `i64` rides as a JS `BigInt` (full 64-bit precision,
//!   per the W3C WebAssembly JS Interface), the rest as `Number`.

pub mod interp;
pub mod parser;
pub mod simd;
pub mod value;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use parser::{ExportKind, ImportKind, Module};
use value::FuncType;
#[cfg(feature = "v8-backend")]
use value::{ValType, Value};

/// Thread-local store of compiled modules, shared by both JS-engine backends
/// (only [`v8_bridge`] keeps its own live-instance registry now — see its
/// module doc comment).
#[derive(Default)]
struct Registry {
    next_module: u32,
    modules: HashMap<u32, Rc<Module>>,
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

/// Drop all compiled modules on this thread.
///
/// Historically also released rquickjs `Persistent` import handles held by
/// the QuickJS-backed instance registry (BUG-222); that registry was removed
/// in S12b-B17 along with the rquickjs WASM bridge, so this now only clears
/// the module cache. Still called from `QuickJsRuntime`'s thread teardown —
/// harmless there since no code path populates a `Persistent` anymore.
pub fn clear_registry() {
    REGISTRY.with(|r| r.borrow_mut().modules.clear());
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

// ── Value marshalling (used by the v8_bridge backend below) ────────────────

/// Convert a runtime value to the `f64` carried across the JS boundary.
#[cfg(feature = "v8-backend")]
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
#[cfg(feature = "v8-backend")]
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
#[cfg(feature = "v8-backend")]
fn coerce_value(ty: ValType, v: Value) -> Value {
    match (ty, v) {
        (ValType::I64, Value::I64(x)) => Value::I64(x),
        (ValType::I64, other) => Value::I64(value_to_f64(other) as i64),
        _ => f64_to_value(ty, value_to_f64(v)),
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

// ── V8 backend bridge (Ph3 V8 migration S9) ─────────────────────────────────
//
// The former QuickJS host-import model stored `rquickjs::Persistent<Function>`,
// restored from a live `Ctx` at call time (removed in S12b-B17 along with the
// rest of the rquickjs WASM bridge). V8's GC-root equivalent is
// `v8::Global<v8::Function>`, converted back to a `v8::Local` via
// `v8::Local::new(scope, &global)` whenever a live scope is available — which
// it always is here, since every entry point below runs inside a native
// function dispatched through `V8Inner::run`, which owns the scope for the
// call's whole duration.
//
// This is a separate thread-local registry from [`REGISTRY`] above: instance
// ids are not shared between the two backends (compiled `Module`s *are*
// shared, via [`with_module`], since `Module` carries no backend-specific
// state). In practice only one backend runs per JS thread, but keeping the
// instance stores fully separate avoids any cross-backend id confusion if
// both features are ever compiled into the same binary.
#[cfg(feature = "v8-backend")]
pub(crate) mod v8_bridge {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    use super::interp::{HostImports, Instance, Trap};
    use super::parser::{ImportKind, Module};
    use super::value::{ValType, Value};
    use super::{f64_to_value, with_module};

    /// A live V8-backed instance plus the JS functions resolving its imports
    /// (in func-import order). V8 twin of [`super::InstanceEntry`].
    struct InstanceEntry {
        instance: Instance,
        host_funcs: Vec<v8::Global<v8::Function>>,
    }

    /// Thread-local store of live V8-backed instances.
    #[derive(Default)]
    struct Registry {
        next_instance: u32,
        instances: HashMap<u32, InstanceEntry>,
    }

    thread_local! {
        static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
    }

    /// Drop all live V8-backed instances on this thread, releasing the
    /// `v8::Global` JS handles held for function imports.
    ///
    /// Must be called before the owning V8 isolate is disposed (mirrors
    /// [`super::clear_registry`]'s QuickJS teardown discipline) so the
    /// persistent handles are released while the isolate can still process
    /// the reset — see `v8_runtime.rs::v8_thread_main`.
    pub(crate) fn clear_registry() {
        REGISTRY.with(|r| r.borrow_mut().instances.clear());
    }

    /// Bridge implementing [`HostImports`] by calling stored JS functions
    /// through a live V8 scope.
    struct JsHost<'a, 's, 'i> {
        scope: &'a mut v8::PinScope<'s, 'i>,
        funcs: &'a [v8::Global<v8::Function>],
        module: Rc<Module>,
    }

    impl<'a, 's, 'i> HostImports for JsHost<'a, 's, 'i> {
        fn call_host(&mut self, import_index: usize, args: &[Value]) -> Result<Vec<Value>, Trap> {
            let global = self
                .funcs
                .get(import_index)
                .ok_or_else(|| Trap(format!("unresolved import {import_index}")))?;
            let func = v8::Local::new(self.scope, global);
            let recv: v8::Local<v8::Value> = v8::undefined(self.scope).into();
            let call_args: Vec<v8::Local<v8::Value>> = args
                .iter()
                .map(|v| wasm_value_to_v8(self.scope, *v))
                .collect();
            let ret = func
                .call(self.scope, recv, &call_args)
                .ok_or_else(|| Trap("import call threw".into()))?;

            let rtypes = self
                .module
                .func_type(import_index as u32)
                .map(|t| t.results.clone())
                .unwrap_or_default();
            match rtypes.first() {
                None => Ok(Vec::new()),
                // An `i64` result is read back exactly from a returned `BigInt`.
                Some(ValType::I64) => Ok(vec![Value::I64(v8_value_to_i64(self.scope, ret))]),
                Some(ty) => Ok(vec![f64_to_value(*ty, v8_value_to_f64(self.scope, ret))]),
            }
        }
    }

    /// Instantiate a compiled module against V8-backed host imports. V8 twin
    /// of [`super::instantiate`].
    pub(crate) fn instantiate(
        scope: &mut v8::PinScope,
        module_id: u32,
        host_funcs: Vec<v8::Global<v8::Function>>,
        imported_globals: Vec<f64>,
    ) -> Result<u32, String> {
        let module = with_module(module_id, Rc::clone).ok_or("unknown module")?;

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
            let mut host = JsHost {
                scope,
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

    /// Call an exported function on a V8-backed instance. V8 twin of
    /// [`super::call_typed`].
    pub(crate) fn call_typed(
        scope: &mut v8::PinScope,
        instance_id: u32,
        func_idx: u32,
        args: &[Value],
    ) -> Result<Vec<Value>, String> {
        // Take the entry out so re-entrant calls into a *different* instance
        // work; re-entry into the same instance returns an error.
        let mut entry = REGISTRY
            .with(|r| r.borrow_mut().instances.remove(&instance_id))
            .ok_or("unknown or busy instance")?;

        let module = entry.instance.module.clone();
        let result = {
            let mut host = JsHost {
                scope,
                funcs: &entry.host_funcs,
                module: module.clone(),
            };
            entry.instance.invoke(func_idx, args, &mut host, 0)
        };

        REGISTRY.with(|r| {
            r.borrow_mut().instances.insert(instance_id, entry);
        });

        result.map_err(|t| t.0)
    }

    /// Parameter/result types of an exported function, for a V8-backed instance.
    /// V8 twin of [`super::func_signature`].
    pub(crate) fn func_signature(
        instance_id: u32,
        func_idx: u32,
    ) -> Option<(Vec<ValType>, Vec<ValType>)> {
        REGISTRY.with(|r| {
            let r = r.borrow();
            let e = r.instances.get(&instance_id)?;
            let ft = e.instance.module.func_type(func_idx)?;
            Some((ft.params.clone(), ft.results.clone()))
        })
    }

    /// Current memory size (64 KiB pages) of a V8-backed instance.
    pub(crate) fn mem_size(instance_id: u32) -> u32 {
        REGISTRY.with(|r| {
            r.borrow()
                .instances
                .get(&instance_id)
                .map(|e| e.instance.mem_pages())
                .unwrap_or(0)
        })
    }

    /// Grow a V8-backed instance's memory by `delta` pages.
    pub(crate) fn mem_grow(instance_id: u32, delta: u32) -> i32 {
        REGISTRY.with(|r| {
            r.borrow_mut()
                .instances
                .get_mut(&instance_id)
                .map(|e| e.instance.mem_grow(delta))
                .unwrap_or(-1)
        })
    }

    /// Copy `len` bytes of a V8-backed instance's linear memory at `offset`.
    pub(crate) fn mem_read(instance_id: u32, offset: u32, len: u32) -> Vec<u8> {
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

    /// Write `bytes` into a V8-backed instance's linear memory at `offset`.
    pub(crate) fn mem_write(instance_id: u32, offset: u32, bytes: &[u8]) -> bool {
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

    /// Full linear-memory snapshot of a V8-backed instance.
    pub(crate) fn mem_read_all(instance_id: u32) -> Vec<u8> {
        REGISTRY.with(|r| {
            r.borrow()
                .instances
                .get(&instance_id)
                .map(|e| e.instance.memory.clone())
                .unwrap_or_default()
        })
    }

    /// Read an exported global's current value on a V8-backed instance.
    pub(crate) fn global_value(instance_id: u32, index: u32) -> Option<Value> {
        REGISTRY.with(|r| {
            r.borrow()
                .instances
                .get(&instance_id)
                .and_then(|e| e.instance.globals.get(index as usize).copied())
        })
    }

    /// Set a mutable exported global on a V8-backed instance.
    pub(crate) fn global_set_value(instance_id: u32, index: u32, v: Value) -> bool {
        REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            let Some(e) = r.instances.get_mut(&instance_id) else {
                return false;
            };
            let idx = index as usize;
            if idx >= e.instance.globals.len()
                || !e.instance.global_mut.get(idx).copied().unwrap_or(false)
            {
                return false;
            }
            let ty = e.instance.globals[idx].val_type();
            e.instance.globals[idx] = super::coerce_value(ty, v);
            true
        })
    }

    /// Convert a runtime WASM value to the V8 value carried across the
    /// boundary. Mirrors [`super::wasm_value_to_js`]: `i64` becomes a JS
    /// `BigInt`, everything else a `Number`.
    pub(crate) fn wasm_value_to_v8<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        v: Value,
    ) -> v8::Local<'s, v8::Value> {
        match v {
            Value::I32(x) => v8::Number::new(scope, f64::from(x)).into(),
            Value::I64(x) => v8::BigInt::new_from_i64(scope, x).into(),
            Value::F32(x) => v8::Number::new(scope, f64::from(x)).into(),
            Value::F64(x) => v8::Number::new(scope, x).into(),
            Value::FuncRef(r) | Value::ExternRef(r) => {
                v8::Number::new(scope, r.map(f64::from).unwrap_or(-1.0)).into()
            }
            // v128 has no JS Number/BigInt mapping; surface 0 rather than throw.
            Value::V128(_) => v8::Number::new(scope, 0.0).into(),
        }
    }

    /// Read a V8 value as `i64`, accepting a `BigInt` exactly and falling
    /// back to numeric truncation for a plain `Number`. Mirrors
    /// [`super::js_value_to_i64`].
    fn v8_value_to_i64(scope: &mut v8::PinScope, v: v8::Local<v8::Value>) -> i64 {
        if v.is_big_int()
            && let Ok(b) = v8::Local::<v8::BigInt>::try_from(v)
        {
            return b.i64_value().0;
        }
        v8_value_to_f64(scope, v) as i64
    }

    /// Read a V8 value as `f64`, tolerating a `BigInt` (down-converted).
    /// Mirrors [`super::js_value_to_f64`].
    fn v8_value_to_f64(scope: &mut v8::PinScope, v: v8::Local<v8::Value>) -> f64 {
        if v.is_big_int()
            && let Ok(b) = v8::Local::<v8::BigInt>::try_from(v)
        {
            return b.i64_value().0 as f64;
        }
        v.number_value(scope).unwrap_or(0.0)
    }

    /// Coerce an incoming V8 value to a typed WASM value for `ty`. Mirrors
    /// [`super::js_value_to_wasm`]: an `i64` parameter accepts a `BigInt`
    /// exactly and tolerates a plain `Number`; other types read as `f64`.
    pub(crate) fn v8_value_to_wasm(
        scope: &mut v8::PinScope,
        v: v8::Local<v8::Value>,
        ty: ValType,
    ) -> Value {
        match ty {
            ValType::I64 => Value::I64(v8_value_to_i64(scope, v)),
            _ => f64_to_value(ty, v8_value_to_f64(scope, v)),
        }
    }
}

#[cfg(test)]
mod tests;
