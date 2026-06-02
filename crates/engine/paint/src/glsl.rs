//! Minimal GLSL ES 1.0 interpreter for `SoftwareWebGl` (task #34, §7F).
//!
//! Parses and evaluates the vertex and fragment shaders attached to a WebGL
//! program so that `drawArrays` can compute correct per-vertex positions,
//! interpolate varyings across primitives, and evaluate the fragment colour for
//! each rasterized pixel — instead of always filling with the last `uniform4f`.
//!
//! # Scope
//!
//! Covers the GLSL ES 1.0 idioms used by real-world WebGL demos:
//! - Type system: `float`, `int`, `bool`, `vec2`/`vec3`/`vec4`, `mat4`,
//!   `sampler2D`.
//! - Declarations: `uniform`, `attribute`, `varying`, `precision`.
//! - Expressions: arithmetic (+−×÷), unary minus/not, comparison, logical
//!   and/or, swizzle (`.xyzw` / `.rgba`), vector/matrix constructors, built-in
//!   functions (`vec2/3/4`, `mat4`, `texture2D`, `mix`, `clamp`, `abs`, `min`,
//!   `max`, `pow`, `sqrt`, `length`, `normalize`, `dot`, `cross`, `sin`, `cos`,
//!   `tan`, `step`, `smoothstep`, `fract`, `floor`, `ceil`, `mod`, `sign`).
//! - Statements: variable declaration/init, assignment (with compound `+=` etc.),
//!   `if`/`else`, `for`, `return`, `discard`.
//! - Built-in outputs: `gl_Position` (vertex), `gl_FragColor` (fragment).
//!
//! Unsupported (but silently ignored): user-defined functions (only `main()` is
//! called), arrays (beyond `mat4` columns), `#version`, preprocessor macros.

use std::collections::HashMap;

// ─── Value type ────────────────────────────────────────────────────────────

/// Runtime value inside the GLSL interpreter.
#[derive(Clone, Debug, Default)]
pub enum Val {
    #[default]
    Void,
    Float(f32),
    Int(i32),
    Bool(bool),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    /// Column-major 4×4 matrix.
    Mat4([f32; 16]),
    /// Sampler handle (texture unit index).
    Sampler(u32),
}

impl Val {
    /// Convert any numeric-ish value to a scalar f32.
    pub fn to_float(&self) -> f32 {
        match self {
            Val::Float(v) => *v,
            Val::Int(v) => *v as f32,
            Val::Bool(true) => 1.0,
            Val::Bool(false) => 0.0,
            Val::Vec2(v) => v[0],
            Val::Vec3(v) => v[0],
            Val::Vec4(v) => v[0],
            _ => 0.0,
        }
    }

    /// Convert any value to vec4 (broadcasting rules).
    pub fn to_vec4(&self) -> [f32; 4] {
        match self {
            Val::Vec4(v) => *v,
            Val::Vec3(v) => [v[0], v[1], v[2], 1.0],
            Val::Vec2(v) => [v[0], v[1], 0.0, 1.0],
            Val::Float(v) => [*v, *v, *v, *v],
            Val::Int(v) => { let f = *v as f32; [f, f, f, f] }
            _ => [0.0, 0.0, 0.0, 1.0],
        }
    }

    /// Number of scalar components.
    pub fn components(&self) -> usize {
        match self {
            Val::Float(_) | Val::Int(_) | Val::Bool(_) => 1,
            Val::Vec2(_) => 2,
            Val::Vec3(_) => 3,
            Val::Vec4(_) | Val::Mat4(_) => 4,
            _ => 0,
        }
    }

    /// Read a single float component by index (0-based).
    pub fn get_component(&self, i: usize) -> f32 {
        match self {
            Val::Float(v) => *v,
            Val::Int(v) => *v as f32,
            Val::Bool(true) => 1.0,
            Val::Bool(false) => 0.0,
            Val::Vec2(v) => v.get(i).copied().unwrap_or(0.0),
            Val::Vec3(v) => v.get(i).copied().unwrap_or(0.0),
            Val::Vec4(v) => v.get(i).copied().unwrap_or(0.0),
            _ => 0.0,
        }
    }
}

// ─── Lexer ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    // Literals
    FloatLit(f32),
    IntLit(i32),
    Ident(String),
    // Keywords — types
    KwVoid,
    KwFloat, KwInt, KwBool,
    KwVec2, KwVec3, KwVec4,
    KwMat2, KwMat3, KwMat4,
    KwSampler2D,
    // Keywords — storage qualifiers
    KwUniform, KwAttribute, KwVarying,
    KwConst, KwIn, KwOut, KwInOut,
    // Keywords — precision
    KwPrecision, KwHighp, KwMediump, KwLowp,
    // Keywords — control flow
    KwIf, KwElse, KwFor, KwWhile, KwDo,
    KwReturn, KwDiscard, KwBreak, KwContinue,
    KwTrue, KwFalse,
    // Operators
    Plus, Minus, Star, Slash, Percent,
    PlusEq, MinusEq, StarEq, SlashEq,
    PlusPlus, MinusMinus,
    Eq, EqEq, BangEq,
    Lt, Gt, LtEq, GtEq,
    And, Or, Bang,
    AmpAmp, PipePipe,
    // Punctuation
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Semi, Comma, Dot,
    // End
    Eof,
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    fn new(src: &str) -> Self {
        Self { chars: src.chars().collect(), pos: 0 }
    }

    fn peek(&self) -> char {
        self.chars.get(self.pos).copied().unwrap_or('\0')
    }

    fn peek_next(&self) -> char {
        self.chars.get(self.pos + 1).copied().unwrap_or('\0')
    }

    fn advance(&mut self) -> char {
        let c = self.peek();
        self.pos += 1;
        c
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace
            while self.peek().is_ascii_whitespace() {
                self.advance();
            }
            // Line comment
            if self.peek() == '/' && self.peek_next() == '/' {
                while self.peek() != '\n' && self.peek() != '\0' {
                    self.advance();
                }
                continue;
            }
            // Block comment
            if self.peek() == '/' && self.peek_next() == '*' {
                self.advance(); self.advance();
                loop {
                    if self.peek() == '\0' { break; }
                    if self.peek() == '*' && self.peek_next() == '/' {
                        self.advance(); self.advance();
                        break;
                    }
                    self.advance();
                }
                continue;
            }
            // Preprocessor line — skip whole line
            if self.peek() == '#' {
                while self.peek() != '\n' && self.peek() != '\0' {
                    self.advance();
                }
                continue;
            }
            break;
        }
    }

    fn read_number(&mut self) -> Token {
        let start = self.pos;
        while self.peek().is_ascii_digit() { self.advance(); }
        let is_float = self.peek() == '.' || self.peek() == 'e' || self.peek() == 'E';
        if self.peek() == '.' && self.peek_next().is_ascii_digit() {
            self.advance();
            while self.peek().is_ascii_digit() { self.advance(); }
        } else if self.peek() == '.' {
            // trailing dot like "1."
            self.advance();
        }
        if self.peek() == 'e' || self.peek() == 'E' {
            self.advance();
            if self.peek() == '+' || self.peek() == '-' { self.advance(); }
            while self.peek().is_ascii_digit() { self.advance(); }
        }
        // Skip type suffix: f, u, etc.
        if self.peek() == 'f' || self.peek() == 'F' { self.advance(); }
        let s: String = self.chars[start..self.pos].iter().collect();
        if is_float || s.contains('.') || s.contains('e') || s.contains('E') {
            Token::FloatLit(s.trim_end_matches('f').parse().unwrap_or(0.0))
        } else {
            Token::IntLit(s.parse().unwrap_or(0))
        }
    }

    fn read_ident(&mut self) -> Token {
        let start = self.pos;
        while self.peek().is_alphanumeric() || self.peek() == '_' { self.advance(); }
        let s: String = self.chars[start..self.pos].iter().collect();
        match s.as_str() {
            "void" => Token::KwVoid,
            "float" => Token::KwFloat,
            "int" => Token::KwInt,
            "bool" => Token::KwBool,
            "vec2" => Token::KwVec2,
            "vec3" => Token::KwVec3,
            "vec4" => Token::KwVec4,
            "mat2" => Token::KwMat2,
            "mat3" => Token::KwMat3,
            "mat4" => Token::KwMat4,
            "sampler2D" => Token::KwSampler2D,
            "uniform" => Token::KwUniform,
            "attribute" => Token::KwAttribute,
            "varying" => Token::KwVarying,
            "const" => Token::KwConst,
            "in" => Token::KwIn,
            "out" => Token::KwOut,
            "inout" => Token::KwInOut,
            "precision" => Token::KwPrecision,
            "highp" => Token::KwHighp,
            "mediump" => Token::KwMediump,
            "lowp" => Token::KwLowp,
            "if" => Token::KwIf,
            "else" => Token::KwElse,
            "for" => Token::KwFor,
            "while" => Token::KwWhile,
            "do" => Token::KwDo,
            "return" => Token::KwReturn,
            "discard" => Token::KwDiscard,
            "break" => Token::KwBreak,
            "continue" => Token::KwContinue,
            "true" => Token::KwTrue,
            "false" => Token::KwFalse,
            _ => Token::Ident(s),
        }
    }

    fn next_token(&mut self) -> Token {
        self.skip_whitespace_and_comments();
        let c = self.peek();
        if c == '\0' { return Token::Eof; }
        if c.is_ascii_digit() || (c == '.' && self.peek_next().is_ascii_digit()) {
            return self.read_number();
        }
        if c.is_alphabetic() || c == '_' {
            return self.read_ident();
        }
        self.advance();
        match c {
            '+' => if self.peek() == '+' { self.advance(); Token::PlusPlus }
                   else if self.peek() == '=' { self.advance(); Token::PlusEq }
                   else { Token::Plus },
            '-' => if self.peek() == '-' { self.advance(); Token::MinusMinus }
                   else if self.peek() == '=' { self.advance(); Token::MinusEq }
                   else { Token::Minus },
            '*' => if self.peek() == '=' { self.advance(); Token::StarEq } else { Token::Star },
            '/' => if self.peek() == '=' { self.advance(); Token::SlashEq } else { Token::Slash },
            '%' => Token::Percent,
            '=' => if self.peek() == '=' { self.advance(); Token::EqEq } else { Token::Eq },
            '!' => if self.peek() == '=' { self.advance(); Token::BangEq } else { Token::Bang },
            '<' => if self.peek() == '=' { self.advance(); Token::LtEq } else { Token::Lt },
            '>' => if self.peek() == '=' { self.advance(); Token::GtEq } else { Token::Gt },
            '&' => if self.peek() == '&' { self.advance(); Token::AmpAmp } else { Token::And },
            '|' => if self.peek() == '|' { self.advance(); Token::PipePipe } else { Token::Or },
            '(' => Token::LParen,
            ')' => Token::RParen,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            ';' => Token::Semi,
            ',' => Token::Comma,
            '.' => Token::Dot,
            _ => Token::Eof,
        }
    }

    fn tokenize(mut self) -> Vec<Token> {
        let mut out = Vec::new();
        loop {
            let t = self.next_token();
            let done = t == Token::Eof;
            out.push(t);
            if done { break; }
        }
        out
    }
}

