//! Fixed-width SIMD (`v128`) execution for the WASM interpreter.
//!
//! Operates on the raw 16-byte vector carried by [`Value::V128`]. Lanes are
//! little-endian: lane `i` of a shape with lane size `SZ` occupies bytes
//! `i*SZ .. i*SZ+SZ`. Pure stack ops (splat, arithmetic, comparisons, bitwise,
//! shifts, conversions, lane shuffles) live here; memory loads/stores stay in
//! [`super::interp`] because they need the [`Instance`](super::interp::Instance)
//! linear memory.
//!
//! Coverage is the complete WebAssembly fixed-width SIMD opcode set (the `0xFD`
//! prefix, sub-opcodes 0..=255) plus relaxed-SIMD (sub-opcodes `0x100..=0x113`).
//! Relaxed-SIMD ops permit implementation-defined results in edge cases (NaN,
//! out-of-range swizzle indices, fused vs split multiply-add); we always pick
//! the strict/deterministic behaviour, which is a conforming choice.

use super::interp::Trap;
use super::value::Value;

/// Pop a value off the operand stack or trap on underflow.
fn pop(stack: &mut Vec<Value>) -> Result<Value, Trap> {
    stack.pop().ok_or_else(|| Trap::new("operand stack underflow"))
}

/// Pop a `v128` (raw 16 bytes) off the operand stack.
fn pop_v(stack: &mut Vec<Value>) -> Result<[u8; 16], Trap> {
    Ok(pop(stack)?.as_v128())
}

/// Saturating-truncate an `f64` to `i32` (NaN → 0), per `trunc_sat` semantics.
fn trunc_sat_i32(x: f64) -> i32 {
    if x.is_nan() {
        0
    } else if x <= i32::MIN as f64 {
        i32::MIN
    } else if x >= i32::MAX as f64 {
        i32::MAX
    } else {
        x as i32
    }
}

/// Saturating-truncate an `f64` to `u32` (NaN/negative → 0).
fn trunc_sat_u32(x: f64) -> u32 {
    if x.is_nan() || x <= 0.0 {
        0
    } else if x >= u32::MAX as f64 {
        u32::MAX
    } else {
        x as u32
    }
}

/// IEEE-754 `min` with WASM lane semantics: NaN propagates, `min(-0,+0) = -0`.
fn f32_min(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == b {
        f32::from_bits(a.to_bits() | b.to_bits())
    } else if a < b {
        a
    } else {
        b
    }
}

/// IEEE-754 `max` with WASM lane semantics: NaN propagates, `max(-0,+0) = +0`.
fn f32_max(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == b {
        f32::from_bits(a.to_bits() & b.to_bits())
    } else if a > b {
        a
    } else {
        b
    }
}

/// IEEE-754 `min` for `f64` lanes (see [`f32_min`]).
fn f64_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        f64::from_bits(a.to_bits() | b.to_bits())
    } else if a < b {
        a
    } else {
        b
    }
}

/// IEEE-754 `max` for `f64` lanes (see [`f32_max`]).
fn f64_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        f64::from_bits(a.to_bits() & b.to_bits())
    } else if a > b {
        a
    } else {
        b
    }
}

/// `i8x16.shuffle`: pick 16 lanes from the concatenation of `a` (lanes 0..15)
/// and `b` (lanes 16..31) using the immediate `lanes` indices.
pub fn shuffle(lanes: &[u8; 16], stack: &mut Vec<Value>) -> Result<(), Trap> {
    let b = pop_v(stack)?;
    let a = pop_v(stack)?;
    let mut r = [0u8; 16];
    for (i, &idx) in lanes.iter().enumerate() {
        r[i] = if idx < 16 {
            a[idx as usize]
        } else {
            b[(idx - 16) as usize]
        };
    }
    stack.push(Value::V128(r));
    Ok(())
}

/// `*.extract_lane*` / `*.replace_lane` (`0xFD` sub-opcodes 21..=34).
pub fn lane_op(sub: u32, lane: u8, stack: &mut Vec<Value>) -> Result<(), Trap> {
    let l = lane as usize;
    macro_rules! extract {
        ($ty:ty, $sz:expr, $n:expr, $val:expr) => {{
            let v = pop_v(stack)?;
            if l >= $n {
                return Err(Trap::new("lane index out of range"));
            }
            let x = <$ty>::from_le_bytes(v[l * $sz..l * $sz + $sz].try_into().unwrap());
            stack.push($val(x));
        }};
    }
    macro_rules! replace {
        ($ty:ty, $sz:expr, $n:expr, $scalar:expr) => {{
            let s: $ty = $scalar(pop(stack)?);
            let mut v = pop_v(stack)?;
            if l >= $n {
                return Err(Trap::new("lane index out of range"));
            }
            v[l * $sz..l * $sz + $sz].copy_from_slice(&s.to_le_bytes());
            stack.push(Value::V128(v));
        }};
    }
    match sub {
        21 => extract!(i8, 1, 16, |x: i8| Value::I32(x as i32)), // extract_lane_s
        22 => extract!(u8, 1, 16, |x: u8| Value::I32(x as i32)), // extract_lane_u
        23 => replace!(u8, 1, 16, |v: Value| v.as_i32() as u8),
        24 => extract!(i16, 2, 8, |x: i16| Value::I32(x as i32)),
        25 => extract!(u16, 2, 8, |x: u16| Value::I32(x as i32)),
        26 => replace!(u16, 2, 8, |v: Value| v.as_i32() as u16),
        27 => extract!(i32, 4, 4, Value::I32),
        28 => replace!(i32, 4, 4, |v: Value| v.as_i32()),
        29 => extract!(i64, 8, 2, Value::I64),
        30 => replace!(i64, 8, 2, |v: Value| v.as_i64()),
        31 => extract!(f32, 4, 4, Value::F32),
        32 => replace!(f32, 4, 4, |v: Value| v.as_f32()),
        33 => extract!(f64, 8, 2, Value::F64),
        34 => replace!(f64, 8, 2, |v: Value| v.as_f64()),
        _ => return Err(Trap::new("bad SIMD lane op")),
    }
    Ok(())
}

