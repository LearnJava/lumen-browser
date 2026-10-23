//! BUG-1430 — вычисление математических функций (CSS Values L4 §10) в
//! нечисловых по длине контекстах: `<number>`, `<integer>`, `<angle>`, `<time>`.
//!
//! `calc.rs` вычисляет выражение в `f32`-пиксели: единиц `s`/`ms`, констант
//! `pi`/`e`/`infinity`/`NaN`, `progress()` и типизации (число, угол, время) там нет.
//! Здесь — отдельный вычислитель с учётом размерности: он разбирает тот же поток
//! токенов (`tokenize_calc`), считает в `f64` и отдаёт значение вместе с размерностью.
//! [`substitute_numeric_math`] стоит в `apply_declaration` перед разбором свойства:
//! каждый внешний вызов math-функции с числовым, угловым или временным результатом
//! заменяется литералом (`calc(1s + 1s)` → `2s`), поэтому парсеры свойств
//! (`z-index`, `scale`, `transition-delay`, `rotate`, `transform: rotate(...)`…)
//! остаются без изменений.
//!
//! Длина без процентов подставляется, только если прежний разбор `calc.rs` выражение
//! не принимает (константы, `progress()`): корректные выражения остаются деревом
//! `Length::Calc`, чтобы `em`/`vw` пересчитывались как раньше.

use lumen_core::geom::Size;

use crate::style::calc::{
    calc_num_to_node, parse_math_function_value, tokenize_calc, CalcToken,
};
use crate::style::calc::CalcNode;

/// Показатели степени базовых единиц величины: длина (px), угол (deg), время (s),
/// процент. Число — все нули; `1px / 1s` — `len: 1, time: -1`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Dim {
    len: i8,
    ang: i8,
    time: i8,
    pct: i8,
}

const NUM: Dim = Dim { len: 0, ang: 0, time: 0, pct: 0 };
const ANG: Dim = Dim { len: 0, ang: 1, time: 0, pct: 0 };
const TIME: Dim = Dim { len: 0, ang: 0, time: 1, pct: 0 };
const LEN: Dim = Dim { len: 1, ang: 0, time: 0, pct: 0 };
const PCT: Dim = Dim { len: 0, ang: 0, time: 0, pct: 1 };

impl Dim {
    fn add(self, o: Dim) -> Dim {
        Dim {
            len: self.len + o.len,
            ang: self.ang + o.ang,
            time: self.time + o.time,
            pct: self.pct + o.pct,
        }
    }

    fn sub(self, o: Dim) -> Dim {
        Dim {
            len: self.len - o.len,
            ang: self.ang - o.ang,
            time: self.time - o.time,
            pct: self.pct - o.pct,
        }
    }
}

/// Значение с размерностью: длина в px, угол в градусах, время в секундах,
/// процент в единицах процента.
#[derive(Clone, Copy, Debug)]
struct Val {
    v: f64,
    d: Dim,
}

impl Val {
    fn num(v: f64) -> Val {
        Val { v, d: NUM }
    }
}

/// Имена math-функций, с которых начинается вычисляемый вызов.
const MATH_FNS: &[&str] = &[
    "calc", "min", "max", "clamp", "abs", "sign", "round", "mod", "rem", "sin", "cos", "tan",
    "asin", "acos", "atan", "atan2", "pow", "sqrt", "hypot", "log", "exp", "progress",
];

/// Предел длины в px при бесконечности: `LayoutUnit` Chrome (2^25 − 1).
const MAX_LENGTH_PX: f64 = 33_554_431.0;

/// Свойства `<integer>`: дробный результат округляется до ближайшего целого
/// (середина — к +∞, CSS Values L4 §10.1).
fn integer_property(prop: &str) -> bool {
    matches!(
        prop,
        "z-index"
            | "order"
            | "orphans"
            | "widows"
            | "column-count"
            | "flex-line-count"
            | "-webkit-line-clamp"
            | "line-clamp"
            | "grid-row"
            | "grid-column"
            | "grid-row-start"
            | "grid-row-end"
            | "grid-column-start"
            | "grid-column-end"
    )
}