// ─── AST ────────────────────────────────────────────────────────────────────

/// GLSL type tag (declaration-time).
#[derive(Debug, Clone, PartialEq)]
pub enum GlType {
    Void, Float, Int, Bool,
    Vec2, Vec3, Vec4,
    Mat2, Mat3, Mat4,
    Sampler2D,
}

/// Storage qualifier (used during top-level declaration parsing).
#[derive(Debug, Clone, PartialEq)]
enum Storage { Uniform, Attribute, Varying, Local, Const }

/// An assignable location (lvalue).
#[derive(Debug, Clone)]
enum LValue {
    Var(String),
    Swizzle(String, String),
}

/// An expression node.
#[derive(Debug, Clone)]
enum Expr {
    FloatLit(f32),
    IntLit(i32),
    BoolLit(bool),
    Var(String),
    /// `a.xyz` — `Swizzle(inner, mask)`.
    Swizzle(Box<Expr>, String),
    /// Binary operation: `a op b`.
    BinOp(Box<Expr>, BinOpKind, Box<Expr>),
    /// Unary operation: `op a`.
    UnaryOp(UnaryKind, Box<Expr>),
    /// Function / constructor call.
    Call(String, Vec<Expr>),
    /// `var++` / `++var` — increment variable (returns new value).
    Inc(String),
    /// `var--` / `--var` — decrement variable (returns new value).
    Dec(String),
}

#[derive(Debug, Clone, Copy)]
enum BinOpKind {
    Add, Sub, Mul, Div, Rem,
    Eq, Ne, Lt, Gt, Le, Ge,
    And, Or,
}

#[derive(Debug, Clone, Copy)]
enum UnaryKind { Neg, Not }