/// Execute a SIMD op with no immediate beyond the sub-opcode (the `Instr::Simd`
/// catch-all). Returns `Err` for an unsupported/unknown sub-opcode so it traps
/// rather than silently producing a wrong result.
#[allow(clippy::too_many_lines)]
pub fn exec_simd(sub: u32, stack: &mut Vec<Value>) -> Result<(), Trap> {
    // Lane-wise binary op over `$n` lanes of type `$ty`.
    macro_rules! bin {
        ($ty:ty, $sz:expr, $n:expr, $f:expr) => {{
            let b = pop_v(stack)?;
            let a = pop_v(stack)?;
            let mut r = [0u8; 16];
            for i in 0..$n {
                let av = <$ty>::from_le_bytes(a[i * $sz..i * $sz + $sz].try_into().unwrap());
                let bv = <$ty>::from_le_bytes(b[i * $sz..i * $sz + $sz].try_into().unwrap());
                let rv: $ty = ($f)(av, bv);
                r[i * $sz..i * $sz + $sz].copy_from_slice(&rv.to_le_bytes());
            }
            stack.push(Value::V128(r));
        }};
    }
    // Lane-wise unary op.
    macro_rules! un {
        ($ty:ty, $sz:expr, $n:expr, $f:expr) => {{
            let a = pop_v(stack)?;
            let mut r = [0u8; 16];
            for i in 0..$n {
                let av = <$ty>::from_le_bytes(a[i * $sz..i * $sz + $sz].try_into().unwrap());
                let rv: $ty = ($f)(av);
                r[i * $sz..i * $sz + $sz].copy_from_slice(&rv.to_le_bytes());
            }
            stack.push(Value::V128(r));
        }};
    }
    // Lane-wise comparison producing an all-ones/all-zeros mask of width `$uty`.
    macro_rules! cmp {
        ($ty:ty, $uty:ty, $sz:expr, $n:expr, $f:expr) => {{
            let b = pop_v(stack)?;
            let a = pop_v(stack)?;
            let mut r = [0u8; 16];
            for i in 0..$n {
                let av = <$ty>::from_le_bytes(a[i * $sz..i * $sz + $sz].try_into().unwrap());
                let bv = <$ty>::from_le_bytes(b[i * $sz..i * $sz + $sz].try_into().unwrap());
                let rv: $uty = if ($f)(av, bv) { <$uty>::MAX } else { 0 };
                r[i * $sz..i * $sz + $sz].copy_from_slice(&rv.to_le_bytes());
            }
            stack.push(Value::V128(r));
        }};
    }
    // Lane-wise shift by a scalar `i32` count (popped first), reduced mod lane bits.
    macro_rules! shift {
        ($ty:ty, $sz:expr, $n:expr, $bits:expr, $f:expr) => {{
            let s = (pop(stack)?.as_i32() as u32) % $bits;
            let a = pop_v(stack)?;
            let mut r = [0u8; 16];
            for i in 0..$n {
                let av = <$ty>::from_le_bytes(a[i * $sz..i * $sz + $sz].try_into().unwrap());
                let rv: $ty = ($f)(av, s);
                r[i * $sz..i * $sz + $sz].copy_from_slice(&rv.to_le_bytes());
            }
            stack.push(Value::V128(r));
        }};
    }
    // Splat a scalar into every lane.
    macro_rules! splat {
        ($ty:ty, $sz:expr, $n:expr, $scalar:expr) => {{
            let s: $ty = $scalar;
            let mut r = [0u8; 16];
            let bytes = s.to_le_bytes();
            for i in 0..$n {
                r[i * $sz..i * $sz + $sz].copy_from_slice(&bytes);
            }
            stack.push(Value::V128(r));
        }};
    }
    // `all_true`: push i32 1 iff every lane is nonzero.
    macro_rules! all_true {
        ($sz:expr, $n:expr) => {{
            let a = pop_v(stack)?;
            let mut all = 1i32;
            for i in 0..$n {
                if a[i * $sz..i * $sz + $sz].iter().all(|&x| x == 0) {
                    all = 0;
                    break;
                }
            }
            stack.push(Value::I32(all));
        }};
    }
    // `bitmask`: gather the sign (high) bit of each lane into an i32.
    macro_rules! bitmask {
        ($ty:ty, $sz:expr, $n:expr) => {{
            let a = pop_v(stack)?;
            let mut m = 0i32;
            for i in 0..$n {
                let av = <$ty>::from_le_bytes(a[i * $sz..i * $sz + $sz].try_into().unwrap());
                if av < 0 {
                    m |= 1 << i;
                }
            }
            stack.push(Value::I32(m));
        }};
    }

    match sub {
        // ── splat ───────────────────────────────────────────────────────────
        15 => splat!(u8, 1, 16, pop(stack)?.as_i32() as u8),
        16 => splat!(u16, 2, 8, pop(stack)?.as_i32() as u16),
        17 => splat!(i32, 4, 4, pop(stack)?.as_i32()),
        18 => splat!(i64, 8, 2, pop(stack)?.as_i64()),
        19 => splat!(f32, 4, 4, pop(stack)?.as_f32()),
        20 => splat!(f64, 8, 2, pop(stack)?.as_f64()),

        // ── i8x16.swizzle ─────────────────────────────────────────────────────
        14 => {
            let idx = pop_v(stack)?;
            let a = pop_v(stack)?;
            let mut r = [0u8; 16];
            for i in 0..16 {
                let j = idx[i];
                r[i] = if j < 16 { a[j as usize] } else { 0 };
            }
            stack.push(Value::V128(r));
        }

        // ── i8x16 comparisons ─────────────────────────────────────────────────
        35 => cmp!(i8, u8, 1, 16, |a, b| a == b),
        36 => cmp!(i8, u8, 1, 16, |a, b| a != b),
        37 => cmp!(i8, u8, 1, 16, |a, b| a < b),
        38 => cmp!(u8, u8, 1, 16, |a, b| a < b),
        39 => cmp!(i8, u8, 1, 16, |a, b| a > b),
        40 => cmp!(u8, u8, 1, 16, |a, b| a > b),
        41 => cmp!(i8, u8, 1, 16, |a, b| a <= b),
        42 => cmp!(u8, u8, 1, 16, |a, b| a <= b),
        43 => cmp!(i8, u8, 1, 16, |a, b| a >= b),
        44 => cmp!(u8, u8, 1, 16, |a, b| a >= b),
        // ── i16x8 comparisons ─────────────────────────────────────────────────
        45 => cmp!(i16, u16, 2, 8, |a, b| a == b),
        46 => cmp!(i16, u16, 2, 8, |a, b| a != b),
        47 => cmp!(i16, u16, 2, 8, |a, b| a < b),
        48 => cmp!(u16, u16, 2, 8, |a, b| a < b),
        49 => cmp!(i16, u16, 2, 8, |a, b| a > b),
        50 => cmp!(u16, u16, 2, 8, |a, b| a > b),
        51 => cmp!(i16, u16, 2, 8, |a, b| a <= b),
        52 => cmp!(u16, u16, 2, 8, |a, b| a <= b),
        53 => cmp!(i16, u16, 2, 8, |a, b| a >= b),
        54 => cmp!(u16, u16, 2, 8, |a, b| a >= b),
        // ── i32x4 comparisons ─────────────────────────────────────────────────
        55 => cmp!(i32, u32, 4, 4, |a, b| a == b),
        56 => cmp!(i32, u32, 4, 4, |a, b| a != b),
        57 => cmp!(i32, u32, 4, 4, |a, b| a < b),
        58 => cmp!(u32, u32, 4, 4, |a, b| a < b),
        59 => cmp!(i32, u32, 4, 4, |a, b| a > b),
        60 => cmp!(u32, u32, 4, 4, |a, b| a > b),
        61 => cmp!(i32, u32, 4, 4, |a, b| a <= b),
        62 => cmp!(u32, u32, 4, 4, |a, b| a <= b),
        63 => cmp!(i32, u32, 4, 4, |a, b| a >= b),
        64 => cmp!(u32, u32, 4, 4, |a, b| a >= b),
        // ── f32x4 comparisons ─────────────────────────────────────────────────
        65 => cmp!(f32, u32, 4, 4, |a, b| a == b),
        66 => cmp!(f32, u32, 4, 4, |a, b| a != b),
        67 => cmp!(f32, u32, 4, 4, |a, b| a < b),
        68 => cmp!(f32, u32, 4, 4, |a, b| a > b),
        69 => cmp!(f32, u32, 4, 4, |a, b| a <= b),
        70 => cmp!(f32, u32, 4, 4, |a, b| a >= b),
        // ── f64x2 comparisons ─────────────────────────────────────────────────
        71 => cmp!(f64, u64, 8, 2, |a, b| a == b),
        72 => cmp!(f64, u64, 8, 2, |a, b| a != b),
        73 => cmp!(f64, u64, 8, 2, |a, b| a < b),
        74 => cmp!(f64, u64, 8, 2, |a, b| a > b),
        75 => cmp!(f64, u64, 8, 2, |a, b| a <= b),
        76 => cmp!(f64, u64, 8, 2, |a, b| a >= b),
        // ── i64x2 comparisons ─────────────────────────────────────────────────
        214 => cmp!(i64, u64, 8, 2, |a, b| a == b),
        215 => cmp!(i64, u64, 8, 2, |a, b| a != b),
        216 => cmp!(i64, u64, 8, 2, |a, b| a < b),
        217 => cmp!(i64, u64, 8, 2, |a, b| a > b),
        218 => cmp!(i64, u64, 8, 2, |a, b| a <= b),
        219 => cmp!(i64, u64, 8, 2, |a, b| a >= b),

        // ── v128 bitwise ──────────────────────────────────────────────────────
        77 => un!(u8, 1, 16, |a: u8| !a),
        78 => bin!(u8, 1, 16, |a: u8, b: u8| a & b),
        79 => bin!(u8, 1, 16, |a: u8, b: u8| a & !b),
        80 => bin!(u8, 1, 16, |a: u8, b: u8| a | b),
        81 => bin!(u8, 1, 16, |a: u8, b: u8| a ^ b),
        82 => {
            // bitselect(v1, v2, c) = (v1 & c) | (v2 & !c)
            let c = pop_v(stack)?;
            let v2 = pop_v(stack)?;
            let v1 = pop_v(stack)?;
            let mut r = [0u8; 16];
            for i in 0..16 {
                r[i] = (v1[i] & c[i]) | (v2[i] & !c[i]);
            }
            stack.push(Value::V128(r));
        }
        83 => {
            let a = pop_v(stack)?;
            stack.push(Value::I32(i32::from(a.iter().any(|&x| x != 0))));
        }

        // ── f32x4 / f64x2 rounding ────────────────────────────────────────────
        103 => un!(f32, 4, 4, |x: f32| x.ceil()),
        104 => un!(f32, 4, 4, |x: f32| x.floor()),
        105 => un!(f32, 4, 4, |x: f32| x.trunc()),
        106 => un!(f32, 4, 4, round_ties_even_f32),
        116 => un!(f64, 8, 2, |x: f64| x.ceil()),
        117 => un!(f64, 8, 2, |x: f64| x.floor()),
        122 => un!(f64, 8, 2, |x: f64| x.trunc()),
        148 => un!(f64, 8, 2, round_ties_even_f64),

        // ── i8x16 arithmetic ──────────────────────────────────────────────────
        96 => un!(i8, 1, 16, |x: i8| x.wrapping_abs()),
        97 => un!(i8, 1, 16, |x: i8| x.wrapping_neg()),
        98 => un!(u8, 1, 16, |x: u8| x.count_ones() as u8),
        99 => all_true!(1, 16),
        100 => bitmask!(i8, 1, 16),
        101 => narrow(stack, 2, true)?,  // i8x16.narrow_i16x8_s
        102 => narrow(stack, 2, false)?, // i8x16.narrow_i16x8_u
        107 => shift!(u8, 1, 16, 8, |x: u8, s| x.wrapping_shl(s)),
        108 => shift!(i8, 1, 16, 8, |x: i8, s| x >> s),
        109 => shift!(u8, 1, 16, 8, |x: u8, s| x >> s),
        110 => bin!(u8, 1, 16, |a: u8, b: u8| a.wrapping_add(b)),
        111 => bin!(i8, 1, 16, |a: i8, b: i8| a.saturating_add(b)),
        112 => bin!(u8, 1, 16, |a: u8, b: u8| a.saturating_add(b)),
        113 => bin!(u8, 1, 16, |a: u8, b: u8| a.wrapping_sub(b)),
        114 => bin!(i8, 1, 16, |a: i8, b: i8| a.saturating_sub(b)),
        115 => bin!(u8, 1, 16, |a: u8, b: u8| a.saturating_sub(b)),
        118 => bin!(i8, 1, 16, |a: i8, b: i8| a.min(b)),
        119 => bin!(u8, 1, 16, |a: u8, b: u8| a.min(b)),
        120 => bin!(i8, 1, 16, |a: i8, b: i8| a.max(b)),
        121 => bin!(u8, 1, 16, |a: u8, b: u8| a.max(b)),
        123 => bin!(u8, 1, 16, |a: u8, b: u8| ((a as u16 + b as u16 + 1) >> 1) as u8),

        // ── pairwise extending add ────────────────────────────────────────────
        124 => extadd_pairwise(stack, 1, true)?,   // i16x8.extadd_pairwise_i8x16_s
        125 => extadd_pairwise(stack, 1, false)?,  // i16x8.extadd_pairwise_i8x16_u
        126 => extadd_pairwise(stack, 2, true)?,   // i32x4.extadd_pairwise_i16x8_s
        127 => extadd_pairwise(stack, 2, false)?,  // i32x4.extadd_pairwise_i16x8_u

        // ── i16x8 arithmetic ──────────────────────────────────────────────────
        128 => un!(i16, 2, 8, |x: i16| x.wrapping_abs()),
        129 => un!(i16, 2, 8, |x: i16| x.wrapping_neg()),
        130 => bin!(i16, 2, 8, |a: i16, b: i16| {
            (((a as i32 * b as i32) + 0x4000) >> 15).clamp(i16::MIN as i32, i16::MAX as i32) as i16
        }),
        131 => all_true!(2, 8),
        132 => bitmask!(i16, 2, 8),
        133 => narrow(stack, 4, true)?,  // i16x8.narrow_i32x4_s
        134 => narrow(stack, 4, false)?, // i16x8.narrow_i32x4_u
        135 => extend(stack, 1, false, true)?,  // extend_low_i8x16_s
        136 => extend(stack, 1, true, true)?,   // extend_high_i8x16_s
        137 => extend(stack, 1, false, false)?, // extend_low_i8x16_u
        138 => extend(stack, 1, true, false)?,  // extend_high_i8x16_u
        139 => shift!(u16, 2, 8, 16, |x: u16, s| x.wrapping_shl(s)),
        140 => shift!(i16, 2, 8, 16, |x: i16, s| x >> s),
        141 => shift!(u16, 2, 8, 16, |x: u16, s| x >> s),
        142 => bin!(u16, 2, 8, |a: u16, b: u16| a.wrapping_add(b)),
        143 => bin!(i16, 2, 8, |a: i16, b: i16| a.saturating_add(b)),
        144 => bin!(u16, 2, 8, |a: u16, b: u16| a.saturating_add(b)),
        145 => bin!(u16, 2, 8, |a: u16, b: u16| a.wrapping_sub(b)),
        146 => bin!(i16, 2, 8, |a: i16, b: i16| a.saturating_sub(b)),
        147 => bin!(u16, 2, 8, |a: u16, b: u16| a.saturating_sub(b)),
        149 => bin!(u16, 2, 8, |a: u16, b: u16| a.wrapping_mul(b)),
        150 => bin!(i16, 2, 8, |a: i16, b: i16| a.min(b)),
        151 => bin!(u16, 2, 8, |a: u16, b: u16| a.min(b)),
        152 => bin!(i16, 2, 8, |a: i16, b: i16| a.max(b)),
        153 => bin!(u16, 2, 8, |a: u16, b: u16| a.max(b)),
        155 => bin!(u16, 2, 8, |a: u16, b: u16| ((a as u32 + b as u32 + 1) >> 1) as u16),
        156 => extmul(stack, 1, false, true)?,  // extmul_low_i8x16_s
        157 => extmul(stack, 1, true, true)?,   // extmul_high_i8x16_s
        158 => extmul(stack, 1, false, false)?, // extmul_low_i8x16_u
        159 => extmul(stack, 1, true, false)?,  // extmul_high_i8x16_u

        // ── i32x4 arithmetic ──────────────────────────────────────────────────
        160 => un!(i32, 4, 4, |x: i32| x.wrapping_abs()),
        161 => un!(i32, 4, 4, |x: i32| x.wrapping_neg()),
        163 => all_true!(4, 4),
        164 => bitmask!(i32, 4, 4),
        165 => extend(stack, 2, false, true)?,  // extend_low_i16x8_s
        166 => extend(stack, 2, true, true)?,   // extend_high_i16x8_s
        167 => extend(stack, 2, false, false)?, // extend_low_i16x8_u
        168 => extend(stack, 2, true, false)?,  // extend_high_i16x8_u
        171 => shift!(u32, 4, 4, 32, |x: u32, s| x.wrapping_shl(s)),
        172 => shift!(i32, 4, 4, 32, |x: i32, s| x >> s),
        173 => shift!(u32, 4, 4, 32, |x: u32, s| x >> s),
        174 => bin!(u32, 4, 4, |a: u32, b: u32| a.wrapping_add(b)),
        177 => bin!(u32, 4, 4, |a: u32, b: u32| a.wrapping_sub(b)),
        181 => bin!(u32, 4, 4, |a: u32, b: u32| a.wrapping_mul(b)),
        182 => bin!(i32, 4, 4, |a: i32, b: i32| a.min(b)),
        183 => bin!(u32, 4, 4, |a: u32, b: u32| a.min(b)),
        184 => bin!(i32, 4, 4, |a: i32, b: i32| a.max(b)),
        185 => bin!(u32, 4, 4, |a: u32, b: u32| a.max(b)),
        186 => dot_i16x8_s(stack)?,
        188 => extmul(stack, 2, false, true)?,  // extmul_low_i16x8_s
        189 => extmul(stack, 2, true, true)?,   // extmul_high_i16x8_s
        190 => extmul(stack, 2, false, false)?, // extmul_low_i16x8_u
        191 => extmul(stack, 2, true, false)?,  // extmul_high_i16x8_u

        // ── i64x2 arithmetic ──────────────────────────────────────────────────
        192 => un!(i64, 8, 2, |x: i64| x.wrapping_abs()),
        193 => un!(i64, 8, 2, |x: i64| x.wrapping_neg()),
        195 => all_true!(8, 2),
        196 => bitmask!(i64, 8, 2),
        199 => extend(stack, 4, false, true)?,  // extend_low_i32x4_s
        200 => extend(stack, 4, true, true)?,   // extend_high_i32x4_s
        201 => extend(stack, 4, false, false)?, // extend_low_i32x4_u
        202 => extend(stack, 4, true, false)?,  // extend_high_i32x4_u
        203 => shift!(u64, 8, 2, 64, |x: u64, s| x.wrapping_shl(s)),
        204 => shift!(i64, 8, 2, 64, |x: i64, s| x >> s),
        205 => shift!(u64, 8, 2, 64, |x: u64, s| x >> s),
        206 => bin!(u64, 8, 2, |a: u64, b: u64| a.wrapping_add(b)),
        209 => bin!(u64, 8, 2, |a: u64, b: u64| a.wrapping_sub(b)),
        213 => bin!(u64, 8, 2, |a: u64, b: u64| a.wrapping_mul(b)),
        220 => extmul(stack, 4, false, true)?,  // extmul_low_i32x4_s
        221 => extmul(stack, 4, true, true)?,   // extmul_high_i32x4_s
        222 => extmul(stack, 4, false, false)?, // extmul_low_i32x4_u
        223 => extmul(stack, 4, true, false)?,  // extmul_high_i32x4_u

        // ── f32x4 arithmetic ──────────────────────────────────────────────────
        224 => un!(f32, 4, 4, |x: f32| x.abs()),
        225 => un!(f32, 4, 4, |x: f32| -x),
        227 => un!(f32, 4, 4, |x: f32| x.sqrt()),
        228 => bin!(f32, 4, 4, |a: f32, b: f32| a + b),
        229 => bin!(f32, 4, 4, |a: f32, b: f32| a - b),
        230 => bin!(f32, 4, 4, |a: f32, b: f32| a * b),
        231 => bin!(f32, 4, 4, |a: f32, b: f32| a / b),
        232 => bin!(f32, 4, 4, f32_min),
        233 => bin!(f32, 4, 4, f32_max),
        234 => bin!(f32, 4, 4, |a: f32, b: f32| if b < a { b } else { a }),
        235 => bin!(f32, 4, 4, |a: f32, b: f32| if a < b { b } else { a }),

        // ── f64x2 arithmetic ──────────────────────────────────────────────────
        236 => un!(f64, 8, 2, |x: f64| x.abs()),
        237 => un!(f64, 8, 2, |x: f64| -x),
        239 => un!(f64, 8, 2, |x: f64| x.sqrt()),
        240 => bin!(f64, 8, 2, |a: f64, b: f64| a + b),
        241 => bin!(f64, 8, 2, |a: f64, b: f64| a - b),
        242 => bin!(f64, 8, 2, |a: f64, b: f64| a * b),
        243 => bin!(f64, 8, 2, |a: f64, b: f64| a / b),
        244 => bin!(f64, 8, 2, f64_min),
        245 => bin!(f64, 8, 2, f64_max),
        246 => bin!(f64, 8, 2, |a: f64, b: f64| if b < a { b } else { a }),
        247 => bin!(f64, 8, 2, |a: f64, b: f64| if a < b { b } else { a }),

        // ── relaxed-SIMD (0x100..=0x113) ──────────────────────────────────────
        0x100..=0x113 => return exec_simd_relaxed(sub, stack),

        // ── conversions (94, 95, 248..=255) + any unknown sub ─────────────────
        _ => return exec_simd_convert(sub, stack),
    }
    Ok(())
}

