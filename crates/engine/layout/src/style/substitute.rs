//! Подстановки в значении декларации до её разбора: `var()` (CSS Variables L1
//! §3), `env()` (CSS Environment L1), `attr()` (CSS Values L5 §7) и вызовы
//! пользовательских функций `@function` (CSS Mixins L1).
//!
//! Перенесено батчем SPLIT-ST9 из `crates/engine/layout/src/style.rs`
//! (анкер `const VAR_EXPAND_MAX_DEPTH`) без правок тел: изменена только
//! видимость тех items, которые продолжает звать `style.rs`.

use std::collections::HashMap;

use lumen_css_parser::FunctionRule;
use lumen_dom::{Document, NodeId};

/// Глубина рекурсии при разворачивании `var()` — защита от циклов вида
/// `--a: var(--b); --b: var(--a)`. CSS spec не задаёт точного предела;
/// 32 уровня хватает для любого реалистичного nesting, а зацикленные
/// определения отсекутся быстро.
const VAR_EXPAND_MAX_DEPTH: u32 = 32;

/// Раскрывает `var()`, затем `env()`, ровно в том порядке и с той же
/// семантикой отказа, что и main-pass внутри [`apply_declaration`]: `env()`
/// идёт вторым, потому что custom property может содержать `env(...)`.
/// `None` = декларация invalid at computed value time (CSS Variables L1 §3.3).
///
/// Вынесено в отдельную функцию ради pre-pass-а `font-size`/`font`
/// ([`apply_font_size`], BUG-731) — единственного места, где значение
/// парсится вне main-pass-а, и где раньше `var()` терялся.
pub(crate) fn expand_vars_and_env(
    value: &str,
    custom: &HashMap<String, String>,
) -> Option<String> {
    let after_var = if value.contains("var(") {
        expand_vars(value, custom, 0)?
    } else {
        value.to_string()
    };
    if after_var.contains("env(") {
        expand_env_vars(&after_var, &empty_env_registry(), 0)
    } else {
        Some(after_var)
    }
}

/// CSS Variables L1 §3: рекурсивно разворачивает все `var(--name [, fallback])`
/// в `value`. Возвращает None, если:
///   - встретилась `var()` с именем, которого нет в `custom`, и нет fallback;
///   - превышена глубина рекурсии (cycle / слишком глубокий nest);
///   - синтаксис `var(...)` сломан (нет закрывающей скобки).
///
/// При успехе — возвращает строку с подставленными значениями. Все
/// substitution-ы делаются как plain string replacement; типы значений
/// проверит уже сам `apply_declaration` после expand.
pub(in crate::style) fn expand_vars(value: &str, custom: &HashMap<String, String>, depth: u32) -> Option<String> {
    if depth > VAR_EXPAND_MAX_DEPTH {
        return None;
    }
    let Some(start) = find_var_open(value) else {
        return Some(value.to_string());
    };
    let prefix = &value[..start];
    let after_open = &value[start + 4..]; // skip "var("
    let (args, after_close) = parse_balanced_to_close(after_open)?;
    let (name, fallback) = split_var_args(args);
    if !name.starts_with("--") {
        return None;
    }
    let resolved = if let Some(v) = custom.get(name) {
        expand_vars(v.trim(), custom, depth + 1)?
    } else {
        let fb = fallback?;
        expand_vars(fb.trim(), custom, depth + 1)?
    };
    let combined = format!("{prefix}{resolved}{after_close}");
    expand_vars(&combined, custom, depth + 1)
}

/// Recursion guard for `--name(args)` custom function call expansion
/// (CSS Functions and Mixins L1). Kept smaller than `VAR_EXPAND_MAX_DEPTH`
/// because each call additionally recurses through parameter binding.
const FUNCTION_CALL_MAX_DEPTH: u32 = 16;

