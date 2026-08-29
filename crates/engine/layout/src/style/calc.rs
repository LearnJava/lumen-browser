//! CSS Values L4 §10 — `calc()` и математические функции: AST (`CalcNode`),
//! лексер выражения, рекурсивный спуск по грамматике и резолв `MathFn`.
//!
//! Перенесено батчем SPLIT-ST9 из `crates/engine/layout/src/style.rs`
//! (анкер `enum CalcNode`) без правок тел: изменена только видимость тех items,
//! которые продолжает звать `style::values::length`.

// Долг по документации переезжает вместе с кодом (§2 очереди SPLIT, правило 3):
// варианты `pub enum CalcNode`/`MathFn`/`RoundStrategy` написаны до включения
// `missing_docs`. Область — файл, счётчики — docs/lint-policy.md §10.
#![allow(missing_docs)]

use lumen_core::geom::Size;

use crate::style::Length;

/// CSS Values L4 §10 — AST `calc()`-выражения. Хранится как двоичное дерево
/// (`Add`/`Sub`/`Mul`/`Div`) с листовыми `Length` и unitless `Number`.
/// `Number` нужен для умножения / деления, где спецификация требует, чтобы
/// один операнд был unitless. В Phase 0 мы не валидируем строго типы
/// операндов (`px * px` математически считается, но семантически бессмысленно
/// — реальный CSS такого не пишет, а наш resolve всё равно даёт `f32`).
#[derive(Debug, Clone, PartialEq)]
pub enum CalcNode {
    /// Листовое length-значение (`10px`, `2em`, `50%`, …).
    Length(Length),
    /// Unitless число (например `2` в `calc(2 * 10px)`). Для углов
    /// (`45deg`, `1turn`) лексер тоже даёт Number — конвертирует в радианы
    /// сразу при чтении.
    Number(f32),
    Add(Box<CalcNode>, Box<CalcNode>),
    Sub(Box<CalcNode>, Box<CalcNode>),
    Mul(Box<CalcNode>, Box<CalcNode>),
    Div(Box<CalcNode>, Box<CalcNode>),
    /// CSS Values L4 §10.6.1 — `min(a, b, ...)`. Минимум по списку.
    Min(Vec<CalcNode>),
    /// CSS Values L4 §10.6.2 — `max(a, b, ...)`. Максимум по списку.
    Max(Vec<CalcNode>),
    /// CSS Values L4 §10.6.3 — `clamp(min, val, max)`. Эквивалентно
    /// `max(min, min(val, max))`. Если `min > max` — побеждает `min`.
    Clamp(Box<CalcNode>, Box<CalcNode>, Box<CalcNode>),
    /// CSS Values L4 §10.7-10.9 — научные math-функции: тригонометрия
    /// (`sin/cos/tan/asin/acos/atan/atan2`), экспоненциальные
    /// (`pow/sqrt/exp/log/hypot`), signs/stepping (`abs/sign/mod/rem/round`).
    /// Все 15 функций унифицированы под `Func(MathFn, args)`: арность
    /// и формула — внутри `resolve` по match-у на MathFn.
    Func(MathFn, Vec<CalcNode>),
}

/// CSS Values L4 §10.7-10.9 — научные math-функции. Имена case-insensitive
/// (нормализованы в нижний регистр в лексере).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathFn {
    // §10.7 trig
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    // §10.8 exponential
    Pow,
    Sqrt,
    Exp,
    Log,
    Hypot,
    // §10.9 sign/stepping
    Abs,
    Sign,
    Mod,
    Rem,
    /// CSS Values L4 §10.5.1 — `round( <rounding-strategy>?, A, B? )`.
    /// Strategy keyword вычисляется парсером и зашит в variant; отсутствие
    /// keyword-а ≡ `Nearest`.
    Round(RoundStrategy),
}

