//! CSS Properties and Values API — the `syntax` descriptor's micro-grammar
//! ("syntax strings",
//! <https://drafts.css-houdini.org/css-properties-values-api-1/#supported-names>)
//! and matching of a value against a parsed syntax.
//!
//! BUG-531: this is the piece `CSS.registerProperty()` was missing entirely —
//! `crates/js/src/css_properties_values_api.rs` stored `syntax`/`initialValue`
//! verbatim with no grammar check at all, so `SyntaxError` was never thrown
//! for a malformed descriptor or a mismatched initial value. [`validate_registered_property`]
//! is the entry point the JS shim calls into (via a native binding) to decide
//! whether to throw.
//!
//! Kept separate from [`crate::style::property_syntax`], which owns the
//! simpler `@property`-at-rule-facing `validate_against_syntax` used by the
//! cascade to gate arbitrary declarations against an already-registered
//! property — that entry point now delegates here for the actual grammar
//! parsing and matching, but stays permissive when the descriptor itself is
//! malformed (a pre-existing `@property` concern this bug doesn't reach) and,
//! unlike [`validate_registered_property`], does not reject font-relative
//! length units: those are only disallowed for a registration's
//! *initial value* (which has no element to resolve them against), not for a
//! later declaration on a real element (which does).

use crate::style::calc::calc_node_contains_percent;
use crate::style::parse::image::parse_bg_image_value;
use crate::style::parse::transform::parse_transform_fn;
use crate::style::{BackgroundImage, CalcNode, Length, parse_color, parse_length};

/// One `|`-alternative in a `syntax` descriptor, together with its optional
/// `+`/`#` multiplier.
#[derive(Clone, Debug, PartialEq)]
struct SyntaxComponent {
    kind: SyntaxComponentKind,
    multiplier: Option<SyntaxMultiplier>,
}

/// What a single [`SyntaxComponent`] matches: a `<type>` data-type name or a
/// literal identifier (already escape-decoded).
#[derive(Clone, Debug, PartialEq)]
enum SyntaxComponentKind {
    Type(SyntaxType),
    Literal(String),
}

/// The fixed set of `<type>` names the syntax-string grammar recognizes
/// (CSS Properties and Values L1 §Syntax Strings). Names are matched
/// verbatim lowercase — `<LENGTH>`/`<Length>` are not the same token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyntaxType {
    Length,
    Number,
    Percentage,
    LengthPercentage,
    Color,
    Image,
    Url,
    Integer,
    Angle,
    Time,
    Resolution,
    TransformFunction,
    TransformList,
    CustomIdent,
    String,
}

/// `+` (whitespace-separated list) or `#` (comma-separated list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyntaxMultiplier {
    Plus,
    Hash,
}

/// A fully parsed `syntax` descriptor: either the universal syntax (`*`) or
/// a non-empty list of `|`-alternative components.
#[derive(Clone, Debug, PartialEq)]
enum ParsedSyntax {
    /// `syntax: '*'` — matches any value except a CSS-wide keyword.
    Universal,
    /// `syntax: '<a> | <b>+ | literal#'` — matches if any alternative does.
    Components(Vec<SyntaxComponent>),
}

// ---------------------------------------------------------------------
// Grammar: parsing the `syntax` descriptor itself.
// ---------------------------------------------------------------------

/// Parses and validates a `syntax` descriptor's grammar.
///
/// Returns `Err(())` for anything the CSS Properties and Values API grammar
/// rejects: a malformed `<type>` name, a doubled/misplaced `+`/`#`, `*`
/// combined with anything else (via `|` or a multiplier), an empty
/// alternative, or a literal component that collides with a CSS-wide
/// keyword or `default`.
fn parse_syntax_string(input: &str) -> Result<ParsedSyntax, ()> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(());
    }
    if trimmed == "*" {
        return Ok(ParsedSyntax::Universal);
    }
    let mut components = Vec::new();
    for raw in trimmed.split('|') {
        let piece = raw.trim();
        if piece.is_empty() || piece == "*" {
            return Err(());
        }
        components.push(parse_syntax_component(piece)?);
    }
    Ok(ParsedSyntax::Components(components))
}

