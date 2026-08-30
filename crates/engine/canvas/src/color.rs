/// RGBA color used by the Canvas 2D API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasColor {
    /// Красный канал, 0–255.
    pub r: u8,
    /// Зелёный канал, 0–255.
    pub g: u8,
    /// Синий канал, 0–255.
    pub b: u8,
    /// Альфа, 0 (прозрачно) – 255 (непрозрачно). Не premultiplied.
    pub a: u8,
}

impl CanvasColor {
    /// Собирает цвет из каналов 0–255 (альфа не premultiplied).
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Multiply `self.a` by `alpha` (0.0–1.0).
    pub fn with_alpha_mult(self, alpha: f32) -> Self {
        Self {
            a: (self.a as f32 * alpha.clamp(0.0, 1.0)) as u8,
            ..self
        }
    }

    /// Разбирает CSS-значение `<color>`.
    ///
    /// Своего парсера у Canvas 2D нет — это тонкая обёртка над
    /// [`lumen_layout::parse_color`], то есть над тем же кодом, которым цвет
    /// разбирает каскад: named-цвета (все 148), `#rgb`/`#rgba`/`#rrggbb`/
    /// `#rrggbbaa`, `rgb()`/`rgba()`/`hsl()`/`hsla()` в обеих формах (запятая
    /// и пробел, с `/ alpha` и процентами), `hwb()`, `lab()`/`lch()`/
    /// `oklab()`/`oklch()`, `color()`, `color-mix()`, относительные цвета.
    /// До BUG-451 здесь лежала своя копия на 60 строк, знавшая ровно
    /// `#rgb`/`#rrggbb`/`#rrggbbaa`, `rgb(,,)`/`rgba(,,,)` и 20 имён.
    ///
    /// `None` — значение не является цветом; вызывающий обязан **сохранить
    /// прежнее состояние** (HTML LS §4.12.5.1.3: невалидное значение
    /// игнорируется), а не откатываться в чёрный.
    ///
    /// `currentColor` возвращает `None`: разрешать его нужно в вычисленный
    /// `color` элемента `<canvas>`, а сюда этот контекст не доходит.
    pub fn from_css_str(s: &str) -> Option<Self> {
        let c = lumen_layout::parse_color(s.trim())?;
        Some(Self::rgba(c.r, c.g, c.b, c.a))
    }

    /// Сериализация для `fillStyle`/`strokeStyle`/`shadowColor` по
    /// HTML LS §4.12.5.1.3: непрозрачный цвет — `#rrggbb` строчными,
    /// полупрозрачный — `rgba(r, g, b, a)`.
    ///
    /// Альфа хранится байтом, а спека сериализует её числом 0–1, поэтому
    /// печатается **кратчайшая десятичная дробь, которая обратно даёт тот же
    /// байт**: 128 → `0.5` (а не `0.502` от прямого деления 128/255), 115 →
    /// `0.45`. Именно этого требуют `2d.fillStyle.get.halftransparent`
    /// (точное `0.5`) и `…get.semitransparent` (`/^0\.4\d+$/`).
    pub fn to_css_string(self) -> String {
        if self.a == 255 {
            return format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b);
        }
        format!("rgba({}, {}, {}, {})", self.r, self.g, self.b, serialize_alpha(self.a))
    }
}

/// Кратчайшая десятичная запись альфы в [0, 1], round-trip-ящая в байт `a`
/// по правилу разбора (`round(x * 255)`).
fn serialize_alpha(a: u8) -> String {
    for digits in 0..=3 {
        let s = format!("{:.*}", digits, f32::from(a) / 255.0);
        if let Ok(v) = s.parse::<f32>()
            && (v * 255.0).round() as u8 == a
        {
            return s;
        }
    }
    // Пять знаков хватает любому байту: шаг между соседними значениями
    // альфы — 1/255 ≈ 0.0039, то есть заведомо крупнее 1e-5.
    format!("{:.5}", f32::from(a) / 255.0)
}