/// CSS Functions and Mixins L1: recursively expands `--name(<args>)` custom
/// function calls in `value` against `functions` (the stylesheet's parsed
/// `@function` rules) and `custom` (the element's resolved custom
/// properties, used as the outer scope for `var()` inside argument
/// expressions). Positional arguments bind to the function's declared
/// parameters (a missing trailing argument falls back to that parameter's
/// default, if any); the function's local `--x: ...;` declarations are
/// applied in order to build a local scope, then the `result:` descriptor
/// is expanded against that scope and spliced in place of the call.
///
/// Returns `None` (property invalid at computed-value time, mirroring
/// `var()` with no fallback) when: the called name doesn't match any
/// `@function` rule, an argument is missing with no default, the function
/// has no `result` descriptor, or recursion exceeds `FUNCTION_CALL_MAX_DEPTH`
/// (cycle guard, e.g. `--a() { result: --b(); }` / `--b() { result: --a(); }`).
///
/// Deferred (CSS Functions and Mixins L1, not yet implemented): `returns`
/// type-checking, conditional group rules inside the function body
/// (`@media`, `@container`), named/keyword arguments.
pub(in crate::style) fn expand_custom_functions(
    value: &str,
    functions: &[FunctionRule],
    custom: &HashMap<String, String>,
    depth: u32,
) -> Option<String> {
    if depth > FUNCTION_CALL_MAX_DEPTH {
        return None;
    }
    let Some((start, name_end)) = find_custom_function_call(value) else {
        return Some(value.to_string());
    };
    let name = &value[start..name_end];
    let after_open = &value[name_end + 1..]; // skip '('
    let (args_str, after_close) = parse_balanced_to_close(after_open)?;
    let func = functions.iter().find(|f| f.name == name)?;
    let args = split_call_args(args_str);

    let mut local: HashMap<String, String> = custom.clone();
    for (i, param) in func.parameters.iter().enumerate() {
        let raw_arg = match args.get(i) {
            Some(a) => a.trim().to_string(),
            None => param.default.clone()?,
        };
        let expanded_arg = expand_vars(&raw_arg, custom, depth + 1)
            .and_then(|v| expand_custom_functions(&v, functions, custom, depth + 1))?;
        local.insert(param.name.clone(), expanded_arg);
    }

    let mut result_raw: Option<&str> = None;
    for decl in &func.declarations {
        if decl.property.eq_ignore_ascii_case("result") {
            result_raw = Some(decl.value.as_str());
            continue;
        }
        if let Some(local_name) = decl.property.strip_prefix("--") {
            let v = expand_vars(&decl.value, &local, depth + 1)
                .and_then(|v| expand_custom_functions(&v, functions, &local, depth + 1))?;
            local.insert(format!("--{local_name}"), v);
        }
    }
    let resolved = expand_vars(result_raw?, &local, depth + 1)
        .and_then(|v| expand_custom_functions(&v, functions, &local, depth + 1))?;

    let combined = format!("{}{}{}", &value[..start], resolved, after_close);
    expand_custom_functions(&combined, functions, custom, depth + 1)
}

/// Finds the first `--<ident>(` call-site (CSS Functions and Mixins L1
/// function-token grammar: no whitespace between the name and `(`) outside
/// quoted strings. Returns `(name_start, name_end)` where `name_end` is the
/// index of the opening `(`.
fn find_custom_function_call(s: &str) -> Option<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_string: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match in_string {
            Some(q) => {
                if b == q {
                    in_string = None;
                }
                i += 1;
            }
            None => match b {
                b'"' | b'\'' => {
                    in_string = Some(b);
                    i += 1;
                }
                b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                    let start = i;
                    let mut j = i + 2;
                    while j < bytes.len()
                        && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-' || bytes[j] == b'_')
                    {
                        j += 1;
                    }
                    if j > start + 2 && j < bytes.len() && bytes[j] == b'(' {
                        return Some((start, j));
                    }
                    i = j.max(i + 1);
                }
                _ => i += 1,
            },
        }
    }
    None
}