fn parse_syntax_component(piece: &str) -> Result<SyntaxComponent, ()> {
    let (body, multiplier) = match piece.as_bytes().last().copied() {
        Some(b'+') => (&piece[..piece.len() - 1], Some(SyntaxMultiplier::Plus)),
        Some(b'#') => (&piece[..piece.len() - 1], Some(SyntaxMultiplier::Hash)),
        _ => (piece, None),
    };
    if body.is_empty() || matches!(body.as_bytes().last(), Some(b'+' | b'#')) {
        return Err(());
    }
    let kind = if let Some(rest) = body.strip_prefix('<') {
        let name = rest.strip_suffix('>').ok_or(())?;
        SyntaxComponentKind::Type(parse_syntax_type_name(name)?)
    } else if body.contains('<') || body.contains('>') {
        return Err(());
    } else {
        let ident = decode_ident_token(body).ok_or(())?;
        if is_reserved_syntax_ident(&ident) {
            return Err(());
        }
        SyntaxComponentKind::Literal(ident)
    };
    // `<transform-list>` is already a repeatable list at the value level —
    // the spec explicitly disallows re-multiplying it with `+`/`#`.
    if matches!(kind, SyntaxComponentKind::Type(SyntaxType::TransformList)) && multiplier.is_some() {
        return Err(());
    }
    Ok(SyntaxComponent { kind, multiplier })
}

fn parse_syntax_type_name(name: &str) -> Result<SyntaxType, ()> {
    Ok(match name {
        "length" => SyntaxType::Length,
        "number" => SyntaxType::Number,
        "percentage" => SyntaxType::Percentage,
        "length-percentage" => SyntaxType::LengthPercentage,
        "color" => SyntaxType::Color,
        "image" => SyntaxType::Image,
        "url" => SyntaxType::Url,
        "integer" => SyntaxType::Integer,
        "angle" => SyntaxType::Angle,
        "time" => SyntaxType::Time,
        "resolution" => SyntaxType::Resolution,
        "transform-function" => SyntaxType::TransformFunction,
        "transform-list" => SyntaxType::TransformList,
        "custom-ident" => SyntaxType::CustomIdent,
        "string" => SyntaxType::String,
        _ => return Err(()),
    })
}

/// A literal `syntax` component can't be a CSS-wide keyword or `default` —
/// same restriction `<custom-ident>` places on the *value* side
/// ([`matches_custom_ident`]).
fn is_reserved_syntax_ident(ident: &str) -> bool {
    let lower = ident.to_ascii_lowercase();
    matches!(lower.as_str(), "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default")
}

/// A CSS-wide keyword is never a valid registered-property value, regardless
/// of the declared syntax — including the universal one (5 WPT cases the
/// spec doesn't clearly mandate but every engine agrees on, see
/// `register-property-syntax-parsing.html`'s "not clearly backed by the
/// specification" comment). Unlike [`is_reserved_syntax_ident`], `default`
/// is *not* included here — `syntax: '*'` happily accepts it as a value.
fn is_css_wide_keyword_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    matches!(lower.as_str(), "initial" | "inherit" | "unset" | "revert" | "revert-layer")
}

// ---------------------------------------------------------------------
// CSS ident-token decoding (CSS Syntax Module L3 §4.3.7 "consume an escaped
// code point" / §4.3.9 "would start an identifier"), simplified to operate
// on a `&str` already known to be one token's worth of source text.
// ---------------------------------------------------------------------