/// Relaxed-SIMD ops (`0xFD` sub-opcodes `0x100..=0x113`). The spec permits
/// implementation-defined results in their edge cases; we always compute the
/// strict/deterministic variant, which is a conforming choice. Where a relaxed
/// op has an exact strict counterpart we delegate to [`exec_simd`] /
/// [`exec_simd_convert`] to avoid duplicating lane logic.
fn exec_simd_relaxed(sub: u32, stack: &mut Vec<Value>) -> Result<(), Trap> {
    // Fused multiply-add over `$n` float lanes of width `$sz`. Operands on the
    // stack are `a`, `b`, `c` (c on top); `neg` negates the product (nmadd).
    macro_rules! fma {
        ($ty:ty, $sz:expr, $n:expr, $neg:expr) => {{
            let c = pop_v(stack)?;
            let b = pop_v(stack)?;
            let a = pop_v(stack)?;
            let mut r = [0u8; 16];
            for i in 0..$n {
                let av = <$ty>::from_le_bytes(a[i * $sz..i * $sz + $sz].try_into().unwrap());
                let bv = <$ty>::from_le_bytes(b[i * $sz..i * $sz + $sz].try_into().unwrap());
                let cv = <$ty>::from_le_bytes(c[i * $sz..i * $sz + $sz].try_into().unwrap());
                let prod: $ty = if $neg { -(av * bv) } else { av * bv };
                let rv: $ty = prod + cv;
                r[i * $sz..i * $sz + $sz].copy_from_slice(&rv.to_le_bytes());
            }
            stack.push(Value::V128(r));
        }};
    }
    match sub {
        0x100 => return exec_simd(14, stack),          // i8x16.relaxed_swizzle ≡ swizzle
        0x101 => return exec_simd_convert(248, stack), // i32x4.relaxed_trunc_f32x4_s
        0x102 => return exec_simd_convert(249, stack), // i32x4.relaxed_trunc_f32x4_u
        0x103 => return exec_simd_convert(252, stack), // i32x4.relaxed_trunc_f64x2_s_zero
        0x104 => return exec_simd_convert(253, stack), // i32x4.relaxed_trunc_f64x2_u_zero
        0x105 => fma!(f32, 4, 4, false),               // f32x4.relaxed_madd
        0x106 => fma!(f32, 4, 4, true),                // f32x4.relaxed_nmadd
        0x107 => fma!(f64, 8, 2, false),               // f64x2.relaxed_madd
        0x108 => fma!(f64, 8, 2, true),                // f64x2.relaxed_nmadd
        // relaxed_laneselect(a, b, m) ≡ bitselect(a, b, m): (a & m) | (b & !m).
        // Identical bytewise for every lane width, so reuse bitselect (sub 82).
        0x109..=0x10C => return exec_simd(82, stack),
        0x10D => return exec_simd(232, stack), // f32x4.relaxed_min ≡ f32x4.min
        0x10E => return exec_simd(233, stack), // f32x4.relaxed_max ≡ f32x4.max
        0x10F => return exec_simd(244, stack), // f64x2.relaxed_min ≡ f64x2.min
        0x110 => return exec_simd(245, stack), // f64x2.relaxed_max ≡ f64x2.max
        0x111 => return exec_simd(130, stack), // i16x8.relaxed_q15mulr_s ≡ q15mulr_sat_s
        0x112 => relaxed_dot_i16x8(stack)?,     // i16x8.relaxed_dot_i8x16_i7x16_s
        0x113 => relaxed_dot_i32x4_add(stack)?, // i32x4.relaxed_dot_i8x16_i7x16_add_s
        _ => return Err(Trap::new("unsupported relaxed-SIMD sub-opcode")),
    }
    Ok(())
}