/// CSS Values L4 §10.5.1 — стратегия округления для `round()`.
/// Опускание keyword-а в `round(A[, B])` ≡ `Nearest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundStrategy {
    /// Ближайшее кратное step; при равноудалённости — в сторону +∞
    /// (`f32::round` round-half-away-from-zero, но spec в §10.5.1 говорит
    /// «toward +∞»; различие незаметно для положительного step и нечастых
    /// граничных случаев).
    Nearest,
    /// Меньшее или равное кратное step, всегда в сторону +∞
    /// (`ceil(A/B) * B`).
    Up,
    /// Большее или равное кратное step, всегда в сторону −∞
    /// (`floor(A/B) * B`).
    Down,
    /// Округление к нулю (`trunc(A/B) * B`). Для положительных A совпадает
    /// с `Down`, для отрицательных — с `Up`.
    ToZero,
}

impl CalcNode {
    /// Резолвит выражение в `f32`-пиксели по тем же правилам, что
    /// `Length::resolve`. Возвращает `None` если:
    ///   - хотя бы один листовой `Length::Percent` не имеет `percent_basis`
    ///     (контекст не задан);
    ///   - деление на 0;
    ///   - пустой список аргументов в `min()` / `max()`.
    pub fn resolve(
        &self,
        em_basis: f32,
        percent_basis: Option<f32>,
        viewport: Size,
    ) -> Option<f32> {
        match self {
            CalcNode::Length(l) => l.resolve(em_basis, percent_basis, viewport),
            CalcNode::Number(n) => Some(*n),
            CalcNode::Add(a, b) => Some(
                a.resolve(em_basis, percent_basis, viewport)?
                    + b.resolve(em_basis, percent_basis, viewport)?,
            ),
            CalcNode::Sub(a, b) => Some(
                a.resolve(em_basis, percent_basis, viewport)?
                    - b.resolve(em_basis, percent_basis, viewport)?,
            ),
            CalcNode::Mul(a, b) => Some(
                a.resolve(em_basis, percent_basis, viewport)?
                    * b.resolve(em_basis, percent_basis, viewport)?,
            ),
            CalcNode::Div(a, b) => {
                let denom = b.resolve(em_basis, percent_basis, viewport)?;
                if denom == 0.0 {
                    return None;
                }
                Some(a.resolve(em_basis, percent_basis, viewport)? / denom)
            }
            CalcNode::Min(args) => {
                if args.is_empty() {
                    return None;
                }
                let mut acc = args[0].resolve(em_basis, percent_basis, viewport)?;
                for n in &args[1..] {
                    let v = n.resolve(em_basis, percent_basis, viewport)?;
                    if v < acc {
                        acc = v;
                    }
                }
                Some(acc)
            }
            CalcNode::Max(args) => {
                if args.is_empty() {
                    return None;
                }
                let mut acc = args[0].resolve(em_basis, percent_basis, viewport)?;
                for n in &args[1..] {
                    let v = n.resolve(em_basis, percent_basis, viewport)?;
                    if v > acc {
                        acc = v;
                    }
                }
                Some(acc)
            }
            CalcNode::Clamp(min, val, max) => {
                let mn = min.resolve(em_basis, percent_basis, viewport)?;
                let v = val.resolve(em_basis, percent_basis, viewport)?;
                let mx = max.resolve(em_basis, percent_basis, viewport)?;
                // CSS Values L4 §10.6.3: clamp(min, val, max) ≡
                // max(min, min(val, max)). При min > max побеждает min.
                let inner = if v < mx { v } else { mx };
                Some(if mn > inner { mn } else { inner })
            }
            CalcNode::Func(func, args) => {
                resolve_math_func(*func, args, em_basis, percent_basis, viewport)
            }
        }
    }
}

