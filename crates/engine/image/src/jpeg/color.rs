//! YCbCr → RGB конверсия (ITU-R BT.601 / JFIF §7).
//!
//! Уравнения JFIF:
//! ```text
//! R = Y + 1.402 (Cr - 128)
//! G = Y − 0.344136 (Cb - 128) − 0.714136 (Cr - 128)
//! B = Y + 1.772 (Cb - 128)
//! ```
//!
//! Реализация — целочисленная фиксированная точка ×65536 (16-bit shift)
//! для accuracy без больших чисел. Все коэффициенты предкомпилированы.

/// Один пиксель Y/Cb/Cr (0..255) → (R, G, B) в 0..255.
pub fn ycbcr_to_rgb(y: u8, cb: u8, cr: u8) -> (u8, u8, u8) {
    let y = i32::from(y) << 16;
    let cb = i32::from(cb) - 128;
    let cr = i32::from(cr) - 128;

    // Округлённые коэффициенты × 65536:
    //  1.402   → 91881
    //  0.344136 → 22554
    //  0.714136 → 46802
    //  1.772   → 116130
    let r = y + 91881 * cr;
    let g = y - 22554 * cb - 46802 * cr;
    let b = y + 116130 * cb;

    let to_byte = |v: i32| -> u8 {
        let rounded = (v + (1 << 15)) >> 16;
        rounded.clamp(0, 255) as u8
    };

    (to_byte(r), to_byte(g), to_byte(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_gray_maps_to_gray() {
        // Y=128, Cb=Cr=128 (нейтрал) → (128, 128, 128).
        let (r, g, b) = ycbcr_to_rgb(128, 128, 128);
        assert_eq!((r, g, b), (128, 128, 128));
    }

    #[test]
    fn black_and_white_endpoints() {
        // Y=0, Cb=Cr=128 → чёрный.
        let (r, g, b) = ycbcr_to_rgb(0, 128, 128);
        assert_eq!((r, g, b), (0, 0, 0));
        // Y=255, Cb=Cr=128 → белый.
        let (r, g, b) = ycbcr_to_rgb(255, 128, 128);
        assert_eq!((r, g, b), (255, 255, 255));
    }

    #[test]
    fn pure_red_via_chroma() {
        // Подгоним Cr-смещение так, чтобы R ≈ 255, G,B ≈ 0.
        // R = Y + 1.402·(Cr−128); G = Y − 0.714·(Cr−128).
        // При Y=76, Cr=255 (Δ=127): R ≈ 76 + 1.402·127 ≈ 254; G ≈ 76 − 0.714·127 ≈ 0.
        let (r, g, b) = ycbcr_to_rgb(76, 85, 255);
        // Допускаем ±2 из-за округлений.
        assert!(r >= 250, "красный канал должен быть ≥250, получили {r}");
        assert!(g <= 5, "зелёный должен быть ≈0, получили {g}");
        assert!(b <= 5, "синий должен быть ≈0, получили {b}");
    }

    #[test]
    fn clamping_works_for_out_of_range_chroma() {
        // R = Y + 1.402·(Cr−128); G = Y − 0.344·(Cb−128) − 0.714·(Cr−128);
        // B = Y + 1.772·(Cb−128). Y=255, Cb=255 (Δ=+127): B = 255 + 1.772·127 ≈
        // 480, выходит за 255 — clamp обязан вернуть 255.
        let (_, _, b) = ycbcr_to_rgb(255, 255, 128);
        assert_eq!(b, 255, "B при положительном overflow должен быть 255");
        // Y=0, Cb=255 (Δ=+127), Cr=128 — G = 0 − 0.344·127 ≈ −44, R = 0;
        // clamp снизу до 0.
        let (r, g, _) = ycbcr_to_rgb(0, 255, 128);
        assert_eq!(r, 0, "R при Y=0 и нейтральном Cr должен быть 0");
        assert_eq!(g, 0, "G при отрицательном underflow должен быть 0");
        // Дополнительные «нагрузочные» точки — просто не panic-ят.
        let _ = ycbcr_to_rgb(0, 0, 0);
        let _ = ycbcr_to_rgb(255, 255, 255);
    }
}