/// A statement node.
#[derive(Debug, Clone)]
enum Stmt {
    Decl { name: String, init: Option<Expr> },
    Assign { lval: LValue, op: AssignOp, rhs: Expr },
    Expr(Expr),
    If { cond: Expr, then_body: Vec<Stmt>, else_body: Vec<Stmt> },
    For { init: Box<Stmt>, cond: Expr, step: Box<Stmt>, body: Vec<Stmt> },
    While { cond: Expr, body: Vec<Stmt> },
    Return,
    Discard,
    Break,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AssignOp { Plain, Add, Sub, Mul, Div }

// ─── Parsed shader ──────────────────────────────────────────────────────────

/// A parsed GLSL shader: declaration tables + the `main()` function body.
#[derive(Debug, Default)]
pub struct ParsedShader {
    pub uniforms: HashMap<String, GlType>,
    pub attributes: HashMap<String, GlType>,
    pub varyings: HashMap<String, GlType>,
    main_body: Vec<Stmt>,
}

// ─── Parser ─────────────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> &Token {
        let t = self.tokens.get(self.pos).unwrap_or(&Token::Eof);
        self.pos += 1;
        t
    }

    fn expect(&mut self, tok: &Token) -> bool {
        if self.peek() == tok { self.advance(); true } else { false }
    }

    fn consume_semi(&mut self) { self.expect(&Token::Semi); }

    fn is_type_keyword(tok: &Token) -> bool {
        matches!(tok, Token::KwVoid | Token::KwFloat | Token::KwInt | Token::KwBool
            | Token::KwVec2 | Token::KwVec3 | Token::KwVec4
            | Token::KwMat2 | Token::KwMat3 | Token::KwMat4 | Token::KwSampler2D)
    }

    fn parse_type(&mut self) -> GlType {
        match self.advance() {
            Token::KwFloat => GlType::Float,
            Token::KwInt => GlType::Int,
            Token::KwBool => GlType::Bool,
            Token::KwVec2 => GlType::Vec2,
            Token::KwVec3 => GlType::Vec3,
            Token::KwVec4 => GlType::Vec4,
            Token::KwMat2 => GlType::Mat2,
            Token::KwMat3 => GlType::Mat3,
            Token::KwMat4 => GlType::Mat4,
            Token::KwSampler2D => GlType::Sampler2D,
            Token::KwVoid => GlType::Void,
            _ => GlType::Float,
        }
    }

    fn parse_name(&mut self) -> String {
        if let Token::Ident(s) = self.peek() {
            let s = s.clone();
            self.advance();
            s
        } else {
            self.advance();
            String::new()
        }
    }

    /// Parse the whole shader source into a `ParsedShader`.
    fn parse_shader(&mut self) -> ParsedShader {
        let mut shader = ParsedShader::default();
        loop {
            if self.peek() == &Token::Eof { break; }
            self.parse_top_level(&mut shader);
        }
        shader
    }

    /// Parse one top-level declaration or function.
    fn parse_top_level(&mut self, shader: &mut ParsedShader) {
        // Precision qualifiers: skip
        if self.peek() == &Token::KwPrecision {
            self.advance();
            // skip precision_qualifier type ;
            while self.peek() != &Token::Semi && self.peek() != &Token::Eof {
                self.advance();
            }
            self.consume_semi();
            return;
        }

        // Storage qualifier
        let storage = match self.peek() {
            Token::KwUniform => { self.advance(); Some(Storage::Uniform) },
            Token::KwAttribute => { self.advance(); Some(Storage::Attribute) },
            Token::KwVarying => { self.advance(); Some(Storage::Varying) },
            Token::KwConst => { self.advance(); Some(Storage::Const) },
            _ => None,
        };

        // Optional precision qualifier
        if matches!(self.peek(), Token::KwHighp | Token::KwMediump | Token::KwLowp) {
            self.advance();
        }

        if !Self::is_type_keyword(self.peek()) {
            // Unknown — skip to next semicolon or brace
            while !matches!(self.peek(), Token::Semi | Token::LBrace | Token::Eof) {
                self.advance();
            }
            if self.peek() == &Token::Semi { self.advance(); }
            return;
        }

        let ty = self.parse_type();
        let name = self.parse_name();

        if self.peek() == &Token::LParen {
            // Function definition — only `main` is interpreted
            let is_main = name == "main";
            // Skip parameter list
            self.expect(&Token::LParen);
            let mut depth = 1;
            while depth > 0 && self.peek() != &Token::Eof {
                if self.peek() == &Token::LParen { depth += 1; }
                if self.peek() == &Token::RParen { depth -= 1; }
                self.advance();
            }
            if is_main {
                shader.main_body = self.parse_block();
            } else {
                self.skip_block();
            }
            return;
        }

        // Variable declaration; may have array brackets (skip)
        if self.peek() == &Token::LBracket {
            while self.peek() != &Token::Semi && self.peek() != &Token::Eof {
                self.advance();
            }
        }

        // Optional initialiser (skip for top-level uniforms/attributes)
        if self.peek() == &Token::Eq {
            self.advance();
            while !matches!(self.peek(), Token::Semi | Token::Eof) {
                self.advance();
            }
        }

        self.consume_semi();

        let storage = storage.unwrap_or(Storage::Local);
        match storage {
            Storage::Uniform => { shader.uniforms.insert(name, ty); },
            Storage::Attribute => { shader.attributes.insert(name, ty); },
            Storage::Varying => { shader.varyings.insert(name, ty); },
            _ => {},
        }
    }

    fn skip_block(&mut self) {
        if self.peek() != &Token::LBrace { return; }
        self.advance();
        let mut depth = 1;
        while depth > 0 && self.peek() != &Token::Eof {
            if self.peek() == &Token::LBrace { depth += 1; }
            if self.peek() == &Token::RBrace { depth -= 1; }
            self.advance();
        }
    }

    fn parse_block(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        self.expect(&Token::LBrace);
        loop {
            if matches!(self.peek(), Token::RBrace | Token::Eof) { break; }
            if let Some(s) = self.parse_stmt() {
                stmts.push(s);
            }
        }
        self.expect(&Token::RBrace);
        stmts
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        match self.peek() {
            Token::Semi => { self.advance(); return None; },
            Token::KwReturn => {
                self.advance();
                if self.peek() != &Token::Semi {
                    let _ = self.parse_expr(); // consume return expr (void main() — unused)
                }
                self.consume_semi();
                return Some(Stmt::Return);
            },
            Token::KwDiscard => {
                self.advance(); self.consume_semi();
                return Some(Stmt::Discard);
            },
            Token::KwBreak => {
                self.advance(); self.consume_semi();
                return Some(Stmt::Break);
            },
            Token::KwContinue => {
                self.advance(); self.consume_semi();
                return Some(Stmt::Continue);
            },
            Token::KwIf => {
                self.advance();
                self.expect(&Token::LParen);
                let cond = self.parse_expr();
                self.expect(&Token::RParen);
                let then_body = if self.peek() == &Token::LBrace {
                    self.parse_block()
                } else {
                    self.parse_stmt().into_iter().collect()
                };
                let else_body = if self.peek() == &Token::KwElse {
                    self.advance();
                    if self.peek() == &Token::LBrace {
                        self.parse_block()
                    } else {
                        self.parse_stmt().into_iter().collect()
                    }
                } else {
                    Vec::new()
                };
                return Some(Stmt::If { cond, then_body, else_body });
            },
            Token::KwFor => {
                self.advance();
                self.expect(&Token::LParen);
                let init = Box::new(self.parse_stmt().unwrap_or(Stmt::Break));
                let cond = if self.peek() == &Token::Semi {
                    self.advance();
                    Expr::BoolLit(true)
                } else {
                    let e = self.parse_expr();
                    self.consume_semi();
                    e
                };
                // Step: expression without semicolon
                let step_expr = if self.peek() != &Token::RParen {
                    Some(self.parse_expr())
                } else {
                    None
                };
                self.expect(&Token::RParen);
                let body = if self.peek() == &Token::LBrace {
                    self.parse_block()
                } else {
                    self.parse_stmt().into_iter().collect()
                };
                let step = Box::new(step_expr.map(Stmt::Expr).unwrap_or(Stmt::Break));
                return Some(Stmt::For { init, cond, step, body });
            },
            Token::KwWhile => {
                self.advance();
                self.expect(&Token::LParen);
                let cond = self.parse_expr();
                self.expect(&Token::RParen);
                let body = self.parse_block();
                return Some(Stmt::While { cond, body });
            },
            _ => {},
        }

        // Precision qualifier inside function — skip
        if self.peek() == &Token::KwPrecision {
            while !matches!(self.peek(), Token::Semi | Token::Eof) { self.advance(); }
            self.consume_semi();
            return None;
        }

        // Type-keyword start → variable declaration
        let is_decl = Self::is_type_keyword(self.peek())
            || matches!(self.peek(), Token::KwConst | Token::KwHighp | Token::KwMediump | Token::KwLowp);
        if is_decl {
            // Skip precision
            if matches!(self.peek(), Token::KwConst) { self.advance(); }
            if matches!(self.peek(), Token::KwHighp | Token::KwMediump | Token::KwLowp) { self.advance(); }
            let ty = self.parse_type();
            let name = self.parse_name();
            let init = if self.peek() == &Token::Eq {
                self.advance();
                Some(self.parse_expr())
            } else { None };
            self.consume_semi();
            let _ = ty; // type info not needed at runtime
            return Some(Stmt::Decl { name, init });
        }

        // Expression statement / assignment
        let lhs = self.parse_expr();
        let op = match self.peek() {
            Token::Eq => { self.advance(); Some(AssignOp::Plain) },
            Token::PlusEq => { self.advance(); Some(AssignOp::Add) },
            Token::MinusEq => { self.advance(); Some(AssignOp::Sub) },
            Token::StarEq => { self.advance(); Some(AssignOp::Mul) },
            Token::SlashEq => { self.advance(); Some(AssignOp::Div) },
            _ => None,
        };
        if let Some(op) = op {
            let rhs = self.parse_expr();
            self.consume_semi();
            let lval = expr_to_lvalue(lhs);
            return Some(Stmt::Assign { lval, op, rhs });
        }
        self.consume_semi();
        Some(Stmt::Expr(lhs))
    }

    /// Parse a full expression (lowest precedence).
    fn parse_expr(&mut self) -> Expr {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Expr {
        let mut e = self.parse_and();
        while self.peek() == &Token::PipePipe {
            self.advance();
            let r = self.parse_and();
            e = Expr::BinOp(Box::new(e), BinOpKind::Or, Box::new(r));
        }
        e
    }

    fn parse_and(&mut self) -> Expr {
        let mut e = self.parse_equality();
        while self.peek() == &Token::AmpAmp {
            self.advance();
            let r = self.parse_equality();
            e = Expr::BinOp(Box::new(e), BinOpKind::And, Box::new(r));
        }
        e
    }

    fn parse_equality(&mut self) -> Expr {
        let mut e = self.parse_relational();
        loop {
            let op = match self.peek() {
                Token::EqEq => BinOpKind::Eq,
                Token::BangEq => BinOpKind::Ne,
                _ => break,
            };
            self.advance();
            let r = self.parse_relational();
            e = Expr::BinOp(Box::new(e), op, Box::new(r));
        }
        e
    }

    fn parse_relational(&mut self) -> Expr {
        let mut e = self.parse_add();
        loop {
            let op = match self.peek() {
                Token::Lt => BinOpKind::Lt,
                Token::Gt => BinOpKind::Gt,
                Token::LtEq => BinOpKind::Le,
                Token::GtEq => BinOpKind::Ge,
                _ => break,
            };
            self.advance();
            let r = self.parse_add();
            e = Expr::BinOp(Box::new(e), op, Box::new(r));
        }
        e
    }

    fn parse_add(&mut self) -> Expr {
        let mut e = self.parse_mul();
        loop {
            let op = match self.peek() {
                Token::Plus => BinOpKind::Add,
                Token::Minus => BinOpKind::Sub,
                _ => break,
            };
            self.advance();
            let r = self.parse_mul();
            e = Expr::BinOp(Box::new(e), op, Box::new(r));
        }
        e
    }

    fn parse_mul(&mut self) -> Expr {
        let mut e = self.parse_unary();
        loop {
            let op = match self.peek() {
                Token::Star => BinOpKind::Mul,
                Token::Slash => BinOpKind::Div,
                Token::Percent => BinOpKind::Rem,
                _ => break,
            };
            self.advance();
            let r = self.parse_unary();
            e = Expr::BinOp(Box::new(e), op, Box::new(r));
        }
        e
    }

    fn parse_unary(&mut self) -> Expr {
        if self.peek() == &Token::Minus {
            self.advance();
            return Expr::UnaryOp(UnaryKind::Neg, Box::new(self.parse_unary()));
        }
        if self.peek() == &Token::Bang {
            self.advance();
            return Expr::UnaryOp(UnaryKind::Not, Box::new(self.parse_unary()));
        }
        // Prefix ++ / --
        if matches!(self.peek(), Token::PlusPlus | Token::MinusMinus) {
            let is_inc = self.peek() == &Token::PlusPlus;
            self.advance();
            let inner = self.parse_postfix();
            if let Expr::Var(name) = inner {
                return if is_inc { Expr::Inc(name) } else { Expr::Dec(name) };
            }
            // Non-variable prefix op — ignore
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Expr {
        let mut e = self.parse_primary();
        loop {
            if self.peek() == &Token::Dot {
                self.advance();
                let member = self.parse_name();
                e = Expr::Swizzle(Box::new(e), member);
            } else if self.peek() == &Token::LBracket {
                // Array index — advance past the expression and brackets.
                self.advance();
                let _ = self.parse_expr();
                self.expect(&Token::RBracket);
                // Just keep the base expression (indexing unsupported).
            } else if matches!(self.peek(), Token::PlusPlus | Token::MinusMinus) {
                let is_inc = self.peek() == &Token::PlusPlus;
                self.advance();
                if let Expr::Var(name) = &e {
                    let name = name.clone();
                    e = if is_inc { Expr::Inc(name) } else { Expr::Dec(name) };
                }
                // For non-variable postfix ++ — keep base expression unchanged.
            } else {
                break;
            }
        }
        e
    }

    fn parse_primary(&mut self) -> Expr {
        match self.peek().clone() {
            Token::FloatLit(v) => { self.advance(); Expr::FloatLit(v) },
            Token::IntLit(v) => { self.advance(); Expr::IntLit(v) },
            Token::KwTrue => { self.advance(); Expr::BoolLit(true) },
            Token::KwFalse => { self.advance(); Expr::BoolLit(false) },
            Token::LParen => {
                self.advance();
                let e = self.parse_expr();
                self.expect(&Token::RParen);
                e
            },
            Token::Ident(_) | Token::KwFloat | Token::KwInt | Token::KwBool
            | Token::KwVec2 | Token::KwVec3 | Token::KwVec4
            | Token::KwMat2 | Token::KwMat3 | Token::KwMat4 => {
                let name = match self.peek() {
                    Token::Ident(s) => s.clone(),
                    Token::KwFloat => "float".to_string(),
                    Token::KwInt => "int".to_string(),
                    Token::KwBool => "bool".to_string(),
                    Token::KwVec2 => "vec2".to_string(),
                    Token::KwVec3 => "vec3".to_string(),
                    Token::KwVec4 => "vec4".to_string(),
                    Token::KwMat2 => "mat2".to_string(),
                    Token::KwMat3 => "mat3".to_string(),
                    Token::KwMat4 => "mat4".to_string(),
                    _ => String::new(),
                };
                self.advance();
                if self.peek() == &Token::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    while self.peek() != &Token::RParen && self.peek() != &Token::Eof {
                        args.push(self.parse_expr());
                        if self.peek() == &Token::Comma { self.advance(); }
                    }
                    self.expect(&Token::RParen);
                    Expr::Call(name, args)
                } else {
                    Expr::Var(name)
                }
            },
            _ => { self.advance(); Expr::FloatLit(0.0) },
        }
    }
}

fn expr_to_lvalue(e: Expr) -> LValue {
    match e {
        Expr::Var(n) => LValue::Var(n),
        Expr::Swizzle(inner, mask) => {
            if let Expr::Var(n) = *inner {
                LValue::Swizzle(n, mask)
            } else {
                LValue::Var(String::new())
            }
        },
        _ => LValue::Var(String::new()),
    }
}

// ─── Public API: parse ───────────────────────────────────────────────────────

/// Parse a GLSL ES shader source string.
pub fn parse(src: &str) -> ParsedShader {
    let tokens = Lexer::new(src).tokenize();
    let mut p = Parser::new(tokens);
    p.parse_shader()
}

// ─── Interpreter ────────────────────────────────────────────────────────────

/// Execution environment for a single shader invocation.
pub struct ShaderEnv<'a> {
    /// Uniform values (name → value).
    pub uniforms: &'a HashMap<String, Val>,
    /// Per-vertex attribute values (name → value), empty in fragment shader.
    pub attributes: HashMap<String, Val>,
    /// Varying values (name → value).
    pub varyings: HashMap<String, Val>,
    /// Local variables declared inside `main()`.
    locals: HashMap<String, Val>,
    /// `gl_Position` output (vertex shader).
    pub position: [f32; 4],
    /// `gl_FragColor` output (fragment shader).
    pub frag_color: [f32; 4],
    /// Set to `true` if `discard;` was executed.
    pub discard: bool,
}