/// Резолвит научную math-функцию. Валидация арности уже сделана парсером —
/// здесь предполагаем правильное число аргументов. Все вычисления делаются
/// в `f64` для точности (особенно для trig / log), результат сужается до
/// `f32`. Возвращает None если резолв одного из аргументов даёт None
/// (например, `%` без containing block) или результат не конечный
/// (`sqrt(-1)`, `log(0)`, `1.0 / 0.0` и т.п.).
fn resolve_math_func(
    func: MathFn,
    args: &[CalcNode],
    em_basis: f32,
    percent_basis: Option<f32>,
    viewport: Size,
) -> Option<f32> {
    let resolve = |n: &CalcNode| -> Option<f64> {
        n.resolve(em_basis, percent_basis, viewport).map(f64::from)
    };
    let result: f64 = match func {
        MathFn::Sin => resolve(&args[0])?.sin(),
        MathFn::Cos => resolve(&args[0])?.cos(),
        MathFn::Tan => resolve(&args[0])?.tan(),
        MathFn::Asin => resolve(&args[0])?.asin(),
        MathFn::Acos => resolve(&args[0])?.acos(),
        MathFn::Atan => resolve(&args[0])?.atan(),
        MathFn::Atan2 => {
            let y = resolve(&args[0])?;
            let x = resolve(&args[1])?;
            y.atan2(x)
        }
        MathFn::Pow => {
            let base = resolve(&args[0])?;
            let exp = resolve(&args[1])?;
            base.powf(exp)
        }
        MathFn::Sqrt => resolve(&args[0])?.sqrt(),
        MathFn::Exp => resolve(&args[0])?.exp(),
        MathFn::Log => {
            let v = resolve(&args[0])?;
            if args.len() == 2 {
                // log(value, base) — логарифм по основанию.
                let base = resolve(&args[1])?;
                v.log(base)
            } else {
                // Единственный аргумент: натуральный логарифм (CSS §10.8.5).
                v.ln()
            }
        }
        MathFn::Hypot => {
            // hypot(a, b, ...) = sqrt(a² + b² + ...). spec.
            let mut sum_sq = 0.0_f64;
            for a in args {
                let v = resolve(a)?;
                sum_sq += v * v;
            }
            sum_sq.sqrt()
        }
        MathFn::Abs => resolve(&args[0])?.abs(),
        MathFn::Sign => {
            // CSS sign(0) = 0 (spec §10.9.2); std signum даёт +1 для 0.0
            // и -1 для -0.0. Обрабатываем явно.
            let v = resolve(&args[0])?;
            if v == 0.0 {
                0.0
            } else if v > 0.0 {
                1.0
            } else {
                -1.0
            }
        }
        MathFn::Mod => {
            // CSS mod (§10.9.3): результат имеет знак делителя.
            // `((a % b) + b) % b` — стандартная формула positive-mod.
            let a = resolve(&args[0])?;
            let b = resolve(&args[1])?;
            if b == 0.0 {
                return None;
            }
            ((a % b) + b) % b
        }
        MathFn::Rem => {
            // CSS rem (§10.9.4): truncated remainder, sign от делимого
            // (тот же `%` в Rust для f64).
            let a = resolve(&args[0])?;
            let b = resolve(&args[1])?;
            if b == 0.0 {
                return None;
            }
            a % b
        }
        MathFn::Round(strategy) => {
            // round([<strategy>,] val[, step]). Без step (нет 2-го arg) —
            // step = 1, как в spec §10.5.1. step ≠ 0 (иначе ÷ 0 → None).
            // Знак step сохраняется: spec не делает abs, и для nearest
            // результат симметричен, а для up/down/to-zero — нет (это та же
            // semantics, что у chrome/firefox). NaN ловится финальным
            // `is_finite()`-чеком.
            let val = resolve(&args[0])?;
            let step = if args.len() == 2 {
                let s = resolve(&args[1])?;
                if s == 0.0 {
                    return None;
                }
                s
            } else {
                1.0
            };
            let ratio = val / step;
            let rounded = match strategy {
                RoundStrategy::Nearest => ratio.round(),
                RoundStrategy::Up => ratio.ceil(),
                RoundStrategy::Down => ratio.floor(),
                RoundStrategy::ToZero => ratio.trunc(),
            };
            rounded * step
        }
    };
    if result.is_finite() {
        Some(result as f32)
    } else {
        None
    }
}