fn is_newline(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\x0c')
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn is_name_continue(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-'
}

/// Decodes `s` as a single CSS ident token (escapes included), requiring the
/// WHOLE string to be consumed as exactly one token. Used both for a literal
/// `syntax` component's name and for a value checked against it or against
/// `<custom-ident>` — BUG-531's WPT file has both sides carrying `\XX`
/// escapes independently (`syntax: "banan\61"` matching value `"banana"`,
/// and vice versa) and expects them to decode to the same ident. Returns
/// `None` if `s` isn't a valid, fully-consumed ident token.
fn decode_ident_token(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    if n == 0 {
        return None;
    }
    // "Would start an identifier" (CSS Syntax §4.3.9), restricted to the
    // one/two-character forms a syntax-string component can produce.
    let start = if chars[0] == '-' { 1 } else { 0 };
    if start >= n {
        return None; // a lone "-" does not start an identifier
    }
    match chars[start] {
        '-' => {} // second '-' of a "--" prefix always starts an identifier
        '\\' => {
            if start + 1 >= n || is_newline(chars[start + 1]) {
                return None;
            }
        }
        c if is_name_start(c) => {}
        _ => return None,
    }

    let mut out = String::with_capacity(n);
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '\\' {
            if i + 1 >= n {
                out.push('\u{fffd}'); // lone trailing "\" — CSS Syntax replaces it with U+FFFD.
                i += 1;
                continue;
            }
            if is_newline(chars[i + 1]) {
                return None; // invalid escape
            }
            if chars[i + 1].is_ascii_hexdigit() {
                let mut j = i + 1;
                let mut hex = String::new();
                while j < n && hex.len() < 6 && chars[j].is_ascii_hexdigit() {
                    hex.push(chars[j]);
                    j += 1;
                }
                if j < n && chars[j].is_whitespace() {
                    j += 1; // one trailing whitespace terminates the hex escape
                }
                let code = u32::from_str_radix(&hex, 16).ok()?;
                out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                i = j;
            } else {
                out.push(chars[i + 1]);
                i += 2;
            }
        } else if is_name_continue(c) {
            out.push(c);
            i += 1;
        } else {
            return None; // stray char — not a single fully-consumed ident token
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------
// Top-level list splitting for the `+`/`#` multipliers (CSS's
// whitespace-separated / comma-separated list syntax), paren/bracket/
// string-aware so a `calc(...)`'s or a transform function's internal
// whitespace/commas don't get mistaken for list separators.
// ---------------------------------------------------------------------

/// Splits `s` on top-level (i.e. outside `'…'`/`"…"`/`(…)`/`[…]`/`{…}`)
/// separators — commas when `by_comma`, whitespace runs otherwise. Returns
/// `None` on unbalanced nesting.
fn split_top_level(s: &str, by_comma: bool) -> Option<Vec<&str>> {
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut depth = 0i32;
    let mut in_string: Option<u8> = None;
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < n {
        let c = bytes[i];
        if let Some(q) = in_string {
            if c == b'\\' && i + 1 < n {
                i += 2;
                continue;
            }
            if c == q {
                in_string = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' | b'"' => {
                in_string = Some(c);
                i += 1;
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                i += 1;
            }
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                i += 1;
            }
            b',' if by_comma && depth == 0 => {
                items.push(&s[start..i]);
                i += 1;
                start = i;
            }
            b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' if !by_comma && depth == 0 => {
                if i > start {
                    items.push(&s[start..i]);
                }
                while i < n && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c') {
                    i += 1;
                }
                start = i;
                continue;
            }
            _ => i += 1,
        }
    }
    if start < n {
        items.push(&s[start..]);
    }
    if depth != 0 || in_string.is_some() {
        return None;
    }
    Some(items)
}

/// `+` — one-or-more, whitespace-separated.
fn split_top_level_whitespace(s: &str) -> Option<Vec<&str>> {
    let items = split_top_level(s, false)?;
    if items.is_empty() { None } else { Some(items) }
}

/// `#` — one-or-more, comma-separated (whitespace around a comma is
/// trimmed; a leading/trailing/doubled comma yields an empty item, which is
/// rejected — CSS list syntax requires every item to be non-empty).
fn split_top_level_comma(s: &str) -> Option<Vec<&str>> {
    let items = split_top_level(s, true)?;
    if items.is_empty() || items.iter().any(|i| i.trim().is_empty()) {
        return None;
    }
    Some(items.into_iter().map(str::trim).collect())
}

// ---------------------------------------------------------------------
// Per-type value matchers.
// ---------------------------------------------------------------------

fn length_leaf_is_font_relative(l: &Length) -> bool {
    matches!(l, Length::Em(_) | Length::Rem(_) | Length::Ch(_) | Length::Ex(_))
}

fn calc_node_has_font_relative_length(node: &CalcNode) -> bool {
    match node {
        CalcNode::Length(l) => length_leaf_is_font_relative(l),
        CalcNode::Number(_) => false,
        CalcNode::Add(a, b) | CalcNode::Sub(a, b) | CalcNode::Mul(a, b) | CalcNode::Div(a, b) => {
            calc_node_has_font_relative_length(a) || calc_node_has_font_relative_length(b)
        }
        CalcNode::Min(args) | CalcNode::Max(args) => args.iter().any(calc_node_has_font_relative_length),
        CalcNode::Clamp(mn, val, mx) => {
            calc_node_has_font_relative_length(mn)
                || calc_node_has_font_relative_length(val)
                || calc_node_has_font_relative_length(mx)
        }
        CalcNode::Func(_, args) => args.iter().any(calc_node_has_font_relative_length),
    }
}