impl<'a> ShaderEnv<'a> {
    pub fn new(uniforms: &'a HashMap<String, Val>) -> Self {
        Self {
            uniforms,
            attributes: HashMap::new(),
            varyings: HashMap::new(),
            locals: HashMap::new(),
            position: [0.0, 0.0, 0.0, 1.0],
            frag_color: [0.0, 0.0, 0.0, 0.0],
            discard: false,
        }
    }

    fn get_var(&self, name: &str) -> Val {
        if let Some(v) = self.locals.get(name) { return v.clone(); }
        if let Some(v) = self.varyings.get(name) { return v.clone(); }
        if let Some(v) = self.attributes.get(name) { return v.clone(); }
        if let Some(v) = self.uniforms.get(name) { return v.clone(); }
        Val::Float(0.0)
    }

    fn set_var(&mut self, name: &str, val: Val) {
        match name {
            "gl_Position" => self.position = val.to_vec4(),
            "gl_FragColor" => self.frag_color = val.to_vec4(),
            _ => {
                if self.varyings.contains_key(name) {
                    self.varyings.insert(name.to_string(), val);
                } else {
                    self.locals.insert(name.to_string(), val);
                }
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum Flow { Continue, Return, Discard, Break }

/// Execute the `main()` function of a parsed shader.
pub fn exec_main(shader: &ParsedShader, env: &mut ShaderEnv) {
    // Pre-seed varying names so set_var writes to varyings, not locals.
    for name in shader.varyings.keys() {
        env.varyings.entry(name.clone()).or_insert(Val::Float(0.0));
    }
    exec_stmts(&shader.main_body, env);
}

fn exec_stmts(stmts: &[Stmt], env: &mut ShaderEnv) -> Flow {
    for stmt in stmts {
        match exec_stmt(stmt, env) {
            Flow::Continue => {},
            other => return other,
        }
    }
    Flow::Continue
}

fn exec_stmt(stmt: &Stmt, env: &mut ShaderEnv) -> Flow {
    match stmt {
        Stmt::Decl { name, init } => {
            let val = init.as_ref().map(|e| eval_expr(e, env)).unwrap_or_default();
            env.locals.insert(name.clone(), val);
            Flow::Continue
        },
        Stmt::Assign { lval, op, rhs } => {
            let rval = eval_expr(rhs, env);
            let new_val = match op {
                AssignOp::Plain => rval,
                AssignOp::Add => binop_val(env.get_lval(lval), BinOpKind::Add, rval),
                AssignOp::Sub => binop_val(env.get_lval(lval), BinOpKind::Sub, rval),
                AssignOp::Mul => binop_val(env.get_lval(lval), BinOpKind::Mul, rval),
                AssignOp::Div => binop_val(env.get_lval(lval), BinOpKind::Div, rval),
            };
            env.set_lval(lval, new_val);
            Flow::Continue
        },
        Stmt::Expr(e) => {
            eval_expr(e, env);
            Flow::Continue
        },
        Stmt::Return => Flow::Return,
        Stmt::Discard => {
            env.discard = true;
            Flow::Discard
        },
        Stmt::Break => Flow::Break,
        Stmt::Continue => Flow::Continue,
        Stmt::If { cond, then_body, else_body } => {
            let cv = eval_expr(cond, env);
            let branch = if val_truthy(&cv) { then_body } else { else_body };
            exec_stmts(branch, env)
        },
        Stmt::For { init, cond, step, body } => {
            exec_stmt(init, env);
            for _ in 0..4096 {
                let cv = eval_expr(cond, env);
                if !val_truthy(&cv) { break; }
                match exec_stmts(body, env) {
                    Flow::Break | Flow::Return | Flow::Discard => break,
                    _ => {},
                }
                exec_stmt(step, env);
            }
            Flow::Continue
        },
        Stmt::While { cond, body } => {
            for _ in 0..4096 {
                let cv = eval_expr(cond, env);
                if !val_truthy(&cv) { break; }
                match exec_stmts(body, env) {
                    Flow::Break | Flow::Return | Flow::Discard => break,
                    _ => {},
                }
            }
            Flow::Continue
        },
    }
}

impl ShaderEnv<'_> {
    fn get_lval(&self, lval: &LValue) -> Val {
        match lval {
            LValue::Var(n) => self.get_var(n),
            LValue::Swizzle(n, mask) => {
                let base = self.get_var(n);
                apply_swizzle_read(&base, mask)
            },
        }
    }

    fn set_lval(&mut self, lval: &LValue, val: Val) {
        match lval {
            LValue::Var(n) => self.set_var(n, val),
            LValue::Swizzle(var_name, mask) => {
                let base = self.get_var(var_name);
                let merged = apply_swizzle_write(base, mask, &val);
                self.set_var(var_name, merged);
            },
        }
    }
}

fn val_truthy(v: &Val) -> bool {
    match v {
        Val::Bool(b) => *b,
        Val::Float(f) => *f != 0.0,
        Val::Int(i) => *i != 0,
        _ => true,
    }
}

// ─── Expression evaluator ───────────────────────────────────────────────────

fn eval_expr(expr: &Expr, env: &mut ShaderEnv) -> Val {
    match expr {
        Expr::FloatLit(v) => Val::Float(*v),
        Expr::IntLit(v) => Val::Int(*v),
        Expr::BoolLit(v) => Val::Bool(*v),
        Expr::Var(name) => match name.as_str() {
            "gl_Position" => Val::Vec4(env.position),
            "gl_FragColor" => Val::Vec4(env.frag_color),
            _ => env.get_var(name),
        },
        Expr::Swizzle(inner, mask) => {
            let base = eval_expr(inner, env);
            apply_swizzle_read(&base, mask)
        },
        Expr::BinOp(l, op, r) => {
            let lv = eval_expr(l, env);
            let rv = eval_expr(r, env);
            binop_val(lv, *op, rv)
        },
        Expr::UnaryOp(op, inner) => {
            let v = eval_expr(inner, env);
            match op {
                UnaryKind::Neg => negate_val(v),
                UnaryKind::Not => {
                    match v {
                        Val::Bool(b) => Val::Bool(!b),
                        other => Val::Bool(!val_truthy(&other)),
                    }
                },
            }
        },
        Expr::Call(name, args) => eval_call(name, args, env),
        Expr::Inc(name) => {
            let old = env.get_var(name);
            let new_val = match old {
                Val::Int(i) => Val::Int(i + 1),
                Val::Float(f) => Val::Float(f + 1.0),
                _ => Val::Int(1),
            };
            env.set_var(name, new_val.clone());
            new_val
        },
        Expr::Dec(name) => {
            let old = env.get_var(name);
            let new_val = match old {
                Val::Int(i) => Val::Int(i - 1),
                Val::Float(f) => Val::Float(f - 1.0),
                _ => Val::Int(-1),
            };
            env.set_var(name, new_val.clone());
            new_val
        },
    }
}

fn negate_val(v: Val) -> Val {
    match v {
        Val::Float(f) => Val::Float(-f),
        Val::Int(i) => Val::Int(-i),
        Val::Vec2([x, y]) => Val::Vec2([-x, -y]),
        Val::Vec3([x, y, z]) => Val::Vec3([-x, -y, -z]),
        Val::Vec4([x, y, z, w]) => Val::Vec4([-x, -y, -z, -w]),
        other => other,
    }
}

fn binop_val(l: Val, op: BinOpKind, r: Val) -> Val {
    // Comparison → bool
    if matches!(op, BinOpKind::Eq | BinOpKind::Ne | BinOpKind::Lt | BinOpKind::Gt | BinOpKind::Le | BinOpKind::Ge) {
        let lf = l.to_float();
        let rf = r.to_float();
        return Val::Bool(match op {
            BinOpKind::Eq => (lf - rf).abs() < f32::EPSILON,
            BinOpKind::Ne => (lf - rf).abs() >= f32::EPSILON,
            BinOpKind::Lt => lf < rf,
            BinOpKind::Gt => lf > rf,
            BinOpKind::Le => lf <= rf,
            BinOpKind::Ge => lf >= rf,
            _ => unreachable!(),
        });
    }
    if matches!(op, BinOpKind::And) {
        return Val::Bool(val_truthy(&l) && val_truthy(&r));
    }
    if matches!(op, BinOpKind::Or) {
        return Val::Bool(val_truthy(&l) || val_truthy(&r));
    }

    // Matrix multiply — handled before generic component-wise path.
    if matches!(op, BinOpKind::Mul) {
        match (&l, &r) {
            (Val::Mat4(m), Val::Vec4(v)) => return Val::Vec4(mat4_mul_vec4(m, v)),
            (Val::Vec4(v), Val::Mat4(m)) => return Val::Vec4(mat4_mul_vec4(m, v)),
            (Val::Mat4(a), Val::Mat4(b)) => return Val::Mat4(mat4_mul_mat4(a, b)),
            (Val::Mat4(m), Val::Float(s)) => {
                let s = *s;
                return Val::Mat4(m.map(|v| v * s));
            },
            _ => {}
        }
    }

    // Component-wise for vectors
    let nc = l.components().max(r.components());
    if nc > 1 {
        let ls: Vec<f32> = (0..nc).map(|i| l.get_component(i)).collect();
        let rs: Vec<f32> = (0..nc).map(|i| r.get_component(i)).collect();
        let result: Vec<f32> = ls.iter().zip(rs.iter()).map(|(&a, &b)| scalar_op(a, op, b)).collect();
        return match nc {
            2 => Val::Vec2([result[0], result[1]]),
            3 => Val::Vec3([result[0], result[1], result[2]]),
            _ => Val::Vec4([result[0], result[1], result[2], result[3]]),
        };
    }

    // Scalar
    let lf = l.to_float();
    let rf = r.to_float();
    match (l, r) {
        (Val::Int(_), Val::Int(_)) => {
            let li = lf as i32;
            let ri = rf as i32;
            Val::Int(int_op(li, op, ri))
        },
        _ => Val::Float(scalar_op(lf, op, rf)),
    }
}

fn int_op(a: i32, op: BinOpKind, b: i32) -> i32 {
    match op {
        BinOpKind::Add => a + b,
        BinOpKind::Sub => a - b,
        BinOpKind::Mul => a * b,
        BinOpKind::Div if b != 0 => a / b,
        BinOpKind::Div => 0,
        BinOpKind::Rem if b != 0 => a % b,
        BinOpKind::Rem => 0,
        _ => 0,
    }
}

fn scalar_op(a: f32, op: BinOpKind, b: f32) -> f32 {
    match op {
        BinOpKind::Add => a + b,
        BinOpKind::Sub => a - b,
        BinOpKind::Mul => a * b,
        BinOpKind::Div if b.abs() > f32::EPSILON => a / b,
        BinOpKind::Div => 0.0,
        BinOpKind::Rem => a % b,
        _ => 0.0,
    }
}

fn mat4_mul_vec4(m: &[f32; 16], v: &[f32; 4]) -> [f32; 4] {
    // Column-major: col j starts at m[j*4]
    [
        m[0]*v[0] + m[4]*v[1] + m[8]*v[2]  + m[12]*v[3],
        m[1]*v[0] + m[5]*v[1] + m[9]*v[2]  + m[13]*v[3],
        m[2]*v[0] + m[6]*v[1] + m[10]*v[2] + m[14]*v[3],
        m[3]*v[0] + m[7]*v[1] + m[11]*v[2] + m[15]*v[3],
    ]
}

fn mat4_mul_mat4(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut s = 0.0f32;
            for k in 0..4 {
                s += a[k*4 + row] * b[col*4 + k];
            }
            out[col*4 + row] = s;
        }
    }
    out
}

// ─── Swizzle ────────────────────────────────────────────────────────────────

fn swizzle_index(c: char) -> Option<usize> {
    match c {
        'x' | 'r' | 's' => Some(0),
        'y' | 'g' | 't' => Some(1),
        'z' | 'b' | 'p' => Some(2),
        'w' | 'a' | 'q' => Some(3),
        _ => None,
    }
}

fn apply_swizzle_read(val: &Val, mask: &str) -> Val {
    let components: Vec<f32> = mask.chars()
        .filter_map(|c| swizzle_index(c).map(|i| val.get_component(i)))
        .collect();
    match components.len() {
        0 => val.clone(),
        1 => Val::Float(components[0]),
        2 => Val::Vec2([components[0], components[1]]),
        3 => Val::Vec3([components[0], components[1], components[2]]),
        _ => Val::Vec4([components[0], components[1], components[2], components[3]]),
    }
}

fn apply_swizzle_write(base: Val, mask: &str, src: &Val) -> Val {
    let mut arr = [0f32; 4];
    let nc = match &base {
        Val::Float(_) => 1, Val::Vec2(_) => 2, Val::Vec3(_) => 3,
        Val::Vec4(_) => 4, _ => 4,
    };
    arr[..nc].iter_mut().enumerate().for_each(|(i, x)| *x = base.get_component(i));
    for (si, c) in mask.chars().enumerate() {
        if let Some(di) = swizzle_index(c) && di < 4 {
            arr[di] = src.get_component(si);
        }
    }
    match nc {
        1 => Val::Float(arr[0]),
        2 => Val::Vec2([arr[0], arr[1]]),
        3 => Val::Vec3([arr[0], arr[1], arr[2]]),
        _ => Val::Vec4([arr[0], arr[1], arr[2], arr[3]]),
    }
}

// ─── Built-in function calls ─────────────────────────────────────────────────

fn eval_call(name: &str, args: &[Expr], env: &mut ShaderEnv) -> Val {
    // Evaluate args first (except texture2D which needs sampler info)
    let vals: Vec<Val> = args.iter().map(|a| eval_expr(a, env)).collect();

    match name {
        // ── Constructors ──────────────────────────────────────────────
        "vec2" => {
            let floats = expand_to_n(&vals, 2);
            Val::Vec2([floats[0], floats[1]])
        },
        "vec3" => {
            let floats = expand_to_n(&vals, 3);
            Val::Vec3([floats[0], floats[1], floats[2]])
        },
        "vec4" => {
            let floats = expand_to_n(&vals, 4);
            Val::Vec4([floats[0], floats[1], floats[2], floats[3]])
        },
        "float" => {
            Val::Float(vals.first().map(|v| v.to_float()).unwrap_or(0.0))
        },
        "int" => {
            Val::Int(vals.first().map(|v| v.to_float() as i32).unwrap_or(0))
        },
        "bool" => {
            Val::Bool(vals.first().map(val_truthy).unwrap_or(false))
        },
        "mat2" => {
            let fs = expand_to_n(&vals, 4);
            Val::Mat4([fs[0], fs[1], 0.0, 0.0, fs[2], fs[3], 0.0, 0.0,
                       0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0])
        },
        "mat3" => {
            let fs = expand_to_n(&vals, 9);
            Val::Mat4([fs[0], fs[1], fs[2], 0.0, fs[3], fs[4], fs[5], 0.0,
                       fs[6], fs[7], fs[8], 0.0, 0.0, 0.0, 0.0, 1.0])
        },
        "mat4" => {
            let fs = expand_to_n(&vals, 16);
            Val::Mat4([fs[0],fs[1],fs[2],fs[3], fs[4],fs[5],fs[6],fs[7],
                       fs[8],fs[9],fs[10],fs[11], fs[12],fs[13],fs[14],fs[15]])
        },
        // ── Texture sampling ──────────────────────────────────────────
        "texture2D" | "texture" => {
            // Without a real texture store, return a mid-grey sample so textured
            // objects appear in a neutral tone rather than black.
            // The JS side attaches texture data via `_lumen_webgl_tex_*`
            // bindings tracked in env.uniforms as `Val::Sampler`.
            // If the sampler carries a solid colour, return that colour.
            if let Some(Val::Vec4(rgba)) = vals.first().and_then(|v| {
                if let Val::Sampler(unit) = v {
                    let key = format!("__tex_{}", unit);
                    env.uniforms.get(&key).cloned()
                } else { None }
            }) {
                return Val::Vec4(rgba);
            }
            Val::Vec4([0.5, 0.5, 0.5, 1.0])
        },
        // ── Math built-ins ────────────────────────────────────────────
        "abs" => map1(&vals, f32::abs),
        "sign" => map1(&vals, |v| if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 }),
        "floor" => map1(&vals, f32::floor),
        "ceil" => map1(&vals, f32::ceil),
        "fract" => map1(&vals, |v| v - v.floor()),
        "sqrt" => map1(&vals, f32::sqrt),
        "inversesqrt" => map1(&vals, |v| 1.0 / v.sqrt()),
        "sin" => map1(&vals, f32::sin),
        "cos" => map1(&vals, f32::cos),
        "tan" => map1(&vals, f32::tan),
        "asin" => map1(&vals, f32::asin),
        "acos" => map1(&vals, f32::acos),
        "atan" => {
            if vals.len() >= 2 {
                map2(&vals, f32::atan2)
            } else {
                map1(&vals, f32::atan)
            }
        },
        "exp" => map1(&vals, f32::exp),
        "log" => map1(&vals, f32::ln),
        "exp2" => map1(&vals, |v| v.exp2()),
        "log2" => map1(&vals, f32::log2),
        "radians" => map1(&vals, |v| v * std::f32::consts::PI / 180.0),
        "degrees" => map1(&vals, |v| v * 180.0 / std::f32::consts::PI),
        "pow" => map2(&vals, f32::powf),
        "min" => map2(&vals, f32::min),
        "max" => map2(&vals, f32::max),
        "mod" => map2(&vals, |a, b| a - b * (a / b).floor()),
        "clamp" => {
            let v0 = vals.first().cloned().unwrap_or_default();
            let lo = vals.get(1).map(|v| v.to_float()).unwrap_or(0.0);
            let hi = vals.get(2).map(|v| v.to_float()).unwrap_or(1.0);
            map_component(&v0, |c| c.clamp(lo, hi))
        },
        "mix" => {
            let a = vals.first().cloned().unwrap_or_default();
            let b = vals.get(1).cloned().unwrap_or_default();
            let t = vals.get(2).map(|v| v.to_float()).unwrap_or(0.0);
            let nc = a.components().max(b.components());
            let result: Vec<f32> = (0..nc).map(|i| a.get_component(i) * (1.0 - t) + b.get_component(i) * t).collect();
            vec_to_val(result)
        },
        "step" => {
            let edge = vals.first().map(|v| v.to_float()).unwrap_or(0.0);
            let x = vals.get(1).cloned().unwrap_or_default();
            map_component(&x, |c| if c < edge { 0.0 } else { 1.0 })
        },
        "smoothstep" => {
            let lo = vals.first().map(|v| v.to_float()).unwrap_or(0.0);
            let hi = vals.get(1).map(|v| v.to_float()).unwrap_or(1.0);
            let x = vals.get(2).cloned().unwrap_or_default();
            map_component(&x, |c| {
                let t = ((c - lo) / (hi - lo)).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            })
        },
        "length" => {
            let v = vals.first().cloned().unwrap_or_default();
            let sum: f32 = (0..v.components()).map(|i| { let c = v.get_component(i); c*c }).sum();
            Val::Float(sum.sqrt())
        },
        "distance" => {
            let a = vals.first().cloned().unwrap_or_default();
            let b = vals.get(1).cloned().unwrap_or_default();
            let nc = a.components();
            let sum: f32 = (0..nc).map(|i| { let d = a.get_component(i) - b.get_component(i); d*d }).sum();
            Val::Float(sum.sqrt())
        },
        "dot" => {
            let a = vals.first().cloned().unwrap_or_default();
            let b = vals.get(1).cloned().unwrap_or_default();
            let nc = a.components();
            Val::Float((0..nc).map(|i| a.get_component(i) * b.get_component(i)).sum())
        },
        "cross" => {
            let a = vals.first().cloned().unwrap_or_default();
            let b = vals.get(1).cloned().unwrap_or_default();
            Val::Vec3([
                a.get_component(1)*b.get_component(2) - a.get_component(2)*b.get_component(1),
                a.get_component(2)*b.get_component(0) - a.get_component(0)*b.get_component(2),
                a.get_component(0)*b.get_component(1) - a.get_component(1)*b.get_component(0),
            ])
        },
        "normalize" => {
            let v = vals.first().cloned().unwrap_or_default();
            let nc = v.components();
            let len: f32 = (0..nc).map(|i| { let c = v.get_component(i); c*c }).sum::<f32>().sqrt();
            if len < f32::EPSILON { return v; }
            vec_to_val((0..nc).map(|i| v.get_component(i) / len).collect())
        },
        "reflect" => {
            let i = vals.first().cloned().unwrap_or_default();
            let n = vals.get(1).cloned().unwrap_or_default();
            let nc = i.components();
            let dot: f32 = (0..nc).map(|k| i.get_component(k) * n.get_component(k)).sum();
            vec_to_val((0..nc).map(|k| i.get_component(k) - 2.0*dot*n.get_component(k)).collect())
        },
        "transpose" => {
            if let Some(Val::Mat4(m)) = vals.first() {
                Val::Mat4([m[0],m[4],m[8],m[12], m[1],m[5],m[9],m[13],
                           m[2],m[6],m[10],m[14], m[3],m[7],m[11],m[15]])
            } else { Val::default() }
        },
        _ => Val::Void,
    }
}