/// Похоже ли значение на функциональный вызов CSS math-функции?
/// Минимальный критерий: начинается с ASCII-буквы и содержит `(`.
/// Точное соответствие именам функций (`calc`/`min`/`max`/`clamp`)
/// проверяется в parse_calc_factor.
pub(in crate::style) fn looks_like_function_call(s: &str) -> bool {
    matches!(s.as_bytes().first(), Some(b) if b.is_ascii_alphabetic())
        && s.contains('(')
}

/// Парсит top-level math-функцию (`calc(...)` / `min(...)` / `max(...)` /
/// `clamp(...)`) как обычный length-литерал, оборачивая результат в
/// `Length::Calc`. Возвращает None, если разбор не удался — `parse_length`
/// тогда падает в обычную strip_suffix-ветку.
pub(in crate::style) fn parse_math_function_value(s: &str) -> Option<Length> {
    let tokens = tokenize_calc(s)?;
    let mut pos = 0usize;
    let node = parse_calc_expr(&tokens, &mut pos)?;
    if pos != tokens.len() {
        return None;
    }
    Some(Length::Calc(Box::new(node)))
}

// ──────────────── calc() лексер + парсер ────────────────

#[derive(Debug, Clone, PartialEq)]
enum CalcToken {
    /// Числовой токен с (опциональным) unit-суффиксом.
    Num(f32, String),
    /// Идентификатор функции (`calc`, `min`, `max`, `clamp`). Хранится в
    /// нижнем регистре — CSS function names ASCII case-insensitive.
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    /// Разделитель аргументов функции.
    Comma,
}

/// Лексер `calc()` тела. Возвращает None при синтаксической ошибке (например,
/// неизвестный символ или сломанное число).
///
/// `-` всегда токенизируется как `Minus` (не как часть числа). Унарный
/// минус (`calc(-10px + 5px)`) разрешается парсером через
/// `factor := ('-' | '+') factor | …`. Это даёт корректное поведение и для
/// `10px - 5px` (whitespace по спецификации), и для `10px-5px` (lenient).
fn tokenize_calc(s: &str) -> Option<Vec<CalcToken>> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let tok = match b {
            b'+' => CalcToken::Plus,
            b'-' => CalcToken::Minus,
            b'*' => CalcToken::Star,
            b'/' => CalcToken::Slash,
            b'(' => CalcToken::LParen,
            b')' => CalcToken::RParen,
            b',' => CalcToken::Comma,
            // Число без ведущего знака (знак — отдельный токен).
            b'0'..=b'9' | b'.' => {
                let (num, unit, end) = lex_number(bytes, i)?;
                tokens.push(CalcToken::Num(num, unit));
                i = end;
                continue;
            }
            // Идентификатор функции — буквенный старт + опц. цифры/дефис
            // (так в имени `atan2` лексер не споткнётся на `2`).
            c if c.is_ascii_alphabetic() => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-')
                {
                    i += 1;
                }
                let name = std::str::from_utf8(&bytes[start..i])
                    .ok()?
                    .to_ascii_lowercase();
                tokens.push(CalcToken::Ident(name));
                continue;
            }
            _ => return None,
        };
        tokens.push(tok);
        i += 1;
    }
    Some(tokens)
}

/// Парсит число (без знака) + опциональный unit-суффикс начиная с `bytes[start]`.
/// Возвращает (значение, unit, индекс после конца токена). Знак лежит
/// отдельным `Minus`/`Plus`-токеном.
fn lex_number(bytes: &[u8], start: usize) -> Option<(f32, String, usize)> {
    let mut i = start;
    let num_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    let num_end = i;
    if num_end == num_start {
        return None;
    }
    let num_str = std::str::from_utf8(&bytes[num_start..num_end]).ok()?;
    let num = num_str.parse::<f32>().ok()?;
    // Unit-суффикс: буквы (для px/em/rem/vh/vw/vmin/vmax) или `%`.
    let unit_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == unit_start && matches!(bytes.get(i), Some(b'%')) {
        i += 1;
    }
    let unit =
        std::str::from_utf8(&bytes[unit_start..i]).ok()?.to_ascii_lowercase();
    Some((num, unit, i))
}

