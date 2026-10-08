//! Строгая проверка грамматики grid-свойств для inline-`style` CSSOM (BUG-1315).
//!
//! Разборщики каскада (`GridTrackSize::parse_track_list`, `GridLine::parse`, …) мягкие:
//! они возвращают то, что смогли прочитать, и не отличают «нет значения» от «значение
//! не по грамматике». Для `element.style.<prop> = …` нужно обратное — CSSOM §6.7.2:
//! значение, не проходящее грамматику, не меняет декларацию. Здесь только проверка
//! синтаксиса (CSS Grid L1/L2 §7–§8); значение, прошедшее проверку, возвращается как есть
//! (обрезанное по краям) — каноническая сериализация (`*-valid` / `*-computed`) отдельная задача.
//!
//! Покрыто: `grid-template-{columns,rows,areas}`, `grid-auto-{columns,rows,flow}`,
//! `grid-{row,column}-{start,end}`, `grid-{row,column,area}`, шортхенды `grid-template`
//! и `grid`, а также `flex-grow`/`flex-shrink` (`<number [0,∞]>`) и `flow-tolerance`.

use std::collections::HashMap;

use crate::style::values::length::canonical_specified_length;

/// Лексема значения. Строки и имена линий отделены от слов, функции несут исходный текст.
enum Tok<'a> {
    /// Идентификатор, число, размерность или процент.
    Word(&'a str),
    /// `name(args)`: имя в нижнем регистре, содержимое скобок, исходный текст целиком.
    Func { name: String, args: &'a str, raw: &'a str },
    /// `[ident*]` — содержимое скобок.
    Names(&'a str),
    /// `"…"` / `'…'` — содержимое без кавычек.
    Str(&'a str),
    /// Верхнеуровневый `/`.
    Slash,
}

/// Индекс закрывающей скобки для открытой на позиции `open` (с учётом вложенности и строк).
/// Конец ввода закрывает открытые скобки (CSS Syntax §5.4.7): тогда `s.len()`.
fn matching_paren(s: &str, open: usize) -> usize {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut quote = 0u8;
    let mut i = open;
    while i < bytes.len() {
        let b = bytes[i];
        if quote != 0 {
            if b == b'\\' {
                i += 1;
            } else if b == quote {
                quote = 0;
            }
        } else {
            match b {
                b'"' | b'\'' => quote = b,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return i;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    s.len()
}

/// Разбивает значение на лексемы; `None` — незакрытая `[`/строка или лишняя `]`/`)`.
fn tokenize(s: &str) -> Option<Vec<Tok<'_>>> {
    let bytes = s.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if s[i..].starts_with("/*") {
            i = s[i + 2..].find("*/").map_or(s.len(), |k| i + 2 + k + 2);
            continue;
        }
        match b {
            b'/' => {
                toks.push(Tok::Slash);
                i += 1;
            }
            b'[' => {
                let close = s[i + 1..].find(']')? + i + 1;
                toks.push(Tok::Names(&s[i + 1..close]));
                i = close + 1;
            }
            b'"' | b'\'' => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != b {
                    if bytes[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                if j >= bytes.len() {
                    return None;
                }
                toks.push(Tok::Str(&s[i + 1..j]));
                i = j + 1;
            }
            b']' | b')' => return None,
            _ => {
                let mut j = i;
                while j < bytes.len() {
                    let c = bytes[j];
                    if c.is_ascii_whitespace() || matches!(c, b'/' | b'[' | b']' | b'"' | b'\'' | b'(' | b')') {
                        break;
                    }
                    if c == b'\\' {
                        let rest: Vec<char> = s[j..].chars().take(8).collect();
                        let n = escape_len(&rest)?;
                        j += rest[..n].iter().map(|c| c.len_utf8()).sum::<usize>();
                    } else {
                        j += 1;
                    }
                }
                if j > i && j < bytes.len() && bytes[j] == b'(' {
                    let close = matching_paren(s, j);
                    let end = (close + 1).min(s.len());
                    toks.push(Tok::Func {
                        name: s[i..j].to_ascii_lowercase(),
                        args: &s[j + 1..close],
                        raw: &s[i..end],
                    });
                    i = end;
                } else if j == i {
                    return None;
                } else {
                    toks.push(Tok::Word(&s[i..j]));
                    i = j;
                }
            }
        }
    }
    Some(toks)
}

/// Делит содержимое функции по верхнеуровневым запятым.
fn split_commas(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let (mut depth, mut start, mut quote) = (0usize, 0usize, 0u8);
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if quote != 0 {
            if b == b'\\' {
                i += 1;
            } else if b == quote {
                quote = 0;
            }
        } else {
            match b {
                b'"' | b'\'' => quote = b,
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => {
                    parts.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

/// `<number>` по CSS Syntax: `[+-]? (\d+ | \d* \. \d+) ([eE] [+-]? \d+)?`.
fn parse_number(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && matches!(b[i], b'+' | b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - f;
        if frac_digits == 0 {
            return None;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return None;
    }
    if i < b.len() && matches!(b[i], b'e' | b'E') {
        i += 1;
        if i < b.len() && matches!(b[i], b'+' | b'-') {
            i += 1;
        }
        let e = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == e {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    s.parse::<f64>().ok()
}

/// `<integer>`: `[+-]? \d+`.
fn parse_integer(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Значение за пределами i64 в любом случае далеко за пределами `[1, ∞)`-ограничений грамматики
    // по смыслу, но синтаксически остаётся целым — насыщаем, а не отклоняем.
    Some(s.parse::<i64>().unwrap_or(if s.starts_with('-') { i64::MIN } else { i64::MAX }))
}

/// Длина CSS-escape в начале `cs` (обратная косая черта первым символом): она + 1–6 hex-цифр + один необязательный
/// пробельный символ, либо `\` + любой другой символ; `None` — `\` в конце ввода или перед переводом строки.
fn escape_len(cs: &[char]) -> Option<usize> {
    let next = *cs.get(1)?;
    if next == '\n' {
        return None;
    }
    if !next.is_ascii_hexdigit() {
        return Some(2);
    }
    let hex = cs[1..].iter().take(6).take_while(|c| c.is_ascii_hexdigit()).count();
    let ws = cs.get(1 + hex).is_some_and(|c| c.is_ascii_whitespace());
    Some(1 + hex + usize::from(ws))
}

/// Символ имени: буква, цифра, `-`, `_`, не-ASCII.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

/// Сколько символов `cs` занимает последовательность «символов имени» с escape'ами; `None` — битый escape.
fn name_len(cs: &[char]) -> Option<usize> {
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '\\' {
            i += escape_len(&cs[i..])?;
        } else if is_name_char(cs[i]) {
            i += 1;
        } else {
            break;
        }
    }
    Some(i)
}

/// `<ident>` по синтаксису: не начинается с цифры и не с `-цифра`; escape'ы допустимы.
fn is_ident(s: &str) -> bool {
    let cs: Vec<char> = s.chars().collect();
    let mut start = 0;
    if cs.first() == Some(&'-') {
        start = 1;
    }
    match cs.get(start) {
        Some(c) if c.is_ascii_alphabetic() || *c == '_' || !c.is_ascii() || (*c == '-' && start == 1) || *c == '\\' => {}
        _ => return false,
    }
    name_len(&cs) == Some(cs.len())
}

/// `<custom-ident>` в грамматике grid: ключевые слова CSS-wide, `default`, `span`, `auto`
/// не допускаются (CSS Grid L1 §8.3 / §7.2.1).
fn is_grid_ident(s: &str) -> bool {
    is_ident(s)
        && !["span", "auto", "default", "initial", "inherit", "unset", "revert", "revert-layer"]
            .iter()
            .any(|k| s.eq_ignore_ascii_case(k))
}

/// `[a b c]` с допустимыми именами (пустое содержимое — тоже допустимо).
fn valid_names(inner: &str) -> bool {
    inner.split_ascii_whitespace().all(is_grid_ident)
}

/// `<length-percentage [0,∞]>`, включая математические функции.
fn is_length_percentage(s: &str) -> bool {
    canonical_specified_length(s, false, true).is_some()
}

/// Класс одиночного размера дорожки.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Breadth {
    /// `<length-percentage>`.
    Fixed,
    /// `<flex>`.
    Flex,
    /// `min-content` / `max-content` / `auto`.
    Intrinsic,
}

/// `<fixed-breadth> | <flex> | min-content | max-content | auto` как слово или математическая функция.
fn breadth(tok: &Tok<'_>) -> Option<Breadth> {
    match tok {
        Tok::Word(w) => {
            if is_length_percentage(w) {
                return Some(Breadth::Fixed);
            }
            if w.eq_ignore_ascii_case("min-content")
                || w.eq_ignore_ascii_case("max-content")
                || w.eq_ignore_ascii_case("auto")
            {
                return Some(Breadth::Intrinsic);
            }
            let n = w.len().checked_sub(2).filter(|&k| w.is_char_boundary(k) && w[k..].eq_ignore_ascii_case("fr"))?;
            (parse_number(&w[..n])? >= 0.0).then_some(Breadth::Flex)
        }
        Tok::Func { raw, name, .. } if name != "minmax" && name != "fit-content" && name != "repeat" => {
            is_length_percentage(raw).then_some(Breadth::Fixed)
        }
        _ => None,
    }
}

/// Один аргумент функции как единственная лексема.
fn single_tok(arg: &str) -> Option<Tok<'_>> {
    let mut toks = tokenize(arg)?;
    if toks.len() == 1 { toks.pop() } else { None }
}

/// Разобранный `<track-size>`: `fixed` — подходит под `<fixed-size>`, `bare_flex` — голый `<flex>`.
#[derive(Clone, Copy)]
struct TrackSz {
    fixed: bool,
    bare_flex: bool,
}

/// `<track-size> = <track-breadth> | minmax(<inflexible-breadth>, <track-breadth>) | fit-content(<length-percentage>)`.
fn track_size(tok: &Tok<'_>) -> Option<TrackSz> {
    match tok {
        Tok::Func { name, args, .. } if name == "minmax" => {
            let parts = split_commas(args);
            if parts.len() != 2 {
                return None;
            }
            let min = breadth(&single_tok(parts[0])?)?;
            let max = breadth(&single_tok(parts[1])?)?;
            if min == Breadth::Flex {
                return None;
            }
            Some(TrackSz {
                fixed: min == Breadth::Fixed || max == Breadth::Fixed,
                bare_flex: false,
            })
        }
        Tok::Func { name, args, .. } if name == "fit-content" => {
            let single = single_tok(args)?;
            let ok = match &single {
                Tok::Word(w) => is_length_percentage(w),
                Tok::Func { raw, .. } => is_length_percentage(raw),
                _ => false,
            };
            ok.then_some(TrackSz { fixed: false, bare_flex: false })
        }
        _ => {
            let b = breadth(tok)?;
            Some(TrackSz { fixed: b == Breadth::Fixed, bare_flex: b == Breadth::Flex })
        }
    }
}

/// Содержимое `repeat()` после первой запятой: `(<line-names>? <track-size>)+ <line-names>?`.
/// Возвращает `(все ли размеры fixed, есть ли голый flex)`.
fn repeat_body(body: &str) -> Option<(bool, bool)> {
    let toks = tokenize(body)?;
    let (mut sizes, mut all_fixed, mut any_flex, mut prev_names) = (0usize, true, false, false);
    for t in &toks {
        match t {
            Tok::Names(n) => {
                if prev_names || !valid_names(n) {
                    return None;
                }
                prev_names = true;
            }
            _ => {
                let sz = track_size(t)?;
                all_fixed &= sz.fixed;
                any_flex |= sz.bare_flex;
                sizes += 1;
                prev_names = false;
            }
        }
    }
    (sizes > 0).then_some((all_fixed, any_flex))
}

/// `<track-list> | <auto-track-list>` (CSS Grid L1 §7.2, интринсивные размеры в `auto-fill`
/// допускаются по Grid L2).
fn valid_track_list(toks: &[Tok<'_>]) -> bool {
    let (mut tracks, mut auto_repeats, mut nonfixed_outside, mut prev_names) = (0usize, 0usize, false, false);
    for t in toks {
        match t {
            Tok::Names(n) => {
                if prev_names || !valid_names(n) {
                    return false;
                }
                prev_names = true;
                continue;
            }
            Tok::Func { name, args, .. } if name == "repeat" => {
                let parts = split_commas(args);
                if parts.len() < 2 {
                    return false;
                }
                let count = parts[0].trim();
                // Тело — всё после первой запятой (запятых в теле быть не может: размеры без запятых).
                let body = &args[parts[0].len() + 1..];
                let Some((all_fixed, any_flex)) = repeat_body(body) else { return false };
                if count.eq_ignore_ascii_case("auto-fill") || count.eq_ignore_ascii_case("auto-fit") {
                    // `repeat(auto-*, …)`: голый `<flex>` внутри недопустим.
                    if any_flex {
                        return false;
                    }
                    auto_repeats += 1;
                } else {
                    match parse_integer(count) {
                        Some(n) if n >= 1 => {}
                        _ => return false,
                    }
                    nonfixed_outside |= !all_fixed;
                }
            }
            _ => {
                let Some(sz) = track_size(t) else { return false };
                nonfixed_outside |= !sz.fixed;
            }
        }
        tracks += 1;
        prev_names = false;
    }
    tracks > 0 && auto_repeats <= 1 && !(auto_repeats == 1 && nonfixed_outside)
}

/// `subgrid <line-name-list>?` — `toks[0]` уже `subgrid`.
fn valid_subgrid(rest: &[Tok<'_>]) -> bool {
    let mut auto_repeats = 0;
    for t in rest {
        match t {
            Tok::Names(n) => {
                if !valid_names(n) {
                    return false;
                }
            }
            Tok::Func { name, args, .. } if name == "repeat" => {
                let parts = split_commas(args);
                if parts.len() != 2 {
                    return false;
                }
                let count = parts[0].trim();
                if count.eq_ignore_ascii_case("auto-fill") {
                    auto_repeats += 1;
                } else if !matches!(parse_integer(count), Some(n) if n >= 1) {
                    return false;
                }
                let Some(body) = tokenize(parts[1]) else { return false };
                if body.is_empty() || !body.iter().all(|b| matches!(b, Tok::Names(n) if valid_names(n))) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    auto_repeats <= 1
}

/// `grid-template-columns` / `grid-template-rows` (и соответствующая половина шортхендов).
fn valid_template_axis(toks: &[Tok<'_>]) -> bool {
    match toks {
        [] => false,
        [Tok::Word(w)] if w.eq_ignore_ascii_case("none") || w.eq_ignore_ascii_case("masonry") => true,
        [Tok::Word(w), rest @ ..] if w.eq_ignore_ascii_case("subgrid") => valid_subgrid(rest),
        _ => valid_track_list(toks),
    }
}

/// `grid-auto-columns` / `grid-auto-rows`: `<track-size>+`.
fn valid_auto_tracks(toks: &[Tok<'_>]) -> bool {
    !toks.is_empty() && toks.iter().all(|t| track_size(t).is_some())
}

/// `[row | column] || dense`.
fn valid_auto_flow(toks: &[Tok<'_>]) -> bool {
    let (mut axis, mut dense) = (0, 0);
    for t in toks {
        match t {
            Tok::Word(w) if w.eq_ignore_ascii_case("row") || w.eq_ignore_ascii_case("column") => axis += 1,
            Tok::Word(w) if w.eq_ignore_ascii_case("dense") => dense += 1,
            _ => return false,
        }
    }
    axis <= 1 && dense <= 1 && axis + dense > 0
}

/// Одна строка `grid-template-areas`: ячейки `.`-последовательности или идентификаторы.
fn area_cells(row: &str) -> Option<Vec<Option<&str>>> {
    let cells: Vec<_> = row.split_ascii_whitespace().collect();
    if cells.is_empty() {
        return None;
    }
    cells
        .into_iter()
        .map(|c| {
            if c.bytes().all(|b| b == b'.') {
                Some(None)
            } else if name_len(&c.chars().collect::<Vec<_>>()) == Some(c.chars().count()) {
                Some(Some(c))
            } else {
                None
            }
        })
        .collect()
}

/// Набор строк областей: одинаковая ширина, каждая именованная область — заполненный прямоугольник.
fn valid_area_rows(rows: &[&str]) -> bool {
    let mut grid = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(cells) = area_cells(r) else { return false };
        if grid.first().is_some_and(|first: &Vec<_>| first.len() != cells.len()) {
            return false;
        }
        grid.push(cells);
    }
    // имя → (мин. строка, макс. строка, мин. столбец, макс. столбец, ячеек)
    let mut boxes: HashMap<&str, (usize, usize, usize, usize, usize)> = HashMap::new();
    for (ri, row) in grid.iter().enumerate() {
        for (ci, cell) in row.iter().enumerate() {
            if let Some(name) = cell {
                let e = boxes.entry(name).or_insert((ri, ri, ci, ci, 0));
                e.0 = e.0.min(ri);
                e.1 = e.1.max(ri);
                e.2 = e.2.min(ci);
                e.3 = e.3.max(ci);
                e.4 += 1;
            }
        }
    }
    boxes.values().all(|&(r0, r1, c0, c1, n)| (r1 - r0 + 1) * (c1 - c0 + 1) == n)
}

/// `grid-template-areas`: `none | <string>+`.
fn valid_template_areas(toks: &[Tok<'_>]) -> bool {
    if let [Tok::Word(w)] = toks {
        return w.eq_ignore_ascii_case("none");
    }
    let mut rows = Vec::new();
    for t in toks {
        match t {
            Tok::Str(s) => rows.push(*s),
            _ => return false,
        }
    }
    !rows.is_empty() && valid_area_rows(&rows)
}

/// Часть `grid-row`/`grid-column`/`grid-area` — одно `<grid-line>`.
fn valid_grid_line(toks: &[Tok<'_>]) -> bool {
    // Математическая функция вместо `<integer>` не проверяется (знак результата известен только при вычислении).
    let mut words = Vec::with_capacity(toks.len());
    for t in toks {
        match t {
            Tok::Word(w) => words.push(*w),
            Tok::Func { raw, name, .. } if is_math_fn(name) => words.push(*raw),
            _ => return false,
        }
    }
    let is_math = |w: &str| w.contains('(');
    let nonzero = |w: &str| is_math(w) || matches!(parse_integer(w), Some(n) if n != 0);
    let positive = |w: &str| is_math(w) || matches!(parse_integer(w), Some(n) if n >= 1);
    let spans: Vec<usize> =
        words.iter().enumerate().filter(|(_, w)| w.eq_ignore_ascii_case("span")).map(|(i, _)| i).collect();
    match spans.as_slice() {
        [] => match words.as_slice() {
            [w] => w.eq_ignore_ascii_case("auto") || is_grid_ident(w) || nonzero(w),
            [a, b] => (nonzero(a) && is_grid_ident(b)) || (is_grid_ident(a) && nonzero(b)),
            _ => false,
        },
        // `span && [<integer> || <custom-ident>]`: `span` — с одного из краёв, группа справа/слева целиком.
        [at] if *at == 0 || *at == words.len() - 1 => {
            let rest: Vec<&str> = words.iter().enumerate().filter(|(i, _)| i != at).map(|(_, w)| *w).collect();
            match rest.as_slice() {
                [w] => positive(w) || is_grid_ident(w),
                [a, b] => (positive(a) && is_grid_ident(b)) || (is_grid_ident(a) && positive(b)),
                _ => false,
            }
        }
        _ => false,
    }
}

/// Математическая функция CSS Values L4, которая может стоять на месте `<integer>`.
fn is_math_fn(name: &str) -> bool {
    matches!(name, "calc" | "min" | "max" | "clamp" | "round" | "mod" | "rem")
}

/// Список `<grid-line> [ / <grid-line> ]*` с числом частей в `1..=max_parts`.
fn valid_grid_lines(toks: &[Tok<'_>], max_parts: usize) -> bool {
    let parts: Vec<&[Tok<'_>]> = toks.split(|t| matches!(t, Tok::Slash)).collect();
    parts.len() <= max_parts && parts.iter().all(|p| valid_grid_line(p))
}

/// Делит лексемы по единственному верхнеуровневому `/`.
fn split_slash<'t, 'a>(toks: &'t [Tok<'a>]) -> Option<(&'t [Tok<'a>], &'t [Tok<'a>])> {
    let mut at = toks.iter().enumerate().filter(|(_, t)| matches!(t, Tok::Slash)).map(|(i, _)| i);
    let first = at.next()?;
    at.next().is_none().then(|| (&toks[..first], &toks[first + 1..]))
}

/// Форма шортхенда с `grid-template-areas`:
/// `[<line-names>? <string> <track-size>? <line-names>?]+ [/ <explicit-track-list>]?`.
fn valid_template_areas_shorthand(toks: &[Tok<'_>]) -> bool {
    let mut rows = Vec::new();
    let mut i = 0;
    let at_names = |i: usize| matches!(toks.get(i), Some(Tok::Names(n)) if valid_names(n));
    loop {
        if at_names(i) {
            i += 1;
        }
        let Some(Tok::Str(s)) = toks.get(i) else { return false };
        rows.push(*s);
        i += 1;
        if let Some(t) = toks.get(i)
            && !matches!(t, Tok::Names(_) | Tok::Str(_) | Tok::Slash)
        {
            if track_size(t).is_none() {
                return false;
            }
            i += 1;
        }
        if at_names(i) {
            i += 1;
        }
        if i >= toks.len() || matches!(toks[i], Tok::Slash) {
            break;
        }
    }
    if !valid_area_rows(&rows) {
        return false;
    }
    if i >= toks.len() {
        return true;
    }
    // `/ <explicit-track-list>` = `[<line-names>? <track-size>]+ <line-names>?`.
    let list = &toks[i + 1..];
    let mut prev_names = false;
    let mut sizes = 0;
    for t in list {
        match t {
            Tok::Names(n) if !prev_names && valid_names(n) => prev_names = true,
            Tok::Names(_) | Tok::Str(_) | Tok::Slash => return false,
            _ => {
                if track_size(t).is_none() {
                    return false;
                }
                sizes += 1;
                prev_names = false;
            }
        }
    }
    sizes > 0
}

/// `grid-template`: `none | <rows> / <columns> | areas-форма`.
fn valid_template_shorthand(toks: &[Tok<'_>]) -> bool {
    if toks.iter().any(|t| matches!(t, Tok::Str(_))) {
        return valid_template_areas_shorthand(toks);
    }
    if let [Tok::Word(w)] = toks {
        return w.eq_ignore_ascii_case("none");
    }
    match split_slash(toks) {
        Some((rows, cols)) => valid_template_axis(rows) && valid_template_axis(cols),
        None => false,
    }
}

/// `[auto-flow && dense?] <track-size>*` (половина шортхенда `grid` с автопотоком).
fn valid_auto_flow_half(toks: &[Tok<'_>]) -> bool {
    let (mut flow, mut dense, mut lead) = (0, 0, 0);
    for t in toks {
        match t {
            Tok::Word(w) if w.eq_ignore_ascii_case("auto-flow") => flow += 1,
            Tok::Word(w) if w.eq_ignore_ascii_case("dense") => dense += 1,
            _ => break,
        }
        lead += 1;
    }
    flow == 1 && dense <= 1 && toks[lead..].iter().all(|t| track_size(t).is_some())
}

/// `grid`: `<grid-template> | <rows> / auto-flow… | auto-flow… / <columns>`.
fn valid_grid_shorthand(toks: &[Tok<'_>]) -> bool {
    if valid_template_shorthand(toks) {
        return true;
    }
    match split_slash(toks) {
        Some((l, r)) => {
            (valid_template_axis(l) && valid_auto_flow_half(r)) || (valid_auto_flow_half(l) && valid_template_axis(r))
        }
        None => false,
    }
}

/// `flex-grow` / `flex-shrink`: `<number [0,∞]>`; математические функции не проверяются.
fn valid_flex_factor(toks: &[Tok<'_>]) -> bool {
    match toks {
        [Tok::Word(w)] => parse_number(w).is_some_and(|n| n >= 0.0),
        [Tok::Func { name, .. }] => is_math_fn(name),
        _ => false,
    }
}

/// `flow-tolerance`: `normal | <length-percentage [0,∞]> | infinite`.
fn valid_flow_tolerance(toks: &[Tok<'_>]) -> bool {
    match toks {
        [Tok::Word(w)] => {
            w.eq_ignore_ascii_case("normal") || w.eq_ignore_ascii_case("infinite") || is_length_percentage(w)
        }
        [Tok::Func { raw, .. }] => is_length_percentage(raw),
        _ => false,
    }
}

/// Значение без хвостового `!important` (регистр и пробелы вокруг `!` не важны).
fn strip_important(v: &str) -> &str {
    let t = v.trim_end();
    if t.len() >= 9
        && t.is_char_boundary(t.len() - 9)
        && t[t.len() - 9..].eq_ignore_ascii_case("important")
        && let Some(head) = t[..t.len() - 9].trim_end().strip_suffix('!')
    {
        return head;
    }
    v
}

/// CSSOM-проверка значения `value` свойства `prop` из семейства grid (BUG-1315).
///
/// `Some(значение без пробелов по краям)` — значение проходит грамматику (или `prop` не из
/// этого семейства, и проверять нечего); `None` — значение невалидно, присваивание в
/// `element.style` должно быть отброшено. CSS-wide ключевые слова и `var()`/`env()` сюда не
/// попадают: их обрабатывает вызывающая сторона до этой проверки.
pub fn canonical_specified_grid(prop: &str, value: &str) -> Option<String> {
    let v = value.trim();
    let family = matches!(
        prop,
        "grid-template-columns"
            | "grid-template-rows"
            | "grid-template-areas"
            | "grid-auto-columns"
            | "grid-auto-rows"
            | "grid-auto-flow"
            | "grid-row-start"
            | "grid-row-end"
            | "grid-column-start"
            | "grid-column-end"
            | "grid-row"
            | "grid-column"
            | "grid-area"
            | "grid-template"
            | "grid"
            | "flex-grow"
            | "flex-shrink"
            | "flow-tolerance"
    );
    if !family {
        return Some(v.to_string());
    }
    // Приоритет не часть значения: `style="grid-row: 1 / 3 !important"` остаётся как написано.
    let toks = tokenize(strip_important(v))?;
    let ok = match prop {
        "grid-template-columns" | "grid-template-rows" => valid_template_axis(&toks),
        "grid-template-areas" => valid_template_areas(&toks),
        "grid-auto-columns" | "grid-auto-rows" => valid_auto_tracks(&toks),
        "grid-auto-flow" => valid_auto_flow(&toks),
        "grid-row-start" | "grid-row-end" | "grid-column-start" | "grid-column-end" => valid_grid_lines(&toks, 1),
        "grid-row" | "grid-column" => valid_grid_lines(&toks, 2),
        "grid-area" => valid_grid_lines(&toks, 4),
        "grid-template" => valid_template_shorthand(&toks),
        "grid" => valid_grid_shorthand(&toks),
        "flex-grow" | "flex-shrink" => valid_flex_factor(&toks),
        _ => valid_flow_tolerance(&toks),
    };
    ok.then(|| v.to_string())
}