/// Expand a list of `Val` arguments into exactly `n` f32 scalars (used for
/// constructors like `vec4(v, 0.0, 1.0)` where `v` is a vec2).
fn expand_to_n(vals: &[Val], n: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(n);
    for v in vals {
        match v {
            Val::Float(f) => out.push(*f),
            Val::Int(i) => out.push(*i as f32),
            Val::Bool(b) => out.push(if *b { 1.0 } else { 0.0 }),
            Val::Vec2(arr) => { out.push(arr[0]); out.push(arr[1]); },
            Val::Vec3(arr) => { out.extend_from_slice(arr); },
            Val::Vec4(arr) => { out.extend_from_slice(arr); },
            Val::Mat4(arr) => { out.extend_from_slice(arr); },
            _ => out.push(0.0),
        }
        if out.len() >= n { break; }
    }
    // Broadcast single scalar
    if out.len() == 1 {
        let s = out[0];
        while out.len() < n { out.push(s); }
    }
    while out.len() < n { out.push(0.0); }
    out
}

/// Apply a unary f32→f32 function component-wise.
fn map1(vals: &[Val], f: impl Fn(f32) -> f32) -> Val {
    let v = vals.first().cloned().unwrap_or_default();
    map_component(&v, f)
}

fn map_component(v: &Val, f: impl Fn(f32) -> f32) -> Val {
    match v {
        Val::Float(x) => Val::Float(f(*x)),
        Val::Int(x) => Val::Float(f(*x as f32)),
        Val::Vec2([x, y]) => Val::Vec2([f(*x), f(*y)]),
        Val::Vec3([x, y, z]) => Val::Vec3([f(*x), f(*y), f(*z)]),
        Val::Vec4([x, y, z, w]) => Val::Vec4([f(*x), f(*y), f(*z), f(*w)]),
        _ => Val::Float(0.0),
    }
}