/// Splits `--name(<here>)` call arguments on top-level commas (nested
/// parens and quoted strings are not split points). An all-whitespace `s`
/// (zero-argument call, `--foo()`) yields an empty `Vec` rather than one
/// blank element.
fn split_call_args(s: &str) -> Vec<&str> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    let bytes = s.as_bytes();
    let mut depth = 0u32;
    let mut in_string: Option<u8> = None;
    let mut start = 0usize;
    let mut out = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        match in_string {
            Some(q) => {
                if b == q {
                    in_string = None;
                }
            }
            None => match b {
                b'"' | b'\'' => in_string = Some(b),
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => {
                    out.push(s[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            },
        }
    }
    out.push(s[start..].trim());
    out
}

/// Раскрывает все `env(name [<index>...]?, fallback?)` в value.
/// CSS Environment Variables L1: `env()` — это var()-подобная подстановка
/// из UA-supplied registry. Имена не имеют `--` префикса (env-имена —
/// `safe-area-inset-top`, `viewport-segment-width` и т.д.).
///
/// Phase 0: registry — пустой `HashMap`, все env-вызовы попадают в
/// fallback. Это даёт корректное `padding: env(safe-area-inset-top, 0px)`
/// → `padding: 0px`. Indices (`env(name 0 1, fallback)`) парсятся, но
/// игнорируются (используется только name до пробела).
fn expand_env_vars(
    value: &str,
    env_registry: &HashMap<String, String>,
    depth: u32,
) -> Option<String> {
    if depth > VAR_EXPAND_MAX_DEPTH {
        return None;
    }
    let Some(start) = find_env_open(value) else {
        return Some(value.to_string());
    };
    let prefix = &value[..start];
    let after_open = &value[start + 4..]; // skip "env("
    let (args, after_close) = parse_balanced_to_close(after_open)?;
    let (name_part, fallback) = split_var_args(args);
    // Indices в name-part: `safe-area-inset-top` или `viewport-segment-width 0 0`.
    let env_name = name_part.split_whitespace().next().unwrap_or("");
    if env_name.is_empty() {
        return None;
    }
    let resolved = if let Some(v) = env_registry.get(env_name) {
        expand_env_vars(v.trim(), env_registry, depth + 1)?
    } else {
        let fb = fallback?;
        expand_env_vars(fb.trim(), env_registry, depth + 1)?
    };
    let combined = format!("{prefix}{resolved}{after_close}");
    expand_env_vars(&combined, env_registry, depth + 1)
}

/// Аналог `find_var_open` для `env(`. Учитывает строковые литералы.
fn find_env_open(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_string: Option<u8> = None;
    while i + 4 <= bytes.len() {
        let b = bytes[i];
        match (in_string, b) {
            (Some(q), c) if c == q => {
                in_string = None;
                i += 1;
            }
            (None, b'"') | (None, b'\'') => {
                in_string = Some(b);
                i += 1;
            }
            (None, b'e') if &bytes[i..i + 4] == b"env(" => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// UA env-registry. Phase 0: пустой; вызовы `env(name, fallback)`
/// возвращают fallback. В Phase 2+ значения будут заполняться shell-ом
/// из реального viewport state (safe-area, виртуальная клавиатура).
fn empty_env_registry() -> HashMap<String, String> {
    HashMap::new()
}

/// CSS dimension units recognised in `attr(<name> <unit>)` substitution.
/// When the type annotation matches one of these, the attribute value (a
/// numeric string) gets the unit appended: `attr(data-w px)` with
/// `data-w="100"` → `"100px"`.
const ATTR_UNIT_SUFFIXES: &[&str] = &[
    "px", "em", "rem", "ex", "ch", "vw", "vh", "vmin", "vmax",
    "cm", "mm", "in", "pt", "pc", "q",
    "%",
    "deg", "rad", "grad", "turn",
    "s", "ms",
    "hz", "khz",
    "dpi", "dpcm", "dppx",
    "fr",
];

/// Finds the byte offset of the first `attr(` token in `s` that is
/// outside string literals (single or double quotes).
fn find_attr_open(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_string: Option<u8> = None;
    while i + 5 <= bytes.len() {
        let b = bytes[i];
        match (in_string, b) {
            (Some(q), c) if c == q => {
                in_string = None;
                i += 1;
            }
            (None, b'"') | (None, b'\'') => {
                in_string = Some(b);
                i += 1;
            }
            (None, b'a') if bytes[i..].starts_with(b"attr(") => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// CSS Values L4 §7.7 — `attr()` typed substitution.
///
/// Expands the first `attr(...)` occurrence in `value` using the DOM
/// attribute values of `node`. Supports the full L4 typed form:
///
/// ```text
/// attr( <attr-name> [ <type> ]? [ , <fallback> ]? )
/// ```
///
/// * If `<type>` is a CSS dimension unit (`px`, `em`, `%`, …), the
///   attribute value is concatenated with the unit: `attr(data-w px)` with
///   `data-w="100"` → `"100px"`.
/// * Otherwise (`color`, `string`, `integer`, `number`, …) the attribute
///   value is used verbatim as a CSS token.
/// * If the attribute is absent: use `<fallback>` when present, else
///   `None` (declaration treated as invalid per CSS Values L4 §7.7.1).
pub(in crate::style) fn expand_attr_val(value: &str, doc: &Document, node: NodeId) -> Option<String> {
    let start = find_attr_open(value)?;
    let prefix = &value[..start];
    let after_open = &value[start + 5..]; // skip "attr("
    let (args, after_close) = parse_balanced_to_close(after_open)?;

    // Split by the first top-level comma to separate spec from fallback.
    let (attr_spec, fallback_opt) = split_var_args(args);

    // attr_spec: "<name>" or "<name> <type>".
    let mut parts = attr_spec.splitn(2, |c: char| c.is_ascii_whitespace());
    let attr_name = parts.next().unwrap_or("").trim();
    let attr_type = parts.next().unwrap_or("").trim().to_ascii_lowercase();

    if attr_name.is_empty() {
        return None;
    }

    let node_ref = doc.get(node);
    let resolved: String = if let Some(raw_val) = node_ref.get_attr(attr_name) {
        let raw = raw_val.trim();
        if attr_type.is_empty() || attr_type == "string" {
            // CSS Values L4 §7.7.1: default type is `string` — return a CSS
            // string literal so downstream parsers (e.g. `content`) treat the
            // value as a quoted string token rather than an identifier.
            // Escape embedded double-quotes and backslashes per CSS §9.
            let escaped = raw.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        } else if ATTR_UNIT_SUFFIXES.contains(&attr_type.as_str()) {
            // Numeric attribute value + CSS unit.
            format!("{raw}{attr_type}")
        } else {
            // color, integer, number, length, angle, time, frequency, url …
            // Treat as a raw CSS token; the property-specific parser handles it.
            raw.to_string()
        }
    } else {
        // Attribute absent — use fallback or signal invalid.
        let fb = fallback_opt?;
        fb.to_string()
    };

    let combined = format!("{prefix}{resolved}{after_close}");
    // There may be more attr() in the combined result.
    if combined.contains("attr(") {
        expand_attr_val(&combined, doc, node)
    } else {
        Some(combined)
    }
}

/// Находит позицию первого `var(` в `s` вне строковых литералов. Возвращает
/// индекс символа `v`. Учитывает одинарные и двойные кавычки, чтобы
/// `content: "var(x)"` не давал ложного матча.
fn find_var_open(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_string: Option<u8> = None;
    while i + 4 <= bytes.len() {
        let b = bytes[i];
        match (in_string, b) {
            (Some(q), c) if c == q => {
                in_string = None;
                i += 1;
            }
            (None, b'"') | (None, b'\'') => {
                in_string = Some(b);
                i += 1;
            }
            (None, b'v') if &bytes[i..i + 4] == b"var(" => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Принимает строку, начинающуюся **сразу после** `var(`, и читает её до
/// парной закрывающей скобки с учётом вложенных `(...)` и строковых литералов.
/// Возвращает (содержимое внутри `var(...)`, остаток после `)`).
fn parse_balanced_to_close(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut depth = 1u32;
    let mut in_string: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match (in_string, b) {
            (Some(q), c) if c == q => in_string = None,
            (None, b'"') | (None, b'\'') => in_string = Some(b),
            (None, b'(') => depth += 1,
            (None, b')') => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Разбивает аргументы `var(...)` на (имя, опциональный fallback) по первой
/// top-level запятой. Запятые внутри вложенных скобок или строк — не граница.
fn split_var_args(s: &str) -> (&str, Option<&str>) {
    let bytes = s.as_bytes();
    let mut depth = 0u32;
    let mut in_string: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match (in_string, b) {
            (Some(q), c) if c == q => in_string = None,
            (None, b'"') | (None, b'\'') => in_string = Some(b),
            (None, b'(') => depth += 1,
            (None, b')') => depth = depth.saturating_sub(1),
            (None, b',') if depth == 0 => {
                return (s[..i].trim(), Some(s[i + 1..].trim()));
            }
            _ => {}
        }
    }
    (s.trim(), None)
}