/// `expr := term (('+' | '-') term)*`
fn parse_calc_expr(tokens: &[CalcToken], pos: &mut usize) -> Option<CalcNode> {
    let mut left = parse_calc_term(tokens, pos)?;
    loop {
        match tokens.get(*pos) {
            Some(CalcToken::Plus) => {
                *pos += 1;
                let right = parse_calc_term(tokens, pos)?;
                left = CalcNode::Add(Box::new(left), Box::new(right));
            }
            Some(CalcToken::Minus) => {
                *pos += 1;
                let right = parse_calc_term(tokens, pos)?;
                left = CalcNode::Sub(Box::new(left), Box::new(right));
            }
            _ => return Some(left),
        }
    }
}

/// `term := factor (('*' | '/') factor)*`
fn parse_calc_term(tokens: &[CalcToken], pos: &mut usize) -> Option<CalcNode> {
    let mut left = parse_calc_factor(tokens, pos)?;
    loop {
        match tokens.get(*pos) {
            Some(CalcToken::Star) => {
                *pos += 1;
                let right = parse_calc_factor(tokens, pos)?;
                left = CalcNode::Mul(Box::new(left), Box::new(right));
            }
            Some(CalcToken::Slash) => {
                *pos += 1;
                let right = parse_calc_factor(tokens, pos)?;
                left = CalcNode::Div(Box::new(left), Box::new(right));
            }
            _ => return Some(left),
        }
    }
}

/// `factor := ('-' | '+') factor | function | Num(value, unit) | '(' expr ')'`
///
/// `function := Ident '(' arg-list ')'` где `Ident` — одно из `calc` /
/// `min` / `max` / `clamp` (CSS Values L4 §10 и §10.6). Унарный `-`
/// реализуется как `0 - factor`. Унарный `+` — no-op.
fn parse_calc_factor(tokens: &[CalcToken], pos: &mut usize) -> Option<CalcNode> {
    match tokens.get(*pos)? {
        CalcToken::Minus => {
            *pos += 1;
            let inner = parse_calc_factor(tokens, pos)?;
            Some(CalcNode::Sub(
                Box::new(CalcNode::Number(0.0)),
                Box::new(inner),
            ))
        }
        CalcToken::Plus => {
            *pos += 1;
            parse_calc_factor(tokens, pos)
        }
        CalcToken::LParen => {
            *pos += 1;
            let inner = parse_calc_expr(tokens, pos)?;
            if !matches!(tokens.get(*pos), Some(CalcToken::RParen)) {
                return None;
            }
            *pos += 1;
            Some(inner)
        }
        CalcToken::Ident(name) => {
            let name = name.clone();
            *pos += 1;
            if !matches!(tokens.get(*pos), Some(CalcToken::LParen)) {
                return None;
            }
            *pos += 1;
            parse_function_call(&name, tokens, pos)
        }
        CalcToken::Num(v, unit) => {
            let v = *v;
            let unit = unit.clone();
            *pos += 1;
            calc_num_to_node(v, &unit)
        }
        _ => None,
    }
}

