//! CSS Images L4 §5 — `image-set()` candidate selection algorithm.
//!
//! Provides typed parsing and DPR-based selection for the `image-set()` CSS
//! function (also accepted with the `-webkit-image-set()` vendor prefix).
//!
//! # CSS Images L4 §5 algorithm summary
//!
//! ```text
//! image-set( <image-set-option># )
//! <image-set-option> = [ <image> | <string> ] [ <resolution> || type(<string>) ]
//! ```
//!
//! 1. Discard candidates whose `type()` hint names a MIME type not supported by
//!    the current engine.  If no `type()` is given the option is always kept.
//! 2. Among remaining candidates, pick the one whose resolution is *closest* to
//!    the device pixel ratio (`dpr`). Ties are broken in favour of the higher
//!    resolution (sharper asset).
//! 3. If no candidate survives filtering, fall back to the raw value as-is.
//!
//! # Integration
//!
//! Paint calls [`select_image_set`] after retrieving the raw CSS value from
//! `BackgroundImage::Url`.  Layout calls it when computing intrinsic sizes of
//! images in `image-set()` context.  P4 wires this into `apply_declaration` for
//! all image-bearing properties.
//!
//! Phase 0: `SupportedTypes::all()` accepts every type — no actual decoding
//! capability check.  Phase 1: query the image codec registry.

/// A single parsed candidate inside an `image-set()` expression.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageSetOption {
    /// URL string with `url(…)` wrapper and surrounding quotes stripped.
    pub url: String,
    /// Resolution in device-pixel-ratio units (dppx). Default `1.0` when
    /// no resolution descriptor is present.
    pub resolution_dppx: f32,
    /// MIME type from `type("image/webp")` descriptor, if present.
    /// `None` means the candidate has no type constraint.
    pub mime_type: Option<String>,
}

/// Describes which MIME types the engine can decode.
///
/// Phase 0 implementation accepts everything.  Phase 1 should query the
/// `lumen-image` codec registry.
#[derive(Debug, Clone)]
pub struct SupportedTypes {
    /// When `true`, all MIME types are accepted (Phase 0 default).
    all: bool,
    /// Explicit allow-list used when `all == false`.
    types: Vec<String>,
}

impl SupportedTypes {
    /// Phase 0 — accept every MIME type unconditionally.
    #[must_use]
    pub fn all() -> Self {
        Self { all: true, types: Vec::new() }
    }

    /// Explicit list of accepted MIME types (case-insensitive comparison).
    #[must_use]
    pub fn from_list(types: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { all: false, types: types.into_iter().map(Into::into).collect() }
    }

    /// Returns `true` if `mime_type` is accepted.
    #[must_use]
    pub fn accepts(&self, mime_type: &str) -> bool {
        if self.all {
            return true;
        }
        let lower = mime_type.to_ascii_lowercase();
        self.types.iter().any(|t| t.to_ascii_lowercase() == lower)
    }
}

impl Default for SupportedTypes {
    fn default() -> Self {
        Self::all()
    }
}

// ── Internal parsing helpers ─────────────────────────────────────────────────