#[cfg(test)]
mod tests {
    // Тела `#[test]` clippy.toml освобождает от unwrap/expect/panic, а корень
    // тест-модуля — нет (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;
    /// BUG-451: разбор ушёл в общий CSS-парсер движка, поэтому Canvas 2D
    /// принимает всё, что принимает каскад. Каждая строка — форма, которую
    /// прежний собственный парсер на 60 строк отвергал молча, продолжая
    /// рисовать ПРЕДЫДУЩИМ цветом.
    #[test]
    fn canvas_color_accepts_css_forms_the_own_parser_rejected() {
        let green = |s: &str| {
            let c = CanvasColor::from_css_str(s)
                .unwrap_or_else(|| panic!("{s} отвергнут"));
            assert_eq!((c.r, c.g, c.b), (0, 255, 0), "{s}");
            c
        };
        // Массовый синтаксис CSS 2.1, которого не было вовсе.
        green("hsl(120,100%,50%)");
        green("hsl(120 100% 50%)");
        assert_eq!(green("hsla(120, 100%, 50%, 0.5)").a, 128);
        // Пробельная форма rgb() и проценты — CSS Color 4 §6.
        green("rgb(0 255 0)");
        green("rgb(0% 100% 0%)");
        assert_eq!(green("rgb(0 255 0 / 50%)").a, 128);
        // Каналы вне диапазона клампятся, а не делают значение невалидным.
        green("rgb(-5, 300, 0)");
        // #rgba — четырёхзначный hex.
        assert_eq!(CanvasColor::from_css_str("#0f08").unwrap().a, 136);
        // Именованных цветов теперь все 148, а не 20.
        let c = CanvasColor::from_css_str("rebeccapurple").unwrap();
        assert_eq!((c.r, c.g, c.b), (102, 51, 153));
        // CSS Color 4/5.
        green("hwb(120 0% 0%)");
        green("color(srgb 0 1 0)");
        green("color-mix(in srgb, lime, lime)");
        assert!(CanvasColor::from_css_str("oklch(0.87 0.29 142)").is_some());
        assert!(CanvasColor::from_css_str("lab(50% 40 59.5)").is_some());
    }

    /// BUG-451: невалидное значение обязано остаться невалидным — вызывающий
    /// по HTML LS §4.12.5.1.3 сохраняет прежний цвет. `currentColor` тоже
    /// `None`: разрешать его нужно в вычисленный `color` элемента, а этого
    /// контекста у парсера нет.
    #[test]
    fn canvas_color_rejects_non_colors() {
        for s in ["not-a-color", "", "   ", "#", "#gg", "currentColor", "rgb(1,2)"] {
            assert!(CanvasColor::from_css_str(s).is_none(), "{s} принят");
        }
    }

    /// BUG-451: усечённая функциональная форма роняла собственный парсер
    /// (`&sl[4..sl.len() - 1]` при `len == 4`), а не-ASCII в hex рвал границу
    /// UTF-8 — обе строки приходили со страницы.
    #[test]
    fn canvas_color_does_not_panic_on_truncated_input() {
        for s in ["rgb(", "rgba(", "hsl(", "#±a", "rgb(1,2,3", "rgba()"] {
            assert!(CanvasColor::from_css_str(s).is_none(), "{s} принят");
        }
    }

    /// BUG-451: `fillStyle` сериализуется по HTML LS §4.12.5.1.3 —
    /// `#rrggbb` строчными для непрозрачного, `rgba(r, g, b, a)` иначе.
    /// Альфа печатается кратчайшей дробью, round-trip-ящей в тот же байт:
    /// этого требует `2d.fillStyle.get.halftransparent` (ровно `0.5`).
    #[test]
    fn canvas_color_serializes_canonically() {
        let ser = |s: &str| CanvasColor::from_css_str(s).unwrap().to_css_string();
        assert_eq!(ser("#0F0"), "#00ff00");
        assert_eq!(ser("#fa0"), "#ffaa00");
        assert_eq!(ser("lime"), "#00ff00");
        assert_eq!(ser("hsl(120,100%,50%)"), "#00ff00");
        assert_eq!(ser("rgba(255,255,255,0.5)"), "rgba(255, 255, 255, 0.5)");
        assert_eq!(ser("rgba(255,255,255,0.45)"), "rgba(255, 255, 255, 0.45)");
        assert_eq!(ser("transparent"), "rgba(0, 0, 0, 0)");
        // Каждый байт альфы обязан пережить круг «сериализация → разбор».
        for a in 0..=255u8 {
            let c = CanvasColor::rgba(1, 2, 3, a);
            let back = CanvasColor::from_css_str(&c.to_css_string()).unwrap();
            assert_eq!(back, c, "альфа {a} не пережила круг");
        }
    }
}