/// Apply a binary f32→f32 function component-wise (second arg broadcast to scalar).
fn map2(vals: &[Val], f: impl Fn(f32, f32) -> f32) -> Val {
    let a = vals.first().cloned().unwrap_or_default();
    let b_scalar = vals.get(1).map(|v| v.to_float()).unwrap_or(0.0);
    map_component(&a, |x| f(x, b_scalar))
}

fn vec_to_val(v: Vec<f32>) -> Val {
    match v.len() {
        0 => Val::Float(0.0),
        1 => Val::Float(v[0]),
        2 => Val::Vec2([v[0], v[1]]),
        3 => Val::Vec3([v[0], v[1], v[2]]),
        _ => Val::Vec4([v[0], v[1], v[2], v[3]]),
    }
}

// ─── Varying interpolation ──────────────────────────────────────────────────

/// Linearly interpolate a map of varying values given barycentric weights.
pub fn interp_varyings(
    va: &HashMap<String, Val>,
    vb: &HashMap<String, Val>,
    vc: &HashMap<String, Val>,
    wa: f32, wb: f32, wc: f32,
) -> HashMap<String, Val> {
    let mut out = HashMap::new();
    for key in va.keys() {
        let a = va.get(key).cloned().unwrap_or_default();
        let b = vb.get(key).cloned().unwrap_or_default();
        let c = vc.get(key).cloned().unwrap_or_default();
        let nc = a.components().max(b.components()).max(c.components()).max(1);
        let result: Vec<f32> = (0..nc)
            .map(|i| a.get_component(i)*wa + b.get_component(i)*wb + c.get_component(i)*wc)
            .collect();
        out.insert(key.clone(), vec_to_val(result));
    }
    out
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_uniforms() -> HashMap<String, Val> { HashMap::new() }

    fn run(src: &str, env: &mut ShaderEnv) {
        let s = parse(src);
        exec_main(&s, env);
    }

    #[test]
    fn flat_color_uniform() {
        let uniforms: HashMap<String, Val> = [
            ("u_color".into(), Val::Vec4([1.0, 0.0, 0.0, 1.0])),
        ].into();
        let mut env = ShaderEnv::new(&uniforms);
        run("uniform vec4 u_color; void main() { gl_FragColor = u_color; }", &mut env);
        assert_eq!(env.frag_color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn vec4_constructor_from_vec2() {
        let uniforms = empty_uniforms();
        let mut env = ShaderEnv::new(&uniforms);
        env.attributes.insert("a_pos".into(), Val::Vec2([0.5, -0.5]));
        run("attribute vec2 a_pos; void main() { gl_Position = vec4(a_pos, 0.0, 1.0); }", &mut env);
        assert_eq!(env.position, [0.5, -0.5, 0.0, 1.0]);
    }

    #[test]
    fn varying_passthrough() {
        // Vertex shader sets varying
        let uniforms = empty_uniforms();
        let mut env = ShaderEnv::new(&uniforms);
        env.attributes.insert("a_color".into(), Val::Vec4([0.1, 0.2, 0.3, 1.0]));
        run("attribute vec4 a_color; varying vec4 v_color; void main() { v_color = a_color; }", &mut env);
        let vc = env.varyings.get("v_color").cloned().unwrap_or_default();
        assert_eq!(vc.to_vec4(), [0.1, 0.2, 0.3, 1.0]);
    }

    #[test]
    fn mat4_vec4_multiply() {
        let identity: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        let uniforms: HashMap<String, Val> = [
            ("u_matrix".into(), Val::Mat4(identity)),
        ].into();
        let mut env = ShaderEnv::new(&uniforms);
        env.attributes.insert("a_pos".into(), Val::Vec2([0.3, 0.7]));
        run("uniform mat4 u_matrix; attribute vec2 a_pos; void main() { gl_Position = u_matrix * vec4(a_pos, 0.0, 1.0); }", &mut env);
        assert!((env.position[0] - 0.3).abs() < 1e-5);
        assert!((env.position[1] - 0.7).abs() < 1e-5);
    }

    #[test]
    fn swizzle_read_and_write() {
        let uniforms = empty_uniforms();
        let mut env = ShaderEnv::new(&uniforms);
        run("void main() { vec4 c = vec4(1.0, 0.0, 0.0, 1.0); c.gb = vec2(0.5, 0.5); gl_FragColor = c; }", &mut env);
        assert!((env.frag_color[0] - 1.0).abs() < 1e-5);
        assert!((env.frag_color[1] - 0.5).abs() < 1e-5);
        assert!((env.frag_color[2] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn if_else_branch() {
        let uniforms: HashMap<String, Val> = [
            ("u_flag".into(), Val::Float(1.0)),
        ].into();
        let mut env = ShaderEnv::new(&uniforms);
        run("uniform float u_flag; void main() { if (u_flag > 0.5) { gl_FragColor = vec4(1.0); } else { gl_FragColor = vec4(0.0); } }", &mut env);
        assert_eq!(env.frag_color, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn for_loop_accumulate() {
        let uniforms = empty_uniforms();
        let mut env = ShaderEnv::new(&uniforms);
        run("void main() { float s = 0.0; for (int i = 0; i < 4; i++) { s += 0.25; } gl_FragColor = vec4(s, 0.0, 0.0, 1.0); }", &mut env);
        assert!((env.frag_color[0] - 1.0).abs() < 0.01);
    }

    #[test]
    fn builtin_mix() {
        let uniforms = empty_uniforms();
        let mut env = ShaderEnv::new(&uniforms);
        run("void main() { gl_FragColor = mix(vec4(0.0), vec4(1.0), 0.5); }", &mut env);
        assert!((env.frag_color[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn builtin_clamp() {
        let uniforms = empty_uniforms();
        let mut env = ShaderEnv::new(&uniforms);
        run("void main() { float x = clamp(2.0, 0.0, 1.0); gl_FragColor = vec4(x, 0.0, 0.0, 1.0); }", &mut env);
        assert!((env.frag_color[0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn discard_sets_flag() {
        let uniforms = empty_uniforms();
        let mut env = ShaderEnv::new(&uniforms);
        // The assignment after discard must not execute.
        run("void main() { discard; gl_FragColor = vec4(0.7, 0.3, 0.1, 1.0); }", &mut env);
        assert!(env.discard);
        // frag_color stays at default (0,0,0,0); the vec4(0.7,...) was not reached.
        assert!((env.frag_color[0] - 0.7).abs() > 1e-5, "discard should have stopped execution");
    }

    #[test]
    fn interp_varyings_basic() {
        let mut va = HashMap::new(); va.insert("v".into(), Val::Vec4([0.0, 0.0, 0.0, 1.0]));
        let mut vb = HashMap::new(); vb.insert("v".into(), Val::Vec4([1.0, 0.0, 0.0, 1.0]));
        let mut vc = HashMap::new(); vc.insert("v".into(), Val::Vec4([0.0, 1.0, 0.0, 1.0]));
        let r = interp_varyings(&va, &vb, &vc, 1.0/3.0, 1.0/3.0, 1.0/3.0);
        let v = r["v"].to_vec4();
        assert!((v[0] - 1.0/3.0).abs() < 1e-5);
        assert!((v[1] - 1.0/3.0).abs() < 1e-5);
    }

    #[test]
    fn precision_qualifier_ignored() {
        let uniforms = empty_uniforms();
        let mut env = ShaderEnv::new(&uniforms);
        run("precision mediump float; void main() { gl_FragColor = vec4(0.5); }", &mut env);
        assert!((env.frag_color[0] - 0.5).abs() < 1e-5);
    }
}
