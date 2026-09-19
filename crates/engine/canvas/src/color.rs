use lumen_layout::ColorFloat;

/// RGBA color used by the Canvas 2D API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasColor {
    /// Красный канал, 0–255. Для `wide.is_some()` — тот же цвет, сведённый в
    /// sRGB (рисование остаётся байтовым независимо от исходного пространства).
    pub r: u8,
    /// Зелёный канал, 0–255.
    pub g: u8,
    /// Синий канал, 0–255.
    pub b: u8,
    /// Альфа, 0 (прозрачно) – 255 (непрозрачно). Не premultiplied.
    pub a: u8,
    /// Заполнено, когда значение разобрано из `color(<space> …)` (CSS Color
    /// L4 §10.1) — `to_css_string` сериализует именно его, сохраняя
    /// пространство и float-точность, а не гамут-маппит в `#rrggbb`/`rgb()`
    /// (BUG-930). `None` для всех остальных форм записи (hex/`rgb()`/named/…).
    pub wide: Option<ColorFloat>,
}

impl CanvasColor {
    /// Собирает цвет из каналов 0–255 (альфа не premultiplied). Всегда без
    /// wide-gamut контекста — используй [`Self::from_css_str`] для того,
    /// чтобы разобрать `color()` с сохранением пространства.
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a, wide: None }
    }

    /// Multiply `self.a` by `alpha` (0.0–1.0).
    pub fn with_alpha_mult(self, alpha: f32) -> Self {
        let alpha = alpha.clamp(0.0, 1.0);
        Self {
            a: (self.a as f32 * alpha) as u8,
            wide: self.wide.map(|w| ColorFloat { a: w.a * alpha, ..w }),
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
        let s = s.trim();
        // `color(<space> …)` — CSS Color L4 §10.1. Разобрать здесь ДО общего
        // `parse_color`, чтобы сохранить пространство и float-точность для
        // сериализации (BUG-930); рисование всё равно берёт сведённый sRGB.
        if let Some(wide) = lumen_layout::parse_color_function(s) {
            let srgb = wide.to_srgb_color();
            return Some(Self {
                r: srgb.r,
                g: srgb.g,
                b: srgb.b,
                a: srgb.a,
                wide: Some(wide),
            });
        }
        let c = lumen_layout::parse_color(s)?;
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
    ///
    /// `color(<space> …)`-значения (`self.wide`) — исключение из этого
    /// правила: CSS Color L4 §10.1 требует вернуть их в той же функциональной
    /// форме, а не гамут-маппить в `#rrggbb`/`rgba()` (BUG-930).
    pub fn to_css_string(self) -> String {
        if let Some(wide) = self.wide {
            return wide.to_css_string();
        }
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

    /// BUG-930: `color(<space> …)` обязан сериализоваться обратно в той же
    /// функциональной форме (CSS Color L4 §10.1), а не гамут-маппиться в
    /// `#rrggbb` — в отличие от hex/named/legacy-функций, которые уже
    /// покрыты `canvas_color_serializes_canonically` выше.
    #[test]
    fn wide_gamut_color_function_round_trips_through_serialization() {
        let ser = |s: &str| CanvasColor::from_css_str(s).unwrap().to_css_string();
        assert_eq!(ser("color(display-p3 0 1 0)"), "color(display-p3 0 1 0)");
        assert_eq!(ser("color(srgb 0.5 0 0.5)"), "color(srgb 0.5 0 0.5)");
        assert_eq!(ser("color(rec2020 1 1 1)"), "color(rec2020 1 1 1)");
        assert_eq!(
            ser("color(display-p3 0 1 0 / 0.5)"),
            "color(display-p3 0 1 0 / 0.5)"
        );
    }

    /// BUG-930 / WPT `2d.fillStyle.colormix`: `color-mix()` mixed `in srgb`
    /// uses the *predefined* `srgb` color space, not legacy sRGB notation, so
    /// the result must read back as `color(srgb …)` too — not a hex string.
    /// `color-mix()` in any other space is out of this bug's scope and stays
    /// unaffected (falls back to sRGB-byte serialization).
    #[test]
    fn color_mix_in_srgb_round_trips_through_functional_form() {
        let ser = |s: &str| CanvasColor::from_css_str(s).unwrap().to_css_string();
        assert_eq!(ser("color-mix(in srgb, red, blue)"), "color(srgb 0.5 0 0.5)");
        assert_eq!(
            ser("color-mix(in srgb, red, color(srgb 1 0 0))"),
            "color(srgb 1 0 0)"
        );
        // A different interpolation space is not this bug's scope — stays hex.
        assert!(ser("color-mix(in oklab, red, blue)").starts_with('#'));
    }

    /// BUG-930: рисование остаётся sRGB-байтовым независимо от исходного
    /// пространства — `r/g/b/a` гамут-мапплены сразу при разборе, `wide`
    /// несёт только контекст для сериализации.
    #[test]
    fn wide_gamut_color_still_gamut_maps_to_srgb_bytes_for_rendering() {
        let c = CanvasColor::from_css_str("color(display-p3 0 1 0)").unwrap();
        assert_eq!((c.r, c.g, c.b, c.a), (0, 255, 0, 255));
        assert!(c.wide.is_some());
    }
}
