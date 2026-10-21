//! Каноническая запись `scroll-state(…)` в прелюдии `@container` (`conditionText`).
//!
//! CSS Conditional 5 §«scroll-state()»: корректная функция сериализуется в
//! нижнем регистре и с одним пробелом после `:`; всё, что не укладывается в
//! грамматику `<scroll-state-query>`, — `<general-enclosed>` и остаётся
//! в исходной записи.

/// Прелюдия, в которой каждая `scroll-state(…)` заменена меткой, и записи этих функций
/// (корректная — канонически, остальные — как написаны, с пробелами). Метки не содержат
/// пробелов и двоеточий, поэтому схлопывание пробелов и `space_after_colons` их не
/// трогают; [`restore_scroll_state_functions`] подставляет записи назад.
pub(super) fn extract_scroll_state_functions(prelude: &str) -> (String, Vec<String>) {
    const NAME: &str = "scroll-state(";
    let lower = prelude.to_ascii_lowercase();
    let mut out = String::with_capacity(prelude.len());
    let mut funcs: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(rel) = lower[i..].find(NAME) {
        let at = i + rel;
        let glued = prelude[..at].chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        let body_start = at + NAME.len();
        let Some(body_end) = matching_paren(prelude, body_start) else { break };
        if glued {
            out.push_str(&prelude[i..body_start]);
            i = body_start;
            continue;
        }
        out.push_str(&prelude[i..at]);
        funcs.push(match query(&prelude[body_start..body_end]) {
            Some(canon) => format!("scroll-state({canon})"),
            None => prelude[at..=body_end].to_string(),
        });
        out.push_str(&format!("{MARK_OPEN}{}{MARK_CLOSE}", funcs.len() - 1));
        i = body_end + 1;
    }
    out.push_str(&prelude[i..]);
    (out, funcs)
}

const MARK_OPEN: char = '\u{E000}';
const MARK_CLOSE: char = '\u{E001}';

/// Подставляет записи функций вместо меток [`extract_scroll_state_functions`].
pub(super) fn restore_scroll_state_functions(text: &str, funcs: &[String]) -> String {
    let mut out = text.to_string();
    for (n, f) in funcs.iter().enumerate() {
        out = out.replace(&format!("{MARK_OPEN}{n}{MARK_CLOSE}"), f);
    }
    out
}

/// Индекс закрывающей `)` для скобки, открытой перед `from`.
fn matching_paren(s: &str, from: usize) -> Option<usize> {
    let mut depth = 1usize;
    for (i, c) in s[from..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(from + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// `not <in-parens>` | `<in-parens> [and <in-parens>]*` | `<in-parens> [or <in-parens>]*`
/// | `<feature>`; `None` — вне грамматики.
fn query(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(rest) = strip_word(s, "not") {
        return Some(format!("not {}", in_parens(rest.trim_start())?));
    }
    if !s.starts_with('(') {
        return feature(s);
    }
    let mut parts: Vec<String> = Vec::new();
    let mut op: Option<&str> = None;
    let mut rest = s;
    loop {
        let end = matching_paren(rest, 1)?;
        parts.push(in_parens(&rest[..=end])?);
        rest = rest[end + 1..].trim_start();
        if rest.is_empty() {
            break;
        }
        let word = ["and", "or"].into_iter().find(|w| strip_word(rest, w).is_some())?;
        if op.is_some_and(|o| o != word) {
            return None;
        }
        op = Some(word);
        rest = strip_word(rest, word)?.trim_start();
        if !rest.starts_with('(') {
            return None;
        }
    }
    Some(parts.join(&format!(" {} ", op.unwrap_or("and"))))
}

/// Слово `word` (без учёта регистра), за которым пробел; остаток после него.
fn strip_word<'a>(s: &'a str, word: &str) -> Option<&'a str> {
    let head = s.get(..word.len())?;
    let rest = &s[word.len()..];
    (head.eq_ignore_ascii_case(word) && rest.starts_with(char::is_whitespace)).then_some(rest)
}

/// `( <query> )` или `( <feature> )`.
fn in_parens(s: &str) -> Option<String> {
    let s = s.trim();
    let inner = s.strip_prefix('(')?;
    let end = matching_paren(s, 1)?;
    if end + 1 != s.len() {
        return None;
    }
    Some(format!("({})", query(&inner[..end - 1])?))
}

const DIRECTIONS: &[&str] = &["none", "top", "right", "bottom", "left", "block-start", "block-end", "inline-start", "inline-end"];
const AXES: &[&str] = &["x", "y", "block", "inline"];

/// `name` или `name: value` с допустимым значением; запись нормализована.
fn feature(s: &str) -> Option<String> {
    let (name, value) = match s.split_once(':') {
        Some((n, v)) => (n.trim(), Some(v.trim().to_ascii_lowercase())),
        None => (s.trim(), None),
    };
    let name = name.to_ascii_lowercase();
    let allowed = |extra: &[&str]| -> Option<()> {
        match value.as_deref() {
            None => Some(()),
            Some(v) => (DIRECTIONS.contains(&v) || extra.contains(&v)).then_some(()),
        }
    };
    match name.as_str() {
        "scrollable" | "scrolled" => allowed(AXES)?,
        "stuck" => allowed(&[])?,
        "snapped" => match value.as_deref() {
            None | Some("none" | "x" | "y" | "block" | "inline" | "both") => {}
            _ => return None,
        },
        _ => return None,
    }
    Some(match value {
        Some(v) => format!("{name}: {v}"),
        None => name,
    })
}

#[cfg(test)]
mod tests {
    fn norm(prelude: &str) -> String {
        let (text, funcs) = super::extract_scroll_state_functions(prelude);
        super::restore_scroll_state_functions(&text, &funcs)
    }

    #[test]
    fn canonical_form() {
        assert_eq!(norm("scroll-state(        scrollable:left)"), "scroll-state(scrollable: left)");
        assert_eq!(norm("scroll-STate(scrollable)"), "scroll-state(scrollable)");
        assert_eq!(
            norm("scroll-state(  ( scrollable: LEFT) OR ( SCROLLABLE: BOTTOM  ) )"),
            "scroll-state((scrollable: left) or (scrollable: bottom))"
        );
        assert_eq!(norm("name scroll-state(not ((stuck: top) AND (stuck: bottom)))"), "name scroll-state(not ((stuck: top) and (stuck: bottom)))");
    }

    #[test]
    fn invalid_stays_verbatim() {
        assert_eq!(norm("scroll-STate(scrollable:    )"), "scroll-STate(scrollable:    )");
        assert_eq!(norm("scroll-state (scrollable: right)"), "scroll-state (scrollable: right)");
        assert_eq!(norm("scroll-state(scrollable: auto)"), "scroll-state(scrollable: auto)");
        assert_eq!(norm("scroll-state((a: 1) and (b: 2) or (c: 3))"), "scroll-state((a: 1) and (b: 2) or (c: 3))");
        assert_eq!(norm("my-scroll-state(scrollable)"), "my-scroll-state(scrollable)");
    }
}
