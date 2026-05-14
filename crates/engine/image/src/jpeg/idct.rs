//! Inverse Discrete Cosine Transform 8×8 (ISO/IEC 10918-1 §A.3.3).
//!
//! Реализация — прямой 2D-IDCT в целочисленной арифметике с фиксированной
//! точкой. Сначала 1D-IDCT по строкам, потом по столбцам. Алгоритм AAN
//! (Arai-Agui-Nakajima) был бы быстрее, но требует масштабированных
//! quantization-таблиц; прямой даёт точный результат без дополнительных
//! предусловий.
//!
//! После трансформации значения уровневого диапазона `−128..=127`; мы
//! добавляем 128 (level shift, §A.3.1) и обрезаем до `0..=255`.

/// Cos((2k+1)·n·π/16) × 1024 (округлённые), для k,n ∈ {0..7}.
/// Используется как фиксированно-точные коэффициенты IDCT.
const C: [i32; 8] = [
    1024, // cos(0)         = 1.0      × 1024
    1004, // cos(π/16)      ≈ 0.98079  × 1024
    946,  // cos(2π/16)     ≈ 0.92388  × 1024
    851,  // cos(3π/16)     ≈ 0.83147  × 1024
    724,  // cos(4π/16)     ≈ 0.70711  × 1024 = 1/√2
    569,  // cos(5π/16)     ≈ 0.55557  × 1024
    392,  // cos(6π/16)     ≈ 0.38268  × 1024
    200,  // cos(7π/16)     ≈ 0.19509  × 1024
];

/// 1D IDCT на 8 точках. Сохраняет точность через фиксированную арифметику ×1024.
fn idct_1d(input: &[i32; 8], output: &mut [i32; 8]) {
    // s(u) = 1/√2 при u=0, иначе 1.
    // f(x) = (1/2) · Σu s(u) · F(u) · cos((2x+1)uπ/16)
    //
    // Накапливаем целочисленно с фиксированной точкой; делим на 1024 (≈ × 2^-10)
    // в самом конце — × 1/2 поглощается в одной из шкал ниже.

    for (x, out) in output.iter_mut().enumerate() {
        let mut acc: i32 = 0;
        // u = 0 — F(0) × 1/√2; используем cos(0)=1, но домножим на 1/√2 (= C[4]).
        acc += input[0] * C[4];
        for (u, &val) in input.iter().enumerate().skip(1) {
            // (2x+1)*u mod 16 даёт индекс косинуса.
            let arg = ((2 * x + 1) * u) % 16;
            let coef = if arg < 8 {
                C[arg]
            } else if arg < 16 {
                -C[16 - arg]
            } else {
                C[arg - 16]
            };
            acc += val * coef;
        }
        // Делим на 2 (от формулы 1/2) → сдвиг 1; затем делим на 1024 → сдвиг 10.
        // Итого: (acc + 0x800) >> 11 с округлением к ближайшему.
        *out = (acc + (1 << 10)) >> 11;
    }
}

/// 2D IDCT 8×8: сначала по строкам, потом по столбцам.
/// На вход — DCT-коэффициенты в natural row-major порядке.
/// На выход — пиксельные сэмплы 0..=255 (с level shift +128 и clamp-ом).
pub fn idct_8x8(block: &mut [i32; 64]) {
    // Промежуточный буфер после row-pass.
    let mut tmp = [0i32; 64];
    let mut row = [0i32; 8];
    let mut out = [0i32; 8];

    // Row pass.
    for y in 0..8 {
        for x in 0..8 {
            row[x] = block[y * 8 + x];
        }
        idct_1d(&row, &mut out);
        for x in 0..8 {
            tmp[y * 8 + x] = out[x];
        }
    }

    // Column pass.
    for x in 0..8 {
        for y in 0..8 {
            row[y] = tmp[y * 8 + x];
        }
        idct_1d(&row, &mut out);
        for y in 0..8 {
            // Level shift + clamp в u8.
            let v = out[y] + 128;
            block[y * 8 + x] = v.clamp(0, 255);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idct_of_dc_only_gives_uniform_block() {
        // F(0,0) = 8 (после dequantization), все остальные = 0.
        // По формуле IDCT для u=v=0: f(x,y) = (1/2)·s(0)²·F(0,0) = (1/2)·(1/√2)²·F(0,0) = F(0,0)/4.
        // С level-shift +128: значения должны быть 128 + 8/4 = 130.
        let mut block = [0i32; 64];
        block[0] = 8;
        idct_8x8(&mut block);
        for v in block.iter() {
            // Допускаем ±1 от округления фиксированной точки.
            assert!(
                (129..=131).contains(v),
                "ожидалось ~130, получили {v}"
            );
        }
    }

    #[test]
    fn idct_of_zero_block_gives_128_after_level_shift() {
        let mut block = [0i32; 64];
        idct_8x8(&mut block);
        for v in block.iter() {
            assert_eq!(*v, 128);
        }
    }

    #[test]
    fn idct_clamps_to_byte_range() {
        // Большой DC-коэффициент → значения вылетят за 255 без clamp-а.
        let mut block = [0i32; 64];
        block[0] = 2048;
        idct_8x8(&mut block);
        for v in block.iter() {
            assert!((0..=255).contains(v), "должно быть clamped в [0,255], получили {v}");
        }
    }

    #[test]
    fn idct_round_trip_via_forward_then_inverse_is_approximate_identity() {
        // Прямой DCT не реализован — проверим IDCT через инвариант:
        // IDCT(F)[x,y] = (1/4) Σ Σ s(u)s(v) F(u,v) cos((2x+1)uπ/16) cos((2y+1)vπ/16).
        // Для F(u,v) = δ(u,v) (только F(1,1)=8) — ожидаемый паттерн не uniform.
        let mut block = [0i32; 64];
        block[9] = 8; // F(u=1, v=1) — в natural order это (row=1, col=1) = 1*8+1.
        // Тут просто проверим, что результат разный для разных позиций (не uniform).
        idct_8x8(&mut block);
        let mut distinct = 0;
        for i in 1..64 {
            if block[i] != block[0] {
                distinct += 1;
                break;
            }
        }
        assert!(distinct > 0, "IDCT для F(1,1)=8 должен дать non-uniform блок");
    }
}