fn matches_length(value: &str) -> bool {
    match parse_length(value) {
        Some(Length::Percent(_)) => false,
        Some(Length::Calc(node)) => !calc_node_contains_percent(&node),
        Some(_) => true,
        None => false,
    }
}

fn matches_percentage(value: &str) -> bool {
    matches!(parse_length(value), Some(Length::Percent(_)))
}

fn matches_length_percentage(value: &str) -> bool {
    parse_length(value).is_some()
}

fn matches_integer(value: &str) -> bool {
    value.trim().parse::<i64>().is_ok()
}

fn matches_number(value: &str) -> bool {
    value.trim().parse::<f64>().is_ok()
}

fn matches_angle(value: &str) -> bool {
    for suffix in ["deg", "rad", "turn", "grad"] {
        if let Some(num) = value.strip_suffix(suffix)
            && num.trim().parse::<f64>().is_ok()
        {
            return true;
        }
    }
    false
}

fn matches_time(value: &str) -> bool {
    // `ms` first — otherwise `200ms` reads as `200m` + a leftover `s`.
    for suffix in ["ms", "s"] {
        if let Some(num) = value.strip_suffix(suffix)
            && num.trim().parse::<f64>().is_ok()
        {
            return true;
        }
    }
    false
}

fn matches_resolution(value: &str) -> bool {
    // CSS Values L4 §9.1: "the allowed range of <resolution> values always
    // excludes negative values" — `-5.3dpcm` must fail even though the
    // number itself parses fine.
    for suffix in ["dppx", "dpcm", "dpi", "x"] {
        if let Some(num) = value.strip_suffix(suffix)
            && let Ok(n) = num.trim().parse::<f64>()
            && n >= 0.0
        {
            return true;
        }
    }
    false
}

fn matches_custom_ident(value: &str) -> bool {
    match decode_ident_token(value) {
        Some(ident) => !is_reserved_syntax_ident(&ident),
        None => false,
    }
}

/// Whether `s` is exactly one CSS `<string>` token — `'…'`/`"…"`, escapes
/// allowed, an *unterminated* string running to EOF is still a valid token
/// (CSS Syntax §4.3.5), an *unescaped* literal newline inside one is not
/// (produces a bad-string-token).
fn is_string_token(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    if n == 0 {
        return false;
    }
    let quote = chars[0];
    if quote != '\'' && quote != '"' {
        return false;
    }
    let mut i = 1;
    while i < n {
        let c = chars[i];
        if c == quote {
            return i == n - 1; // must close exactly at the end — one token only
        }
        if c == '\\' {
            if i + 1 >= n {
                return true; // trailing "\" right before EOF: still unterminated, still fine
            }
            i += 2; // skip the escaped char (an escaped newline is a valid line continuation)
            continue;
        }
        if is_newline(c) {
            return false; // bad-string
        }
        i += 1;
    }
    true // ran off the end without a closing quote — unterminated, still valid
}

/// `light-dark(a, b)` — recognized (loosely: just the two-argument shape,
/// not a recursive re-check of each argument's own type) for `<color>` and
/// `<image>`, both of which allow it per CSS Color 5 / CSS Images 4. Neither
/// matcher below has scheme context to pick a branch, so this only answers
/// "is this syntactically a light-dark() call", not "which color does it
/// resolve to".
fn light_dark_args(token: &str) -> Option<Vec<&str>> {
    let t = token.trim();
    if t.len() < 12 || !t[..11].eq_ignore_ascii_case("light-dark(") || !t.ends_with(')') {
        return None;
    }
    split_top_level_comma(&t[11..t.len() - 1])
}

fn matches_color(value: &str) -> bool {
    if let Some(args) = light_dark_args(value) {
        return args.len() == 2;
    }
    parse_color(value).is_some()
}

fn matches_image(value: &str) -> bool {
    let t = value.trim();
    if let Some(args) = light_dark_args(t) {
        return args.len() == 2;
    }
    // `<image>` excludes the `none` keyword (CSS Images L3 §2) even though
    // `parse_bg_image_value` accepts it for `background-image`'s own grammar.
    if t.eq_ignore_ascii_case("none") {
        return false;
    }
    parse_bg_image_value(t).is_some_and(|img| !matches!(img, BackgroundImage::None))
}