struct Eval<'a> {
    toks: &'a [CalcToken],
    pos: usize,
    em: f32,
    viewport: Size,
    /// `false` — контекст без каскада (CSSOM-сеттер): `em`/`vw`… не свернуть.
    relative_units: bool,
    /// Выражение вышло за возможности вычислителя (неизвестная функция, константа,
    /// font-relative единица без каскада), а не нарушило грамматику.
    soft: bool,
}

impl Eval<'_> {
    fn peek(&self) -> Option<&CalcToken> {
        self.toks.get(self.pos)
    }

    fn eat(&mut self, t: &CalcToken) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> Option<Val> {
        let mut left = self.term()?;
        loop {
            let add = match self.peek() {
                Some(CalcToken::Plus) => true,
                Some(CalcToken::Minus) => false,
                _ => return Some(left),
            };
            self.pos += 1;
            let right = self.term()?;
            if left.d != right.d {
                return None;
            }
            left.v = if add { left.v + right.v } else { left.v - right.v };
        }
    }

    fn term(&mut self) -> Option<Val> {
        let mut left = self.unary()?;
        loop {
            let mul = match self.peek() {
                Some(CalcToken::Star) => true,
                Some(CalcToken::Slash) => false,
                _ => return Some(left),
            };
            self.pos += 1;
            let right = self.unary()?;
            left = if mul {
                Val { v: left.v * right.v, d: left.d.add(right.d) }
            } else {
                Val { v: left.v / right.v, d: left.d.sub(right.d) }
            };
        }
    }

    fn unary(&mut self) -> Option<Val> {
        match self.peek()? {
            CalcToken::Minus => {
                self.pos += 1;
                let inner = self.unary()?;
                Some(Val { v: -inner.v, d: inner.d })
            }
            CalcToken::Plus => {
                self.pos += 1;
                self.unary()
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Option<Val> {
        let tok = self.peek()?.clone();
        self.pos += 1;
        match tok {
            CalcToken::Num(n, unit) => self.leaf(f64::from(n), &unit),
            CalcToken::LParen => {
                let inner = self.expr()?;
                self.eat(&CalcToken::RParen).then_some(inner)
            }
            CalcToken::Ident(name) => {
                if self.eat(&CalcToken::LParen) {
                    self.call(&name)
                } else {
                    match name.as_str() {
                        "pi" => Some(Val::num(std::f64::consts::PI)),
                        "e" => Some(Val::num(std::f64::consts::E)),
                        "infinity" => Some(Val::num(f64::INFINITY)),
                        "nan" => Some(Val::num(f64::NAN)),
                        _ => {
                            self.soft = true;
                            None
                        }
                    }
                }
            }
            _ => None,
        }
    }

    fn leaf(&mut self, n: f64, unit: &str) -> Option<Val> {
        let (v, d) = match unit {
            "" => (n, NUM),
            "deg" => (n, ANG),
            "grad" => (n * 0.9, ANG),
            "rad" => (n.to_degrees(), ANG),
            "turn" => (n * 360.0, ANG),
            "s" => (n, TIME),
            "ms" => (n / 1000.0, TIME),
            "%" => (n, PCT),
            _ => {
                if !self.relative_units && !matches!(unit, "px" | "cm" | "mm" | "q" | "in" | "pt" | "pc") {
                    self.soft = true;
                    return None;
                }
                let CalcNode::Length(l) = calc_num_to_node(n as f32, unit)? else {
                    return None;
                };
                (f64::from(l.resolve(self.em, None, self.viewport)?), LEN)
            }
        };
        Some(Val { v, d })
    }

    /// Аргументы до закрывающей `)`; скобку съедает.
    fn args(&mut self) -> Option<Vec<Val>> {
        let mut out = vec![self.expr()?];
        while self.eat(&CalcToken::Comma) {
            out.push(self.expr()?);
        }
        self.eat(&CalcToken::RParen).then_some(out)
    }

    fn call(&mut self, name: &str) -> Option<Val> {
        if !MATH_FNS.contains(&name) {
            self.soft = true;
            return None;
        }
        if name == "round" {
            return self.round_call();
        }
        if name == "clamp" {
            return self.clamp_call();
        }
        let args = self.args()?;
        let same = args.windows(2).all(|w| w[0].d == w[1].d);
        let first = *args.first()?;
        match (name, args.len()) {
            ("calc", 1) => Some(first),
            ("min" | "max", _) if same => Some(Val {
                v: extremum(&args, name == "min"),
                d: first.d,
            }),
            ("abs", 1) => Some(Val { v: first.v.abs(), d: first.d }),
            ("sign", 1) => Some(Val::num(sign(first.v))),
            ("sin" | "cos" | "tan", 1) => {
                let rad = angle_arg(first)?;
                let r = match name {
                    "sin" => rad.sin(),
                    "cos" => rad.cos(),
                    _ => rad.tan(),
                };
                Some(Val::num(r))
            }
            ("asin" | "acos" | "atan", 1) if first.d == NUM => {
                let r = match name {
                    "asin" => first.v.asin(),
                    "acos" => first.v.acos(),
                    _ => first.v.atan(),
                };
                Some(Val { v: r.to_degrees(), d: ANG })
            }
            ("atan2", 2) if same => Some(Val {
                v: first.v.atan2(args[1].v).to_degrees(),
                d: ANG,
            }),
            ("pow", 2) if first.d == NUM && args[1].d == NUM => {
                Some(Val::num(first.v.powf(args[1].v)))
            }
            ("sqrt", 1) if first.d == NUM => Some(Val::num(first.v.sqrt())),
            ("exp", 1) if first.d == NUM => Some(Val::num(first.v.exp())),
            ("log", 1 | 2) if args.iter().all(|a| a.d == NUM) => {
                let r = match args.get(1) {
                    Some(base) => first.v.ln() / base.v.ln(),
                    None => first.v.ln(),
                };
                Some(Val::num(r))
            }
            ("hypot", _) if same => Some(Val {
                v: args.iter().map(|a| a.v * a.v).sum::<f64>().sqrt(),
                d: first.d,
            }),
            ("mod" | "rem", 2) if same => Some(Val {
                v: mod_rem(first.v, args[1].v, name == "mod"),
                d: first.d,
            }),
            ("progress", 3) if same => {
                Some(Val::num((first.v - args[1].v) / (args[2].v - args[1].v)))
            }
            _ => None,
        }
    }

    /// `clamp( [<calc-sum> | none], <calc-sum>, [<calc-sum> | none] )` — `none` снимает
    /// соответствующую границу (CSS Values L4 §10.6.3).
    fn clamp_call(&mut self) -> Option<Val> {
        let mut slots: Vec<Option<Val>> = Vec::with_capacity(3);
        loop {
            let is_none = matches!(self.peek(), Some(CalcToken::Ident(kw)) if kw == "none")
                && matches!(
                    self.toks.get(self.pos + 1),
                    Some(CalcToken::Comma | CalcToken::RParen)
                );
            if is_none {
                self.pos += 1;
                slots.push(None);
            } else {
                slots.push(Some(self.expr()?));
            }
            if !self.eat(&CalcToken::Comma) {
                break;
            }
        }
        if !self.eat(&CalcToken::RParen) || slots.len() != 3 {
            return None;
        }
        let d = slots.iter().flatten().next()?.d;
        if slots.iter().flatten().any(|v| v.d != d) {
            return None;
        }
        let lo = slots[0].map_or(f64::NEG_INFINITY, |v| v.v);
        let mid = slots[1]?.v;
        let hi = slots[2].map_or(f64::INFINITY, |v| v.v);
        let inner = extremum(&[Val { v: mid, d }, Val { v: hi, d }], true);
        let v = extremum(&[Val { v: lo, d }, Val { v: inner, d }], false);
        Some(Val { v, d })
    }

    /// `round( <rounding-strategy>?, A, B? )` — стратегия не значение, поэтому
    /// отдельный разбор до общего списка аргументов.
    fn round_call(&mut self) -> Option<Val> {
        let mut strategy = "nearest";
        if let Some(CalcToken::Ident(kw)) = self.peek()
            && matches!(kw.as_str(), "nearest" | "up" | "down" | "to-zero")
        {
            strategy = match kw.as_str() {
                "up" => "up",
                "down" => "down",
                "to-zero" => "to-zero",
                _ => "nearest",
            };
            self.pos += 1;
            if !self.eat(&CalcToken::Comma) {
                return None;
            }
        }
        let args = self.args()?;
        let a = *args.first()?;
        let step = match args.get(1) {
            Some(b) if b.d == a.d => b.v,
            // Шаг по умолчанию определён только для числа.
            None if a.d == NUM => 1.0,
            _ => return None,
        };
        if args.len() > 2 {
            return None;
        }
        Some(Val { v: round_to_step(strategy, a.v, step), d: a.d })
    }
}

fn angle_arg(v: Val) -> Option<f64> {
    match v.d {
        NUM => Some(v.v),
        ANG => Some(v.v.to_radians()),
        _ => None,
    }
}

/// `-0` и `NaN` сохраняются как есть: `sign(-0)` — `-0`, `sign(NaN)` — `NaN`.
fn sign(v: f64) -> f64 {
    if v.is_nan() || v == 0.0 { v } else { v.signum() }
}

fn extremum(args: &[Val], want_min: bool) -> f64 {
    let mut acc = f64::NAN;
    let mut started = false;
    for a in args {
        if a.v.is_nan() {
            return f64::NAN;
        }
        if !started {
            acc = a.v;
            started = true;
            continue;
        }
        let better = if a.v == acc {
            // `min(0, -0)` — `-0`, `max(-0, 0)` — `0`.
            a.v.is_sign_negative() == want_min
        } else if want_min {
            a.v < acc
        } else {
            a.v > acc
        };
        if better {
            acc = a.v;
        }
    }
    acc
}

fn mod_rem(a: f64, b: f64, is_mod: bool) -> f64 {
    if a.is_nan() || b.is_nan() || a.is_infinite() || b == 0.0 {
        return f64::NAN;
    }
    if b.is_infinite() {
        let same_sign = a.is_sign_negative() == b.is_sign_negative();
        return if is_mod && !same_sign { f64::NAN } else { a };
    }
    let r = a % b;
    if is_mod && r != 0.0 && (r < 0.0) != (b < 0.0) { r + b } else { r }
}

/// CSS Values L4 §10.5.1. Нулевой результат несёт знак `a`.
fn round_to_step(strategy: &str, a: f64, step: f64) -> f64 {
    if a.is_nan() || step.is_nan() || step == 0.0 || (a.is_infinite() && step.is_infinite()) {
        return f64::NAN;
    }
    if a.is_infinite() {
        return a;
    }
    let zero_of_a = if a.is_sign_negative() { -0.0 } else { 0.0 };
    if step.is_infinite() {
        let positive = a > 0.0;
        return match strategy {
            "up" if positive => f64::INFINITY,
            "down" if a < 0.0 => f64::NEG_INFINITY,
            // WPT `round-mod-rem-computed`: `round(down, 1, infinity)` is `-0`.
            "down" if positive => -0.0,
            _ => zero_of_a,
        };
    }
    let step = step.abs();
    let q = a / step;
    let n = match strategy {
        "up" => q.ceil(),
        "down" => q.floor(),
        "to-zero" => q.trunc(),
        _ => (q + 0.5).floor(),
    };
    let r = n * step;
    if r == 0.0 { zero_of_a } else { r }
}

/// Литерал числа: до 7 значащих цифр — точность `f32`, в которой лексер читает числа
/// (`.1` приходит как `0.100000001`), так что шум `f32`/`f64` отбрасывается. `NaN` в вычисленном значении — `0`, бесконечность —
/// наибольшее конечное `f32` (CSS Values L4 §10.9.1).
fn number_literal(v: f64) -> String {
    let v = if v.is_nan() {
        0.0
    } else if v.is_infinite() {
        f64::from(f32::MAX).copysign(v)
    } else {
        v
    };
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{v}");
    }
    let rounded: f64 = format!("{v:.6e}").parse().unwrap_or(v);
    format!("{rounded}")
}

/// Подставляет в значение `val` литералы вместо внешних вызовов math-функций с
/// числовым, угловым или временным результатом. `None` — подставлять нечего.
pub(in crate::style) fn substitute_numeric_math(
    prop: &str,
    val: &str,
    em_basis: f32,
    viewport: Size,
) -> Option<String> {
    if !val.contains('(') {
        return None;
    }
    let bytes = val.as_bytes();
    let mut out = String::new();
    let mut copied = 0usize; // всё до этого байта уже перенесено в `out` или не менялось
    let mut changed = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' || b == b'\'' {
            i = skip_string(bytes, i);
            continue;
        }
        let ident_start = b.is_ascii_alphabetic()
            && (i == 0 || !is_ident_byte(bytes[i - 1]));
        if !ident_start {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < bytes.len() && is_ident_byte(bytes[j]) {
            j += 1;
        }
        let name = val[i..j].to_ascii_lowercase();
        if bytes.get(j) != Some(&b'(') {
            i = j;
            continue;
        }
        let Some(close) = matching_paren(bytes, j) else {
            return changed.then(|| finish(out, val, copied));
        };
        if name == "url" {
            i = close + 1;
            continue;
        }
        if !MATH_FNS.contains(&name.as_str()) {
            // Обычная функция (`rotate(`, `rgb(`…): её аргументы просматриваем дальше.
            i = j + 1;
            continue;
        }
        let expr = &val[i..=close];
        if let Some(lit) = evaluate(prop, expr, em_basis, viewport) {
            out.push_str(&val[copied..i]);
            out.push_str(&lit);
            copied = close + 1;
            changed = true;
        }
        i = close + 1;
    }
    changed.then(|| finish(out, val, copied))
}

fn finish(mut out: String, val: &str, copied: usize) -> String {
    out.push_str(&val[copied..]);
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80
}

/// Индекс байта после закрывающей кавычки (или конец строки).
fn skip_string(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            c if c == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Индекс `)`, парной `(` в `open`.
fn matching_paren(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                i = skip_string(bytes, i);
                continue;
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Литерал для одного вызова math-функции или `None`, если его оставляют прежнему разбору.
fn evaluate(prop: &str, expr: &str, em_basis: f32, viewport: Size) -> Option<String> {
    let toks = tokenize_calc(expr)?;
    let mut ev = Eval { toks: &toks, pos: 0, em: em_basis, viewport, relative_units: true, soft: false };
    let r = ev.expr()?;
    if ev.pos != toks.len() {
        return None;
    }
    match r.d {
        // CSS Fonts L4 §2.2: результат `calc()` для `font-weight` зажимается в `[1, 1000]`
        // (литерал вне диапазона недопустим, вычисленное значение — нет).
        NUM if prop == "font-weight" && !r.v.is_nan() => Some(number_literal(r.v.clamp(1.0, 1000.0))),
        NUM => {
            let v = if integer_property(prop) {
                if r.v.is_nan() { 0.0 } else { (r.v + 0.5).floor() }
            } else {
                r.v
            };
            Some(number_literal(v))
        }
        // CSS Fonts L4 §2.3: угол `oblique` из `calc()` зажимается в `[-90deg, 90deg]`.
        ANG if prop == "font-style" && !r.v.is_nan() => {
            Some(format!("{}deg", number_literal(r.v.clamp(-90.0, 90.0))))
        }
        ANG => Some(format!("{}deg", number_literal(r.v))),
        TIME => Some(format!("{}s", number_literal(r.v))),
        // Процент зависит от свойства и базы: сводим только там, где процент — число.
        PCT => expected_dims(prop)
            .is_some_and(|d| d.contains(&PCT))
            .then(|| format!("{}%", number_literal(r.v))),
        LEN => parse_math_function_value(expr)
            .is_none()
            .then(|| format!("{}px", number_literal(r.v.clamp(-MAX_LENGTH_PX, MAX_LENGTH_PX)))),
        _ => None,
    }
}

/// Specified-value form of a length expression the old `calc()` parser rejects
/// (`calc(100px * pi)` → `calc(314.159px)`, `calc(1px * NaN)` → `calc(NaN * 1px)`),
/// CSS Values L4 §10.9. Font- and viewport-relative leaves are not folded.
pub(in crate::style) fn canonical_math_length(expr: &str) -> Option<String> {
    let toks = tokenize_calc(expr)?;
    let mut ev = Eval {
        toks: &toks,
        pos: 0,
        em: 0.0,
        viewport: Size { width: 0.0, height: 0.0 },
        relative_units: false,
        soft: false,
    };
    let r = ev.expr()?;
    if ev.pos != toks.len() || r.d != LEN {
        return None;
    }
    Some(if r.v.is_nan() {
        "calc(NaN * 1px)".into()
    } else if r.v.is_infinite() {
        format!("calc({}infinity * 1px)", if r.v < 0.0 { "-" } else { "" })
    } else {
        format!("calc({}px)", number_literal(r.v))
    })
}

/// Что разрешено в значении свойства, чьё одиночное math-выражение сериализуется в CSSOM.
fn expected_dims(prop: &str) -> Option<&'static [Dim]> {
    match prop {
        "opacity" | "scale" | "fill-opacity" | "stroke-opacity" | "stop-opacity" | "flood-opacity" => {
            Some(&[NUM, PCT])
        }
        "z-index" | "order" | "orphans" | "widows" | "column-count" | "flex-grow" | "flex-shrink"
        | "flex-line-count" | "shape-image-threshold" => Some(&[NUM]),
        "transition-delay" | "transition-duration" | "animation-delay" | "animation-duration" => {
            Some(&[TIME])
        }
        "rotate" | "font-style-angle" => Some(&[ANG]),
        "font-weight" => Some(&[NUM]),
        _ => None,
    }
}

/// Вход для JS-сеттера `element.style` (`_lumen_css_canonical_math`): `""` — значение не
/// затронуто (идти обычным путём), `"!"` — отбросить присваивание, иначе — запись для хранения.
pub fn canonical_specified_numeric_math(prop: &str, value: &str) -> String {
    canonical_numeric_math(prop, value).unwrap_or_default()
}

/// Литерал специфицированного значения: `NaN` и бесконечность остаются ключевыми словами.
fn specified_literal(v: f64, unit: &str) -> String {
    let (mag, suffix) = if v.is_nan() {
        ("NaN".to_string(), if unit.is_empty() { String::new() } else { format!(" * 1{unit}") })
    } else if v.is_infinite() {
        let sign = if v < 0.0 { "-" } else { "" };
        (format!("{sign}infinity"), if unit.is_empty() { String::new() } else { format!(" * 1{unit}") })
    } else {
        (format!("{}{unit}", number_literal(v)), String::new())
    };
    format!("calc({mag}{suffix})")
}

/// CSSOM specified value of a property whose whole value is one math function
/// (`opacity: abs(-.5)` → `calc(0.5)`, `transition-delay: calc(1s + 1s)` → `calc(2s)`).
/// `Some("")` — не наше значение (оставить как есть); `Some("!")` — невалидно по типу
/// или синтаксису; иначе каноническая запись. `None` — свойство вне таблицы.
fn canonical_numeric_math(prop: &str, value: &str) -> Option<String> {
    let expected = expected_dims(prop)?;
    let v = value.trim();
    let bytes = v.as_bytes();
    let open = v.find('(')?;
    let name = v[..open].to_ascii_lowercase();
    // Одиночный вызов на всё значение: иначе это список или составное значение.
    if !MATH_FNS.contains(&name.as_str()) || matching_paren(bytes, open) != Some(bytes.len() - 1) {
        return Some(String::new());
    }
    let Some(toks) = tokenize_calc(v) else {
        return Some(String::new());
    };
    let mut ev = Eval {
        toks: &toks,
        pos: 0,
        em: 0.0,
        viewport: Size { width: 0.0, height: 0.0 },
        relative_units: false,
        soft: false,
    };
    let r = ev.expr().filter(|_| ev.pos == toks.len());
    let Some(r) = r else {
        return Some(if ev.soft { String::new() } else { "!".into() });
    };
    if !expected.contains(&r.d) {
        return Some("!".into());
    }
    Some(match r.d {
        NUM => specified_literal(r.v, ""),
        TIME => specified_literal(r.v, "s"),
        ANG => specified_literal(r.v, "deg"),
        // Процент сохраняет прежнюю запись.
        _ => String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VP: Size = Size { width: 800.0, height: 600.0 };

    fn sub(prop: &str, v: &str) -> String {
        substitute_numeric_math(prop, v, 20.0, VP).unwrap_or_else(|| v.to_string())
    }

    #[test]
    fn numbers_times_angles() {
        assert_eq!(sub("z-index", "calc(1 + 2)"), "3");
        assert_eq!(sub("opacity", "calc(0.5 + 0.25)"), "0.75");
        assert_eq!(sub("transition-delay", "calc(1s + 1s)"), "2s");
        assert_eq!(sub("transition-delay", "calc(500ms + 1s)"), "1.5s");
        assert_eq!(sub("rotate", "calc(1deg + 1deg)"), "2deg");
        assert_eq!(sub("rotate", "calc(0.5turn)"), "180deg");
        assert_eq!(sub("transform", "rotate(calc(1deg + 1deg)) scale(2)"), "rotate(2deg) scale(2)");
    }

    #[test]
    fn integer_rounding_half_up() {
        assert_eq!(sub("z-index", "calc(2.5 / 2)"), "1");
        assert_eq!(sub("z-index", "calc(3 / 2)"), "2");
        assert_eq!(sub("z-index", "calc(-3 / 2)"), "-1");
        assert_eq!(sub("z-index", "calc(-3.5 / 2)"), "-2");
    }

    #[test]
    fn constants_and_special_values() {
        assert_eq!(sub("scale", "calc(pi)"), "3.141593");
        assert_eq!(sub("scale", "calc(NaN)"), "0");
        assert_eq!(sub("scale", "calc(infinity)"), sub("scale", "calc(1 / 0)"));
        assert_eq!(sub("scale", "calc(-infinity)"), sub("scale", "calc(-1 / 0)"));
        assert_eq!(sub("scale", "round(nearest, -0, infinity)"), "-0");
        assert_eq!(sub("scale", "round(up, 1, infinity)"), sub("scale", "calc(infinity)"));
        assert_eq!(sub("scale", "round(down, 1, infinity)"), "-0");
        assert_eq!(sub("scale", "mod(-4, infinity)"), "0");
        assert_eq!(sub("scale", "sign(-0)"), "-0");
    }

    #[test]
    fn functions_with_dimensions() {
        assert_eq!(sub("scale", "sign(-1px)"), "-1");
        assert_eq!(sub("scale", "sign(1em)"), "1");
        assert_eq!(sub("scale", "progress(5px, 0px, 10px)"), "0.5");
        assert_eq!(sub("scale", "calc(10px / 5px)"), "2");
        assert_eq!(sub("scale", "abs(-3)"), "3");
        assert_eq!(sub("scale", "mod(-18, 5)"), "2");
        assert_eq!(sub("scale", "rem(-18, 5)"), "-3");
        assert_eq!(sub("scale", "round(to-zero, -2.7)"), "-2");
        assert_eq!(sub("scale", "hypot(3, 4)"), "5");
        assert_eq!(sub("rotate", "atan2(1, 1)"), "45deg");
        assert_eq!(sub("scale", "sin(90deg)"), "1");
    }

    #[test]
    fn lengths_only_when_legacy_parser_rejects() {
        assert_eq!(sub("margin-left", "calc(100px*pi)"), "314.1593px");
        assert_eq!(sub("margin-left", "calc(100px*progress(5px,0px,10px))"), "50px");
        assert_eq!(sub("margin-left", "calc(10px + 2em)"), "calc(10px + 2em)");
        assert_eq!(sub("margin-left", "calc(50% - 10px)"), "calc(50% - 10px)");
        assert_eq!(canonical_math_length("calc(100px*pi)").as_deref(), Some("calc(314.1593px)"));
        assert_eq!(canonical_math_length("calc(1px*NaN)").as_deref(), Some("calc(NaN * 1px)"));
        assert_eq!(canonical_math_length("calc(-1px*infinity)").as_deref(), Some("calc(-infinity * 1px)"));
        assert_eq!(canonical_math_length("calc(1em*pi)"), None);
    }

    #[test]
    fn specified_serialization_and_validity() {
        let c = |p: &str, v: &str| canonical_numeric_math(p, v).unwrap_or_default();
        assert_eq!(c("opacity", "min(.1, .2)"), "calc(0.1)");
        assert_eq!(c("opacity", "calc(NaN)"), "calc(NaN)");
        assert_eq!(c("opacity", "1 * infinity"), "");
        assert_eq!(c("opacity", "calc(1 * -infinity)"), "calc(-infinity)");
        assert_eq!(c("transition-delay", "calc(1s + 1s)"), "calc(2s)");
        assert_eq!(c("rotate", "calc(1deg + 1deg)"), "calc(2deg)");
        assert_eq!(c("z-index", "abs(-3)"), "calc(3)");
        assert_eq!(c("z-index", "abs()"), "!");
        assert_eq!(c("z-index", "calc(1px)"), "!");
        assert_eq!(c("opacity", "calc(1 + )"), "!");
        assert_eq!(c("opacity", "calc(1em / 1px)"), "");
        assert_eq!(c("opacity", "calc(1 + attr(x))"), "");
        assert_eq!(c("scale", "calc(1) calc(2)"), "");
        assert_eq!(canonical_numeric_math("margin-left", "calc(1)"), None);
    }

    #[test]
    fn exponents_clamp_none_and_percent() {
        assert_eq!(sub("margin-left", "hypot(3e+9px, 4e+9px)"), "hypot(3e+9px, 4e+9px)");
        assert_eq!(sub("scale", "calc(2e0 + 1)"), "3");
        assert_eq!(sub("z-index", "clamp(none, 33, 30)"), "30");
        assert_eq!(sub("z-index", "clamp(33, 30, none)"), "33");
        assert_eq!(sub("z-index", "clamp(none, 30, none)"), "30");
        assert_eq!(sub("z-index", "clamp(none, none, 30)"), "clamp(none, none, 30)");
        assert_eq!(sub("opacity", "calc(50% + 40%)"), "90%");
        assert_eq!(sub("width", "calc(50% + 40%)"), "calc(50% + 40%)");
    }

    #[test]
    fn untouched_inputs() {
        assert_eq!(sub("content", "\"calc(1 + 2)\""), "\"calc(1 + 2)\"");
        assert_eq!(sub("background", "url(calc(1+2).png)"), "url(calc(1+2).png)");
        assert_eq!(sub("width", "calc(1px + 1s)"), "calc(1px + 1s)");
        assert_eq!(sub("z-index", "abs()"), "abs()");
        assert_eq!(sub("color", "rgb(1 2 3)"), "rgb(1 2 3)");
        assert_eq!(sub("transition-delay", "calc(1s + 1s), 2s"), "2s, 2s");
    }
}
