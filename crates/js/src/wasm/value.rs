//! Core WebAssembly value and type definitions (MVP / WASM 1.0 core).
//!
//! Covers the four numeric types (`i32`/`i64`/`f32`/`f64`) plus the two
//! reference types (`funcref`/`externref`) needed for tables and `call_indirect`.

/// A WebAssembly value type.
///
/// Numeric types map directly onto Rust integers/floats; reference types are
/// represented at runtime by [`Value::FuncRef`] / [`Value::ExternRef`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValType {
    /// 32-bit integer.
    I32,
    /// 64-bit integer.
    I64,
    /// 32-bit IEEE-754 float.
    F32,
    /// 64-bit IEEE-754 float.
    F64,
    /// Function reference (`funcref`/`anyfunc`) — a table slot index or null.
    FuncRef,
    /// Opaque host reference (`externref`).
    ExternRef,
    /// 128-bit packed SIMD vector (`v128`). Lane interpretation is per-op; the
    /// runtime value ([`Value::V128`]) carries the raw little-endian 16 bytes.
    V128,
}

impl ValType {
    /// Decode a value type from its binary tag byte. Returns `None` for an
    /// unknown tag.
    pub fn from_byte(b: u8) -> Option<ValType> {
        Some(match b {
            0x7F => ValType::I32,
            0x7E => ValType::I64,
            0x7D => ValType::F32,
            0x7C => ValType::F64,
            0x7B => ValType::V128,
            0x70 => ValType::FuncRef,
            0x6F => ValType::ExternRef,
            _ => return None,
        })
    }

    /// The zero/default runtime value for this type (used to initialise locals).
    pub fn default_value(self) -> Value {
        match self {
            ValType::I32 => Value::I32(0),
            ValType::I64 => Value::I64(0),
            ValType::F32 => Value::F32(0.0),
            ValType::F64 => Value::F64(0.0),
            ValType::V128 => Value::V128([0; 16]),
            ValType::FuncRef => Value::FuncRef(None),
            ValType::ExternRef => Value::ExternRef(None),
        }
    }
}

/// A runtime WebAssembly value.
///
/// Floats are stored as their native Rust type; bit-exact reinterpretation
/// (e.g. `f32.reinterpret_i32`) is handled in the interpreter via `to_bits`.
#[derive(Clone, Copy, Debug)]
pub enum Value {
    /// 32-bit integer value.
    I32(i32),
    /// 64-bit integer value.
    I64(i64),
    /// 32-bit float value.
    F32(f32),
    /// 64-bit float value.
    F64(f64),
    /// Function reference: `Some(func_index)` or `None` (null).
    FuncRef(Option<u32>),
    /// Host reference: an opaque slot id into the JS-side extern table, or null.
    ExternRef(Option<u32>),
    /// 128-bit SIMD vector, stored as raw little-endian bytes. Lane access is
    /// done in the interpreter via `from_le_bytes`/`to_le_bytes` on slices.
    V128([u8; 16]),
}

impl Value {
    /// Interpret this value as `i32`, trapping representation is the caller's
    /// concern. Returns 0 for non-i32 values (callers only call this when the
    /// validated type guarantees i32).
    pub fn as_i32(self) -> i32 {
        match self {
            Value::I32(v) => v,
            _ => 0,
        }
    }

    /// Interpret this value as `i64`.
    pub fn as_i64(self) -> i64 {
        match self {
            Value::I64(v) => v,
            _ => 0,
        }
    }

    /// Interpret this value as `f32`.
    pub fn as_f32(self) -> f32 {
        match self {
            Value::F32(v) => v,
            _ => 0.0,
        }
    }

    /// Interpret this value as `f64`.
    pub fn as_f64(self) -> f64 {
        match self {
            Value::F64(v) => v,
            _ => 0.0,
        }
    }

    /// Interpret this value as the raw 16 bytes of a `v128`. Returns all-zero
    /// for non-`v128` values (callers only take this path when the validated
    /// type guarantees `v128`).
    pub fn as_v128(self) -> [u8; 16] {
        match self {
            Value::V128(b) => b,
            _ => [0; 16],
        }
    }

    /// The value type of this runtime value.
    pub fn val_type(self) -> ValType {
        match self {
            Value::I32(_) => ValType::I32,
            Value::I64(_) => ValType::I64,
            Value::F32(_) => ValType::F32,
            Value::F64(_) => ValType::F64,
            Value::V128(_) => ValType::V128,
            Value::FuncRef(_) => ValType::FuncRef,
            Value::ExternRef(_) => ValType::ExternRef,
        }
    }
}

/// A function signature: parameter types followed by result types.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FuncType {
    /// Parameter value types, in declaration order.
    pub params: Vec<ValType>,
    /// Result value types, in declaration order. MVP allows 0 or 1; multi-value
    /// (post-MVP) is permitted by this struct but not produced by the decoder
    /// unless the binary uses it.
    pub results: Vec<ValType>,
}

/// Min/max limits shared by memories and tables (in pages for memory, in
/// elements for tables). `max == None` means unbounded.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Minimum size (lower bound, also the initial size).
    pub min: u32,
    /// Optional maximum size.
    pub max: Option<u32>,
}