fn matches_url(value: &str) -> bool {
    let t = value.trim();
    t.len() >= 4 && t[..4].eq_ignore_ascii_case("url(") && crate::style::parse::image::parse_url_value(t).is_some()
}

fn matches_single_transform_function(token: &str) -> bool {
    let token = token.trim();
    let Some(paren_pos) = token.find('(') else {
        return false;
    };
    if !token.ends_with(')') {
        return false;
    }
    let name = token[..paren_pos].trim().to_ascii_lowercase();
    let args = &token[paren_pos + 1..token.len() - 1];
    if args.contains(')') {
        return false; // more than one top-level function — not a single <transform-function>
    }
    parse_transform_fn(&name, args).is_some()
}

fn matches_transform_list(value: &str) -> bool {
    match split_top_level_whitespace(value) {
        Some(tokens) => tokens.iter().all(|t| matches_single_transform_function(t)),
        None => false,
    }
}

/// CSS Syntax Module §9.1 `<declaration-value>` — the value grammar the
/// universal syntax (`syntax: '*'`) accepts: any token sequence without a
/// bad-string, a bad-url, an unmatched closing bracket, or a top-level `;`
/// / `!`. Also rejects any `var()`/`env()` reference: the CSS Properties and
/// Values API additionally requires a registered value to be
/// "computationally independent", and a substitution function can't be
/// resolved at registration time (BUG-531: `var(--foo)` was silently
/// accepted before this fix).
fn declaration_value_is_well_formed(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut i = 0usize;
    let mut ident_start = 0usize;
    let mut stack: Vec<char> = Vec::new();

    while i < n {
        let c = chars[i];
        match c {
            '/' if i + 1 < n && chars[i + 1] == '*' => {
                i += 2;
                while i + 1 < n && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i = if i + 1 < n { i + 2 } else { n };
                ident_start = i;
            }
            '\'' | '"' => {
                let quote = c;
                i += 1;
                loop {
                    if i >= n {
                        break; // unterminated string at EOF: still a valid token
                    }
                    if chars[i] == '\\' {
                        i = if i + 1 < n { i + 2 } else { n };
                        continue;
                    }
                    if chars[i] == quote {
                        i += 1;
                        break;
                    }
                    if is_newline(chars[i]) {
                        return false; // bad-string
                    }
                    i += 1;
                }
                ident_start = i;
            }
            '(' => {
                let name: String = chars[ident_start..i].iter().collect::<String>().to_ascii_lowercase();
                if name == "var" || name == "env" {
                    return false;
                }
                if name == "url" {
                    let mut j = i + 1;
                    while j < n && chars[j].is_whitespace() {
                        j += 1;
                    }
                    if j < n && (chars[j] == '"' || chars[j] == '\'') {
                        stack.push(')'); // quoted url(...) — an ordinary function call
                        i += 1;
                        ident_start = i;
                        continue;
                    }
                    match consume_unquoted_url_contents(&chars, i + 1) {
                        Some(next) => {
                            i = next;
                            ident_start = i;
                            continue;
                        }
                        None => return false, // bad-url
                    }
                }
                stack.push(')');
                i += 1;
                ident_start = i;
            }
            '[' => {
                stack.push(']');
                i += 1;
                ident_start = i;
            }
            '{' => {
                stack.push('}');
                i += 1;
                ident_start = i;
            }
            ')' | ']' | '}' => {
                match stack.pop() {
                    Some(expected) if expected == c => {}
                    _ => return false, // unmatched closer
                }
                i += 1;
                ident_start = i;
            }
            ';' if stack.is_empty() => return false,
            '!' if stack.is_empty() => return false,
            c if is_name_continue(c) => i += 1,
            _ => {
                i += 1;
                ident_start = i;
            }
        }
    }
    true // a leftover unclosed opening bracket at EOF is fine
}

