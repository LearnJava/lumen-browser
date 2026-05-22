/// RGBA color used by the Canvas 2D API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl CanvasColor {
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

    /// Parse a CSS color string.  Supports:
    /// `#rrggbb`, `#rgb`, `rgb(r,g,b)`, `rgba(r,g,b,a)`, named colors.
    pub fn from_css_str(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix('#') {
            return parse_hex(hex);
        }
        let sl = s.to_ascii_lowercase();
        if sl.starts_with("rgba(") {
            return parse_rgba_fn(&sl[5..sl.len() - 1]);
        }
        if sl.starts_with("rgb(") {
            return parse_rgb_fn(&sl[4..sl.len() - 1]);
        }
        named_color(sl.as_str())
    }
}

fn parse_hex(hex: &str) -> Option<CanvasColor> {
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some(CanvasColor::rgba(r * 17, g * 17, b * 17, 255))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(CanvasColor::rgba(r, g, b, 255))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(CanvasColor::rgba(r, g, b, a))
        }
        _ => None,
    }
}

fn parse_rgb_fn(inner: &str) -> Option<CanvasColor> {
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 { return None; }
    let r = parts[0].trim().parse::<u8>().ok()?;
    let g = parts[1].trim().parse::<u8>().ok()?;
    let b = parts[2].trim().parse::<u8>().ok()?;
    Some(CanvasColor::rgba(r, g, b, 255))
}

fn parse_rgba_fn(inner: &str) -> Option<CanvasColor> {
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 4 { return None; }
    let r = parts[0].trim().parse::<u8>().ok()?;
    let g = parts[1].trim().parse::<u8>().ok()?;
    let b = parts[2].trim().parse::<u8>().ok()?;
    let a_f: f32 = parts[3].trim().parse().ok()?;
    Some(CanvasColor::rgba(r, g, b, (a_f.clamp(0.0, 1.0) * 255.0) as u8))
}

fn named_color(name: &str) -> Option<CanvasColor> {
    let (r, g, b) = match name {
        "black"   => (0, 0, 0),
        "white"   => (255, 255, 255),
        "red"     => (255, 0, 0),
        "green"   => (0, 128, 0),
        "blue"    => (0, 0, 255),
        "yellow"  => (255, 255, 0),
        "cyan"    => (0, 255, 255),
        "magenta" => (255, 0, 255),
        "orange"  => (255, 165, 0),
        "purple"  => (128, 0, 128),
        "gray" | "grey" => (128, 128, 128),
        "silver"  => (192, 192, 192),
        "lime"    => (0, 255, 0),
        "navy"    => (0, 0, 128),
        "teal"    => (0, 128, 128),
        "maroon"  => (128, 0, 0),
        "olive"   => (128, 128, 0),
        "aqua"    => (0, 255, 255),
        "fuchsia" => (255, 0, 255),
        "transparent" => return Some(CanvasColor::rgba(0, 0, 0, 0)),
        _ => return None,
    };
    Some(CanvasColor::rgba(r, g, b, 255))
}