/// Парсит тело math-функции после `<name>(` (открывающая скобка уже
/// съедена), ожидает `)` в конце. Поддерживает `calc` (один expr),
/// `min` / `max` (1+ expr через `,`), `clamp` (ровно 3 expr через `,`).
/// Неизвестное имя → None.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn parse_function_call(
    name: &str,
    tokens: &[CalcToken],
    pos: &mut usize,
) -> Option<CalcNode> {
    // CSS Values L4 §10.5.1: `round( <rounding-strategy>?, A, B? )` —
    // первый аргумент-keyword. Распознаём ДО общего parse_arg_list, чтобы
    // ident-без-`(` не падал в `parse_calc_factor` как «функция без скобок».
    // После keyword обязательна `,` — strategy без последующего expr невалиден.
    let round_strategy = if name == "round" {
        if let Some(CalcToken::Ident(kw)) = tokens.get(*pos)
            && let Some(s) = parse_round_strategy(kw)
        {
            *pos += 1;
            if !matches!(tokens.get(*pos), Some(CalcToken::Comma)) {
                return None;
            }
            *pos += 1;
            Some(s)
        } else {
            None
        }
    } else {
        None
    };

    let args = parse_arg_list(tokens, pos)?;
    if !matches!(tokens.get(*pos), Some(CalcToken::RParen)) {
        return None;
    }
    *pos += 1;
    match name {
        "calc" => {
            if args.len() != 1 {
                return None;
            }
            Some(args.into_iter().next().unwrap())
        }
        "min" => {
            if args.is_empty() {
                return None;
            }
            Some(CalcNode::Min(args))
        }
        "max" => {
            if args.is_empty() {
                return None;
            }
            Some(CalcNode::Max(args))
        }
        "clamp" => {
            if args.len() != 3 {
                return None;
            }
            let mut it = args.into_iter();
            let a = it.next().unwrap();
            let b = it.next().unwrap();
            let c = it.next().unwrap();
            Some(CalcNode::Clamp(Box::new(a), Box::new(b), Box::new(c)))
        }
        // CSS Values L4 §10.7-10.9 — научные math-функции.
        // Имя → (MathFn, валидное число аргументов). Проверяем арность тут,
        // resolve_math_func предполагает корректность.
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sqrt" | "exp"
        | "abs" | "sign" => {
            if args.len() != 1 {
                return None;
            }
            let func = match name {
                "sin" => MathFn::Sin,
                "cos" => MathFn::Cos,
                "tan" => MathFn::Tan,
                "asin" => MathFn::Asin,
                "acos" => MathFn::Acos,
                "atan" => MathFn::Atan,
                "sqrt" => MathFn::Sqrt,
                "exp" => MathFn::Exp,
                "abs" => MathFn::Abs,
                "sign" => MathFn::Sign,
                _ => unreachable!(),
            };
            Some(CalcNode::Func(func, args))
        }
        "atan2" | "pow" | "mod" | "rem" => {
            if args.len() != 2 {
                return None;
            }
            let func = match name {
                "atan2" => MathFn::Atan2,
                "pow" => MathFn::Pow,
                "mod" => MathFn::Mod,
                "rem" => MathFn::Rem,
                _ => unreachable!(),
            };
            Some(CalcNode::Func(func, args))
        }
        "log" => {
            // 1 или 2 аргумента: log(x) = ln(x), log(x, base) = log_base(x).
            if args.is_empty() || args.len() > 2 {
                return None;
            }
            Some(CalcNode::Func(MathFn::Log, args))
        }
        "hypot" => {
            // 1+ аргумента.
            if args.is_empty() {
                return None;
            }
            Some(CalcNode::Func(MathFn::Hypot, args))
        }
        "round" => {
            // round([<strategy>,] val[, step]). Strategy keyword уже снят
            // вверху функции и зашит в `MathFn::Round(...)`; здесь остаётся
            // классический args-чек 1..=2.
            if args.is_empty() || args.len() > 2 {
                return None;
            }
            let s = round_strategy.unwrap_or(RoundStrategy::Nearest);
            Some(CalcNode::Func(MathFn::Round(s), args))
        }
        _ => None, // незнакомая math-функция
    }
}

/// CSS Values L4 §10.5.1: `<rounding-strategy>` = `nearest | up | down | to-zero`.
/// Имя приходит уже в нижнем регистре из лексера; неподходящий ident → None.
fn parse_round_strategy(name: &str) -> Option<RoundStrategy> {
    match name {
        "nearest" => Some(RoundStrategy::Nearest),
        "up" => Some(RoundStrategy::Up),
        "down" => Some(RoundStrategy::Down),
        "to-zero" => Some(RoundStrategy::ToZero),
        _ => None,
    }
}