/// CSS Syntax §4.3.6 "consume a url token", unquoted-content path: called
/// right after `url(` (and any already-consumed leading whitespace is
/// skipped here). A quote, an unescaped `(`, a non-printable code point, or
/// non-whitespace content after the first run of whitespace makes it a
/// bad-url-token — always invalid in a `<declaration-value>`.
fn consume_unquoted_url_contents(chars: &[char], mut i: usize) -> Option<usize> {
    let n = chars.len();
    while i < n && chars[i].is_whitespace() {
        i += 1;
    }
    loop {
        if i >= n {
            return Some(i); // unterminated url(...) at EOF: treated like an unclosed bracket
        }
        match chars[i] {
            ')' => return Some(i + 1),
            c if c.is_whitespace() => {
                let mut j = i;
                while j < n && chars[j].is_whitespace() {
                    j += 1;
                }
                if j >= n {
                    return Some(j);
                }
                if chars[j] == ')' {
                    return Some(j + 1);
                }
                return None; // whitespace followed by more content — bad-url
            }
            '"' | '\'' | '(' => return None,
            '\\' if i + 1 < n && !is_newline(chars[i + 1]) => i += 2,
            '\\' => return None,
            c if c.is_control() => return None,
            _ => i += 1,
        }
    }
}

// ---------------------------------------------------------------------
// Component dispatch + top-level matching.
// ---------------------------------------------------------------------

fn component_matches_single(kind: &SyntaxComponentKind, token: &str) -> bool {
    match kind {
        SyntaxComponentKind::Literal(lit) => decode_ident_token(token).as_deref() == Some(lit.as_str()),
        SyntaxComponentKind::Type(ty) => match ty {
            SyntaxType::Length => matches_length(token),
            SyntaxType::Percentage => matches_percentage(token),
            SyntaxType::LengthPercentage => matches_length_percentage(token),
            SyntaxType::Color => matches_color(token),
            SyntaxType::Image => matches_image(token),
            SyntaxType::Url => matches_url(token),
            SyntaxType::Integer => matches_integer(token),
            SyntaxType::Number => matches_number(token),
            SyntaxType::Angle => matches_angle(token),
            SyntaxType::Time => matches_time(token),
            SyntaxType::Resolution => matches_resolution(token),
            SyntaxType::TransformFunction => matches_single_transform_function(token),
            SyntaxType::TransformList => matches_transform_list(token),
            SyntaxType::CustomIdent => matches_custom_ident(token),
            SyntaxType::String => is_string_token(token),
        },
    }
}

/// CSS Properties and Values API — is `token` "computationally independent"
/// for `kind`? Only `<length>`/`<length-percentage>` carry a restriction:
/// an *initial value* can't use a font-relative unit (`em`/`ex`/`ch`/`rem`),
/// because it is evaluated with no element to resolve them against.
/// Viewport units and absolute lengths are fine, and every other type is
/// unconditionally independent — this check exists only for
/// [`validate_registered_property`]'s initial-value path, NOT for
/// `property_syntax::validate_against_syntax`'s cascade-facing one (an
/// ordinary declaration on a real element resolves `em`/`rem` normally).
fn is_computationally_independent(kind: &SyntaxComponentKind, token: &str) -> bool {
    match kind {
        SyntaxComponentKind::Type(SyntaxType::Length | SyntaxType::LengthPercentage) => match parse_length(token) {
            Some(Length::Calc(node)) => !calc_node_has_font_relative_length(&node),
            Some(l) => !length_leaf_is_font_relative(&l),
            None => true, // already failed the type match itself; not this check's job
        },
        _ => true,
    }
}

fn component_matches(component: &SyntaxComponent, value: &str, require_independent: bool) -> bool {
    let single_ok = |t: &str| {
        component_matches_single(&component.kind, t)
            && (!require_independent || is_computationally_independent(&component.kind, t))
    };
    match component.multiplier {
        None => single_ok(value),
        Some(SyntaxMultiplier::Plus) => match split_top_level_whitespace(value) {
            Some(tokens) => tokens.iter().all(|t| single_ok(t)),
            None => false,
        },
        Some(SyntaxMultiplier::Hash) => match split_top_level_comma(value) {
            Some(tokens) => tokens.iter().all(|t| single_ok(t)),
            None => false,
        },
    }
}

fn value_matches_parsed_syntax(value: &str, parsed: &ParsedSyntax, require_independent: bool) -> bool {
    let value = value.trim();
    match parsed {
        ParsedSyntax::Universal => !is_css_wide_keyword_value(value) && declaration_value_is_well_formed(value),
        ParsedSyntax::Components(components) => {
            components.iter().any(|c| component_matches(c, value, require_independent))
        }
    }
}