/// `i16x8.relaxed_dot_i8x16_i7x16_s`: multiply signed 8-bit lane pairs, sum each
/// adjacent pair into an i16 lane with signed saturation. The second operand is
/// nominally i7-ranged; reading it as a signed i8 is a conforming relaxed choice.
fn relaxed_dot_i16x8(stack: &mut Vec<Value>) -> Result<(), Trap> {
    let b = pop_v(stack)?;
    let a = pop_v(stack)?;
    let mut r = [0u8; 16];
    for i in 0..8 {
        let a0 = a[2 * i] as i8 as i32;
        let a1 = a[2 * i + 1] as i8 as i32;
        let b0 = b[2 * i] as i8 as i32;
        let b1 = b[2 * i + 1] as i8 as i32;
        let v = (a0 * b0 + a1 * b1).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        r[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
    }
    stack.push(Value::V128(r));
    Ok(())
}

/// `i32x4.relaxed_dot_i8x16_i7x16_add_s`: the i16x8 dot above, then widen and
/// pairwise-accumulate into the i32x4 operand `c`. Operands: `a`, `b`, `c`.
fn relaxed_dot_i32x4_add(stack: &mut Vec<Value>) -> Result<(), Trap> {
    let c = pop_v(stack)?;
    let b = pop_v(stack)?;
    let a = pop_v(stack)?;
    // Intermediate i16x8 dot (signed-saturating), same as relaxed_dot_i16x8.
    let mut tmp = [0i32; 8];
    for (i, slot) in tmp.iter_mut().enumerate() {
        let a0 = a[2 * i] as i8 as i32;
        let a1 = a[2 * i + 1] as i8 as i32;
        let b0 = b[2 * i] as i8 as i32;
        let b1 = b[2 * i + 1] as i8 as i32;
        *slot = (a0 * b0 + a1 * b1).clamp(i16::MIN as i32, i16::MAX as i32);
    }
    let mut r = [0u8; 16];
    for j in 0..4 {
        let cv = i32::from_le_bytes(c[j * 4..j * 4 + 4].try_into().unwrap());
        let v = cv.wrapping_add(tmp[2 * j]).wrapping_add(tmp[2 * j + 1]);
        r[j * 4..j * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    stack.push(Value::V128(r));
    Ok(())
}

/// Width-changing conversion ops, split out to keep [`exec_simd`] readable.
fn exec_simd_convert(sub: u32, stack: &mut Vec<Value>) -> Result<(), Trap> {
    match sub {
        // i32x4.trunc_sat_f32x4_s / _u
        248 => convert_4(stack, |a| {
            let mut r = [0u8; 16];
            for i in 0..4 {
                let x = f32::from_le_bytes(a[i * 4..i * 4 + 4].try_into().unwrap()) as f64;
                r[i * 4..i * 4 + 4].copy_from_slice(&trunc_sat_i32(x).to_le_bytes());
            }
            r
        }),
        249 => convert_4(stack, |a| {
            let mut r = [0u8; 16];
            for i in 0..4 {
                let x = f32::from_le_bytes(a[i * 4..i * 4 + 4].try_into().unwrap()) as f64;
                r[i * 4..i * 4 + 4].copy_from_slice(&trunc_sat_u32(x).to_le_bytes());
            }
            r
        }),
        // f32x4.convert_i32x4_s / _u
        250 => convert_4(stack, |a| {
            let mut r = [0u8; 16];
            for i in 0..4 {
                let x = i32::from_le_bytes(a[i * 4..i * 4 + 4].try_into().unwrap());
                r[i * 4..i * 4 + 4].copy_from_slice(&(x as f32).to_le_bytes());
            }
            r
        }),
        251 => convert_4(stack, |a| {
            let mut r = [0u8; 16];
            for i in 0..4 {
                let x = u32::from_le_bytes(a[i * 4..i * 4 + 4].try_into().unwrap());
                r[i * 4..i * 4 + 4].copy_from_slice(&(x as f32).to_le_bytes());
            }
            r
        }),
        // i32x4.trunc_sat_f64x2_s_zero / _u_zero (lanes 2,3 = 0)
        252 => convert_4(stack, |a| {
            let mut r = [0u8; 16];
            for i in 0..2 {
                let x = f64::from_le_bytes(a[i * 8..i * 8 + 8].try_into().unwrap());
                r[i * 4..i * 4 + 4].copy_from_slice(&trunc_sat_i32(x).to_le_bytes());
            }
            r
        }),
        253 => convert_4(stack, |a| {
            let mut r = [0u8; 16];
            for i in 0..2 {
                let x = f64::from_le_bytes(a[i * 8..i * 8 + 8].try_into().unwrap());
                r[i * 4..i * 4 + 4].copy_from_slice(&trunc_sat_u32(x).to_le_bytes());
            }
            r
        }),
        // f64x2.convert_low_i32x4_s / _u
        254 => convert_4(stack, |a| {
            let mut r = [0u8; 16];
            for i in 0..2 {
                let x = i32::from_le_bytes(a[i * 4..i * 4 + 4].try_into().unwrap());
                r[i * 8..i * 8 + 8].copy_from_slice(&(x as f64).to_le_bytes());
            }
            r
        }),
        255 => convert_4(stack, |a| {
            let mut r = [0u8; 16];
            for i in 0..2 {
                let x = u32::from_le_bytes(a[i * 4..i * 4 + 4].try_into().unwrap());
                r[i * 8..i * 8 + 8].copy_from_slice(&(x as f64).to_le_bytes());
            }
            r
        }),
        // f32x4.demote_f64x2_zero (lanes 2,3 = 0)
        94 => convert_4(stack, |a| {
            let mut r = [0u8; 16];
            for i in 0..2 {
                let x = f64::from_le_bytes(a[i * 8..i * 8 + 8].try_into().unwrap());
                r[i * 4..i * 4 + 4].copy_from_slice(&(x as f32).to_le_bytes());
            }
            r
        }),
        // f64x2.promote_low_f32x4
        95 => convert_4(stack, |a| {
            let mut r = [0u8; 16];
            for i in 0..2 {
                let x = f32::from_le_bytes(a[i * 4..i * 4 + 4].try_into().unwrap());
                r[i * 8..i * 8 + 8].copy_from_slice(&(x as f64).to_le_bytes());
            }
            r
        }),
        _ => Err(Trap::new("unsupported SIMD sub-opcode")),
    }
}

/// Pop one `v128`, apply `f` to its raw bytes, push the result.
fn convert_4(stack: &mut Vec<Value>, f: impl Fn([u8; 16]) -> [u8; 16]) -> Result<(), Trap> {
    let a = pop_v(stack)?;
    stack.push(Value::V128(f(a)));
    Ok(())
}

/// Round to nearest, ties to even (matches `f32x4.nearest`).
fn round_ties_even_f32(x: f32) -> f32 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && (r as i64) % 2 != 0 {
        r - x.signum()
    } else {
        r
    }
}

/// Round to nearest, ties to even (matches `f64x2.nearest`).
fn round_ties_even_f64(x: f64) -> f64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && (r as i64) % 2 != 0 {
        r - x.signum()
    } else {
        r
    }
}

/// `narrow`: pack two source vectors (`a` then `b`) of `src_sz`-byte signed
/// lanes into a single vector of half-width lanes, saturating. `signed` selects
/// signed (→ i8/i16) vs unsigned (→ u8/u16) saturation.
fn narrow(stack: &mut Vec<Value>, src_sz: usize, signed: bool) -> Result<(), Trap> {
    let b = pop_v(stack)?;
    let a = pop_v(stack)?;
    let n = 16 / src_sz; // lanes per source vector
    let dst_sz = src_sz / 2;
    let mut r = [0u8; 16];
    let write = |out: &mut [u8; 16], lane: usize, src: [u8; 16]| {
        let v: i64 = match src_sz {
            2 => i16::from_le_bytes(src[(lane % n) * 2..(lane % n) * 2 + 2].try_into().unwrap())
                as i64,
            4 => i32::from_le_bytes(src[(lane % n) * 4..(lane % n) * 4 + 4].try_into().unwrap())
                as i64,
            _ => 0,
        };
        let off = lane * dst_sz;
        if dst_sz == 1 {
            let packed = if signed {
                v.clamp(i8::MIN as i64, i8::MAX as i64) as i8 as u8
            } else {
                v.clamp(0, u8::MAX as i64) as u8
            };
            out[off] = packed;
        } else {
            let packed = if signed {
                (v.clamp(i16::MIN as i64, i16::MAX as i64) as i16).to_le_bytes()
            } else {
                (v.clamp(0, u16::MAX as i64) as u16).to_le_bytes()
            };
            out[off..off + 2].copy_from_slice(&packed);
        }
    };
    for lane in 0..n {
        write(&mut r, lane, a);
    }
    for lane in 0..n {
        write(&mut r, n + lane, b);
    }
    stack.push(Value::V128(r));
    Ok(())
}

/// `extend_low`/`extend_high`: widen 8/4/2 lanes of `src_sz` bytes from the low
/// or high half of one vector into double-width lanes, sign- or zero-extending.
fn extend(stack: &mut Vec<Value>, src_sz: usize, high: bool, signed: bool) -> Result<(), Trap> {
    let a = pop_v(stack)?;
    let dst_sz = src_sz * 2;
    let n = 16 / dst_sz; // number of output lanes
    let base = if high { n * src_sz } else { 0 };
    let mut r = [0u8; 16];
    for i in 0..n {
        let off = base + i * src_sz;
        let v: i64 = read_int(&a, off, src_sz, signed);
        let dst = i * dst_sz;
        match dst_sz {
            2 => r[dst..dst + 2].copy_from_slice(&(v as i16).to_le_bytes()),
            4 => r[dst..dst + 4].copy_from_slice(&(v as i32).to_le_bytes()),
            8 => r[dst..dst + 8].copy_from_slice(&v.to_le_bytes()),
            _ => {}
        }
    }
    stack.push(Value::V128(r));
    Ok(())
}

/// `extmul_low`/`extmul_high`: multiply corresponding low- or high-half lanes
/// of two vectors with widening, producing double-width product lanes.
fn extmul(stack: &mut Vec<Value>, src_sz: usize, high: bool, signed: bool) -> Result<(), Trap> {
    let b = pop_v(stack)?;
    let a = pop_v(stack)?;
    let dst_sz = src_sz * 2;
    let n = 16 / dst_sz;
    let base = if high { n * src_sz } else { 0 };
    let mut r = [0u8; 16];
    for i in 0..n {
        let off = base + i * src_sz;
        let av = read_int(&a, off, src_sz, signed);
        let bv = read_int(&b, off, src_sz, signed);
        let prod = av.wrapping_mul(bv);
        let dst = i * dst_sz;
        match dst_sz {
            2 => r[dst..dst + 2].copy_from_slice(&(prod as i16).to_le_bytes()),
            4 => r[dst..dst + 4].copy_from_slice(&(prod as i32).to_le_bytes()),
            8 => r[dst..dst + 8].copy_from_slice(&prod.to_le_bytes()),
            _ => {}
        }
    }
    stack.push(Value::V128(r));
    Ok(())
}

/// `extadd_pairwise`: add adjacent lane pairs with widening (one operand).
fn extadd_pairwise(stack: &mut Vec<Value>, src_sz: usize, signed: bool) -> Result<(), Trap> {
    let a = pop_v(stack)?;
    let dst_sz = src_sz * 2;
    let n = 16 / dst_sz;
    let mut r = [0u8; 16];
    for i in 0..n {
        let lo = read_int(&a, (2 * i) * src_sz, src_sz, signed);
        let hi = read_int(&a, (2 * i + 1) * src_sz, src_sz, signed);
        let sum = lo + hi;
        let dst = i * dst_sz;
        match dst_sz {
            2 => r[dst..dst + 2].copy_from_slice(&(sum as i16).to_le_bytes()),
            4 => r[dst..dst + 4].copy_from_slice(&(sum as i32).to_le_bytes()),
            _ => {}
        }
    }
    stack.push(Value::V128(r));
    Ok(())
}

/// `i32x4.dot_i16x8_s`: signed multiply of i16 lanes, summed in pairs to i32.
fn dot_i16x8_s(stack: &mut Vec<Value>) -> Result<(), Trap> {
    let b = pop_v(stack)?;
    let a = pop_v(stack)?;
    let mut r = [0u8; 16];
    for i in 0..4 {
        let a0 = i16::from_le_bytes(a[i * 4..i * 4 + 2].try_into().unwrap()) as i32;
        let a1 = i16::from_le_bytes(a[i * 4 + 2..i * 4 + 4].try_into().unwrap()) as i32;
        let b0 = i16::from_le_bytes(b[i * 4..i * 4 + 2].try_into().unwrap()) as i32;
        let b1 = i16::from_le_bytes(b[i * 4 + 2..i * 4 + 4].try_into().unwrap()) as i32;
        let v = a0 * b0 + a1 * b1;
        r[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    stack.push(Value::V128(r));
    Ok(())
}

/// Read an integer lane of `sz` bytes at byte offset `off`, sign- or
/// zero-extended to `i64`.
fn read_int(v: &[u8; 16], off: usize, sz: usize, signed: bool) -> i64 {
    match (sz, signed) {
        (1, true) => v[off] as i8 as i64,
        (1, false) => v[off] as i64,
        (2, true) => i16::from_le_bytes(v[off..off + 2].try_into().unwrap()) as i64,
        (2, false) => u16::from_le_bytes(v[off..off + 2].try_into().unwrap()) as i64,
        (4, true) => i32::from_le_bytes(v[off..off + 4].try_into().unwrap()) as i64,
        (4, false) => u32::from_le_bytes(v[off..off + 4].try_into().unwrap()) as i64,
        _ => 0,
    }
}