/// Парсит список аргументов функции — один или больше expr-ов через
/// запятые. Останавливается перед `)`; не съедает его.
fn parse_arg_list(tokens: &[CalcToken], pos: &mut usize) -> Option<Vec<CalcNode>> {
    let mut args = Vec::new();
    args.push(parse_calc_expr(tokens, pos)?);
    while matches!(tokens.get(*pos), Some(CalcToken::Comma)) {
        *pos += 1;
        args.push(parse_calc_expr(tokens, pos)?);
    }
    Some(args)
}

/// Преобразует пару (число, unit) в `CalcNode`. Пустой unit → `Number`,
/// length-units → `Length::*`, angle-units (deg/rad/turn/grad) →
/// `Number(radians)` (по CSS Values L4 §10.7 — trig-функции принимают
/// число или angle; unitless считается уже в радианах). Неизвестный unit
/// (`pt`, `mm`, …) даёт None.
fn calc_num_to_node(value: f32, unit: &str) -> Option<CalcNode> {
    if unit.is_empty() {
        return Some(CalcNode::Number(value));
    }
    // Angle-units: конвертируем в радианы и храним как Number.
    // Это позволяет sin/cos/tan корректно работать с любой формой угла,
    // и сохраняет результат asin/acos/atan/atan2 как plain number
    // (по умолчанию интерпретируется как радианы при подаче обратно в trig).
    let pi = std::f32::consts::PI;
    match unit {
        "deg" => return Some(CalcNode::Number(value * pi / 180.0)),
        "rad" => return Some(CalcNode::Number(value)),
        "turn" => return Some(CalcNode::Number(value * 2.0 * pi)),
        "grad" => return Some(CalcNode::Number(value * pi / 200.0)),
        _ => {}
    }
    let length = match unit {
        "px" => Length::Px(value),
        "rem" => Length::Rem(value),
        // `ch`/`ex` carry their own variants (resolved against real font metrics
        // at layout time); `cap`/`lh` stay em-approximated (Phase 0, no metric).
        "ch" => Length::Ch(value),
        "ex" => Length::Ex(value),
        "em" => Length::Em(value),
        "cap" => Length::Em(value * 0.7),
        "lh" => Length::Em(value * 1.2),
        "vh" => Length::Vh(value),
        "vw" => Length::Vw(value),
        "vmin" => Length::Vmin(value),
        "vmax" => Length::Vmax(value),
        // Small/Large/Dynamic viewport units → same as vh/vw/vmin/vmax (Phase 0).
        "svh" | "dvh" | "lvh" => Length::Vh(value),
        "svw" | "dvw" | "lvw" => Length::Vw(value),
        "svmin" | "dvmin" | "lvmin" => Length::Vmin(value),
        "svmax" | "dvmax" | "lvmax" => Length::Vmax(value),
        // CSS Container Queries L1 §6.2 — container-relative units.
        "cqw" => Length::Cqw(value),
        "cqh" => Length::Cqh(value),
        "cqi" => Length::Cqi(value),
        "cqb" => Length::Cqb(value),
        "cqmin" => Length::Cqmin(value),
        "cqmax" => Length::Cqmax(value),
        // Absolute units → px (CSS Values L3 §5.2, 96dpi reference pixel).
        "pt" => Length::Px(value * 4.0 / 3.0),
        "pc" => Length::Px(value * 16.0),
        "in" => Length::Px(value * 96.0),
        "cm" => Length::Px(value * 96.0 / 2.54),
        "mm" => Length::Px(value * 96.0 / 25.4),
        "q"  => Length::Px(value * 96.0 / 101.6),
        "%" => Length::Percent(value),
        _ => return None,
    };
    Some(CalcNode::Length(length))
}