// ---------------------------------------------------------------------
// Public entry points.
// ---------------------------------------------------------------------

/// Parses `syntax` and validates it grammatically; if it parses, also
/// checks `value` against it (permissive cascade-facing rules — no
/// computational-independence restriction). `None` means `syntax` itself
/// doesn't parse — the caller decides the (permissive) fallback. Used by
/// [`crate::style::property_syntax::validate_against_syntax`].
pub(in crate::style) fn cascade_validate_against_syntax(value: &str, syntax: &str) -> Option<bool> {
    parse_syntax_string(syntax).ok().map(|parsed| value_matches_parsed_syntax(value, &parsed, false))
}

/// CSS Properties and Values API — `CSS.registerProperty()`'s "register a
/// custom property" algorithm (BUG-531), the part that was entirely
/// missing: parses and validates `syntax`, then — if `initial_value` is
/// given — checks it against that syntax (strict: a font-relative length
/// unit fails the "computationally independent" requirement here, even
/// though it's fine for a later declaration on a real element). `None`
/// means the caller omitted `initialValue`, required unless `syntax` is the
/// universal `'*'`.
///
/// Returns the message to put in the `SyntaxError` `DOMException` the JS
/// shim throws on `Err`.
pub fn validate_registered_property(syntax: &str, initial_value: Option<&str>) -> Result<(), String> {
    let parsed = parse_syntax_string(syntax)
        .map_err(|()| format!("The syntax provided ('{syntax}') is not a valid syntax string."))?;
    match initial_value {
        None => {
            if matches!(parsed, ParsedSyntax::Universal) {
                Ok(())
            } else {
                Err("An initial value is required for non-universal syntax.".to_string())
            }
        }
        Some(v) => {
            if value_matches_parsed_syntax(v, &parsed, true) {
                Ok(())
            } else {
                Err(format!("The initial value ('{v}') does not parse for the given syntax."))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(syntax: &str, value: &str) {
        assert!(
            validate_registered_property(syntax, Some(value)).is_ok(),
            "expected valid: syntax={syntax:?} value={value:?}"
        );
    }

    fn err(syntax: &str, value: &str) {
        assert!(
            validate_registered_property(syntax, Some(value)).is_err(),
            "expected invalid: syntax={syntax:?} value={value:?}"
        );
    }

    #[test]
    fn universal_accepts_almost_anything() {
        ok("*", "a");
        ok("*", "([ brackets ]) { yay (??)}");
        ok("*", "default");
        err("*", "initial");
        err("*", ")");
        err("*", "var(--foo)");
        err("*", "semi;colon");
    }

    #[test]
    fn dashed_and_escaped_idents() {
        ok("--foo", "--foo");
        ok("banana", "banan\\61");
        ok("banan\\61", "banana");
        ok("<custom-ident>", "banan\\61");
        err("<custom-ident>", "default");
    }

    #[test]
    fn malformed_syntax_strings_are_rejected() {
        err("<length", "10px");
        err("<LENGTH>", "10px");
        err("< length>", "10px");
        err("<length >", "10px");
        err("<length> +", "10px");
        err("<length>++", "10px");
        err("<length> | *", "10px");
        err("*|banana", "banana");
        err("|banana", "banana");
        err("<transform-list>+", "scale(2)");
        err("<length>|initial", "10px");
    }

    #[test]
    fn multipliers_and_types() {
        ok("<length>+", "2px 7px calc(8px)");
        ok("<length>#", "2px, 7px, calc(8px)");
        err("<length>+", "2px,7px,calc(8px)");
        err("<color>#", "yellow blue");
        ok("<transform-list>", "translateX(2px) rotate(20deg)");
        err("<transform-function>", "scale()");
        ok("<string>", "'foo bar");
        err("<string>", "foo");
    }

    #[test]
    fn initial_value_must_be_computationally_independent() {
        ok("<length>", "10vmin");
        err("<length>", "10em");
        err("<length>+", "10px calc(20px + 4rem)");
    }

    #[test]
    fn missing_initial_value_required_for_non_universal() {
        assert!(validate_registered_property("*", None).is_ok());
        assert!(validate_registered_property("<length>", None).is_err());
    }
}