/// Returns `true` if `s` starts with `prefix` case-insensitively.
fn ci_starts_with(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len()
        && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// Strips the outer `image-set(…)` / `-webkit-image-set(…)` wrapper.
/// Returns the inner comma-separated option list, or `None` when `s` is not
/// an `image-set()` expression.
fn strip_wrapper(s: &str) -> Option<&str> {
    let s = s.trim();
    if !s.ends_with(')') {
        return None;
    }
    for prefix in ["image-set(", "-webkit-image-set("] {
        if ci_starts_with(s, prefix) {
            return Some(&s[prefix.len()..s.len() - 1]);
        }
    }
    None
}

/// Splits `s` on top-level commas — commas inside `(…)` or quotes are
/// preserved.  Returns subslices of `s`.
fn split_top_level(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut in_quote: Option<u8> = None;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        match in_quote {
            Some(q) => {
                if c == q {
                    in_quote = None;
                }
            }
            None => match c {
                b'"' | b'\'' => in_quote = Some(c),
                b'(' => depth += 1,
                b')' => depth -= 1,
                b',' if depth == 0 => {
                    parts.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            },
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

/// Strips matching surrounding single/double quotes.
fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Parses a `<resolution>` token into dppx.  Supports `x`/`dppx`, `dpi`,
/// `dpcm`.  Returns `None` when `s` contains no recognisable resolution.
fn parse_resolution_token(s: &str) -> Option<f32> {
    let tok = s.split_whitespace().next()?;
    let lower = tok.to_ascii_lowercase();
    let (num_str, factor) = if let Some(n) = lower.strip_suffix("dppx") {
        (n, 1.0_f32)
    } else if let Some(n) = lower.strip_suffix("dpcm") {
        (n, 2.54_f32 / 96.0)
    } else if let Some(n) = lower.strip_suffix("dpi") {
        (n, 1.0_f32 / 96.0)
    } else if let Some(n) = lower.strip_suffix('x') {
        (n, 1.0_f32)
    } else {
        return None;
    };
    let v: f32 = num_str.trim().parse().ok()?;
    Some(v * factor)
}

/// Extracts the MIME type string from a `type("image/webp")` descriptor.
/// Returns `None` when `s` does not start with `type(`.
fn parse_type_descriptor(s: &str) -> Option<&str> {
    let s = s.trim();
    if !ci_starts_with(s, "type(") || !s.ends_with(')') {
        return None;
    }
    Some(strip_quotes(&s[5..s.len() - 1]))
}

/// Parses one raw option token (e.g. `"a.png" 2x type("image/webp")`) into
/// an [`ImageSetOption`].  Returns `None` when no URL can be extracted.
fn parse_option(opt: &str) -> Option<ImageSetOption> {
    let opt = opt.trim();
    let bytes = opt.as_bytes();

    // Extract the URL token (first token: url(…) or quoted string or bare word).
    let (url, rest): (&str, &str) = if ci_starts_with(opt, "url(") {
        if let Some(close) = opt.find(')') {
            (strip_quotes(opt[4..close].trim()), opt[close + 1..].trim_start())
        } else {
            (strip_quotes(opt[4..].trim()), "")
        }
    } else if matches!(bytes.first(), Some(&b'"') | Some(&b'\'')) {
        let q = bytes[0] as char;
        if let Some(rel) = opt[1..].find(q) {
            (&opt[1..1 + rel], opt[1 + rel + 1..].trim_start())
        } else {
            (&opt[1..], "")
        }
    } else {
        match opt.find(char::is_whitespace) {
            Some(sp) => (&opt[..sp], opt[sp..].trim_start()),
            None => (opt, ""),
        }
    };

    if url.is_empty() {
        return None;
    }

    // Scan remaining tokens for a resolution descriptor and a type() descriptor.
    // CSS Images L4 allows them in either order after the URL.
    let mut resolution_dppx = 1.0_f32;
    let mut mime_type: Option<String> = None;
    let mut remaining = rest;
    while !remaining.is_empty() {
        // Try type() first (starts with 'type(').
        if ci_starts_with(remaining, "type(") && let Some(close) = remaining.find(')') {
            let descriptor = &remaining[..close + 1];
            if let Some(mt) = parse_type_descriptor(descriptor) {
                mime_type = Some(mt.to_string());
            }
            remaining = remaining[close + 1..].trim_start();
            continue;
        }
        // Try resolution token (first whitespace-separated token).
        let end = remaining.find(char::is_whitespace).unwrap_or(remaining.len());
        let token = &remaining[..end];
        if let Some(res) = parse_resolution_token(token) {
            resolution_dppx = res;
        }
        remaining = remaining[end..].trim_start();
    }

    Some(ImageSetOption { url: url.to_string(), resolution_dppx, mime_type })
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Parses an `image-set()` / `-webkit-image-set()` expression into a list of
/// typed candidates.
///
/// If `value` is not an `image-set()` expression it is treated as a single 1×
/// candidate with no type constraint, so plain URLs pass through unchanged.
/// Returns an empty `Vec` only when all options fail to parse.
#[must_use]
pub fn parse_image_set(value: &str) -> Vec<ImageSetOption> {
    let trimmed = value.trim();
    let inner = strip_wrapper(trimmed).unwrap_or(trimmed);
    split_top_level(inner)
        .into_iter()
        .filter_map(|opt| parse_option(opt.trim()))
        .collect()
}

/// CSS Images L4 §5 — selects the best candidate from a parsed `image-set()`
/// option list for the given device pixel ratio (`dpr`) and supported MIME
/// types.
///
/// 1. Discards candidates whose `mime_type` is not accepted by `supported`.
/// 2. Among remaining candidates, picks the one whose `resolution_dppx` is
///    closest to `dpr`.  Ties prefer the higher-resolution (sharper) asset.
/// 3. Returns `None` when the candidate list is empty or all are filtered out.
#[must_use]
pub fn select_image_set_candidate<'a>(
    candidates: &'a [ImageSetOption],
    dpr: f32,
    supported: &SupportedTypes,
) -> Option<&'a ImageSetOption> {
    let mut best: Option<(&ImageSetOption, f32)> = None;
    for c in candidates {
        // Type filtering: skip unsupported formats.
        if let Some(mt) = &c.mime_type && !supported.accepts(mt) {
            continue;
        }
        let dist = (c.resolution_dppx - dpr).abs();
        let better = match best {
            None => true,
            Some((_, bd)) => dist < bd || (dist == bd && c.resolution_dppx > best.unwrap().0.resolution_dppx),
        };
        if better {
            best = Some((c, dist));
        }
    }
    best.map(|(c, _)| c)
}

/// Convenience wrapper: parses `value` and immediately selects the best URL
/// string for `dpr`.
///
/// Accepts every MIME type (Phase 0).  Returns `""` when no candidate can be
/// selected.
#[must_use]
pub fn select_image_set_url(value: &str, dpr: f32) -> String {
    let candidates = parse_image_set(value);
    select_image_set_candidate(&candidates, dpr, &SupportedTypes::all())
        .map(|c| c.url.clone())
        .unwrap_or_default()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_image_set ──────────────────────────────────────────────────────

    #[test]
    fn parse_plain_url_as_single_1x() {
        // A bare URL (no image-set wrapper) is treated as 1× candidate.
        let opts = parse_image_set("a.png");
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].url, "a.png");
        assert!((opts[0].resolution_dppx - 1.0).abs() < 1e-6);
        assert!(opts[0].mime_type.is_none());
    }

    #[test]
    fn parse_two_resolutions() {
        let opts = parse_image_set(r#"image-set("a.png" 1x, "a@2x.png" 2x)"#);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].url, "a.png");
        assert!((opts[0].resolution_dppx - 1.0).abs() < 1e-6);
        assert_eq!(opts[1].url, "a@2x.png");
        assert!((opts[1].resolution_dppx - 2.0).abs() < 1e-6);
    }

    #[test]
    fn parse_webkit_vendor_prefix() {
        let opts = parse_image_set(r#"-webkit-image-set(url("x.png") 1x)"#);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].url, "x.png");
    }

    #[test]
    fn parse_dppx_unit() {
        let opts = parse_image_set(r#"image-set("hi.png" 3dppx)"#);
        assert_eq!(opts.len(), 1);
        assert!((opts[0].resolution_dppx - 3.0).abs() < 1e-6);
    }

    #[test]
    fn parse_dpi_unit() {
        // 96dpi == 1dppx
        let opts = parse_image_set(r#"image-set("hi.png" 96dpi)"#);
        assert_eq!(opts.len(), 1);
        assert!((opts[0].resolution_dppx - 1.0).abs() < 1e-5);
    }

    #[test]
    fn parse_type_descriptor_webp() {
        let opts = parse_image_set(r#"image-set("a.webp" 1x type("image/webp"), "a.png" 1x)"#);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].mime_type.as_deref(), Some("image/webp"));
        assert!(opts[1].mime_type.is_none());
    }

    #[test]
    fn parse_type_before_resolution() {
        // CSS Images L4 allows type() before resolution descriptor.
        let opts = parse_image_set(r#"image-set("a.avif" type("image/avif") 2x)"#);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].mime_type.as_deref(), Some("image/avif"));
        assert!((opts[0].resolution_dppx - 2.0).abs() < 1e-6);
    }

    #[test]
    fn parse_missing_resolution_defaults_to_1() {
        let opts = parse_image_set(r#"image-set("a.png")"#);
        assert_eq!(opts.len(), 1);
        assert!((opts[0].resolution_dppx - 1.0).abs() < 1e-6);
    }

    // ── select_image_set_candidate ───────────────────────────────────────────

    #[test]
    fn selects_closest_resolution() {
        let opts = vec![
            ImageSetOption { url: "a1x.png".into(), resolution_dppx: 1.0, mime_type: None },
            ImageSetOption { url: "a2x.png".into(), resolution_dppx: 2.0, mime_type: None },
            ImageSetOption { url: "a3x.png".into(), resolution_dppx: 3.0, mime_type: None },
        ];
        let sel = select_image_set_candidate(&opts, 1.5, &SupportedTypes::all()).unwrap();
        assert_eq!(sel.url, "a2x.png"); // 2x is closer to 1.5 than 1x
    }

    #[test]
    fn tie_prefers_higher_resolution() {
        // dpr=1.5, distance to 1x == distance to 2x == 0.5: prefer 2x (sharper).
        let opts = vec![
            ImageSetOption { url: "a1x.png".into(), resolution_dppx: 1.0, mime_type: None },
            ImageSetOption { url: "a2x.png".into(), resolution_dppx: 2.0, mime_type: None },
        ];
        let sel = select_image_set_candidate(&opts, 1.5, &SupportedTypes::all()).unwrap();
        assert_eq!(sel.url, "a2x.png");
    }

    #[test]
    fn exact_dpr_match_selected() {
        let opts = vec![
            ImageSetOption { url: "a1x.png".into(), resolution_dppx: 1.0, mime_type: None },
            ImageSetOption { url: "a2x.png".into(), resolution_dppx: 2.0, mime_type: None },
        ];
        let sel = select_image_set_candidate(&opts, 2.0, &SupportedTypes::all()).unwrap();
        assert_eq!(sel.url, "a2x.png");
    }

    #[test]
    fn type_filtering_removes_unsupported_format() {
        // Only PNG is supported; AVIF candidate is filtered out.
        let supported = SupportedTypes::from_list(["image/png"]);
        let opts = vec![
            ImageSetOption {
                url: "a.avif".into(),
                resolution_dppx: 2.0,
                mime_type: Some("image/avif".into()),
            },
            ImageSetOption {
                url: "a.png".into(),
                resolution_dppx: 1.0,
                mime_type: Some("image/png".into()),
            },
        ];
        let sel = select_image_set_candidate(&opts, 2.0, &supported).unwrap();
        // AVIF (2x, exact match) filtered → fallback to PNG (1x).
        assert_eq!(sel.url, "a.png");
    }

    #[test]
    fn no_type_constraint_always_accepted() {
        // Options without type() are accepted regardless of SupportedTypes.
        let supported = SupportedTypes::from_list(["image/png"]);
        let opts = vec![
            ImageSetOption { url: "a.png".into(), resolution_dppx: 1.0, mime_type: None },
        ];
        let sel = select_image_set_candidate(&opts, 1.0, &supported).unwrap();
        assert_eq!(sel.url, "a.png");
    }

    #[test]
    fn all_filtered_returns_none() {
        let supported = SupportedTypes::from_list(["image/png"]);
        let opts = vec![
            ImageSetOption {
                url: "a.avif".into(),
                resolution_dppx: 1.0,
                mime_type: Some("image/avif".into()),
            },
        ];
        assert!(select_image_set_candidate(&opts, 1.0, &supported).is_none());
    }

    #[test]
    fn empty_candidates_returns_none() {
        assert!(select_image_set_candidate(&[], 1.0, &SupportedTypes::all()).is_none());
    }

    // ── select_image_set_url convenience wrapper ─────────────────────────────

    #[test]
    fn convenience_wrapper_selects_2x_for_dpr2() {
        let url = select_image_set_url(r#"image-set("a.png" 1x, "a@2x.png" 2x)"#, 2.0);
        assert_eq!(url, "a@2x.png");
    }

    #[test]
    fn convenience_wrapper_plain_url_passthrough() {
        let url = select_image_set_url("logo.png", 1.0);
        assert_eq!(url, "logo.png");
    }
}
