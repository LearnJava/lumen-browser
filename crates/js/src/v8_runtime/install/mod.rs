//! Секции `V8JsRuntime::install_dom`, вынесенные батчем SPLIT-JS6.
//!
//! `install_dom` — одна функция, тело которой лежит внутри замыкания
//! `self.run(…)`: там создаются `scope`/`ctx`/`store`, а дальше идут 42
//! секции-баннера с 231 площадкой `reg!`. Сюда уехала 41 секция; преамбула
//! замыкания и его хвост (вычисление шима, установка воркеров, возврат)
//! остались в `install_dom` — хвост не секция, он лишь стоит под последним
//! баннером.
//!
//! **Почему макрос переехал с контекстом в параметрах.** Локальный
//! `macro_rules! reg!` ссылался на `scope`/`ctx`/`store` как на свободные
//! идентификаторы, и работало это только потому, что объявлен он был внутри
//! той же функции: `macro_rules!` гигиеничен для локальных переменных, так что
//! модульный макрос искал бы `scope` в точке своего ОБЪЯВЛЕНИЯ. Обход через
//! порождающий макрос (передача доллара через `$d:tt`) ломается там же —
//! `error[E0425]: cannot find value 'scope' in this scope` … «not accessible
//! due to macro hygiene». Работающая форма — контекст параметрами: площадка
//! получила приставку `scope, ctx, store,`, тела ветвей не редактировались.

mod platform;
mod storage;

pub(super) use platform::*;
pub(super) use storage::*;

/// Регистрация натива под именем `$name` — та же синтаксическая форма площадки,
/// что была у локального `reg!` внутри `install_dom`, плюс приставка контекста.
///
/// Ветвей 16: арности 0–7 в двух формах возврата (`$body:expr` и
/// `-> $R:ty $body:block`), с необязательным ведущим `move`.
///
/// Имена в теле макроса разрешаются на ПЛОЩАДКЕ (`macro_rules!` не гигиеничен
/// для путей), поэтому файл секций обязан видеть `into_v8_fn0`…`into_v8_fn7` и
/// `register_v8_native` — их даёт `use super::super::*;`.
macro_rules! reg {
    // arity 0
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? || -> $R:ty $body:block) => {{
        let native = into_v8_fn0(move || -> $R { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? || $body:expr) => {{
        let native = into_v8_fn0(move || { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    // arity 1
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty| -> $R:ty $body:block) => {{
        let native = into_v8_fn1(move |$a: $A| -> $R { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty| $body:expr) => {{
        let native = into_v8_fn1(move |$a: $A| { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    // arity 2
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty| -> $R:ty $body:block) => {{
        let native = into_v8_fn2(move |$a: $A, $b: $B| -> $R { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty| $body:expr) => {{
        let native = into_v8_fn2(move |$a: $A, $b: $B| { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    // arity 3
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty| -> $R:ty $body:block) => {{
        let native = into_v8_fn3(move |$a: $A, $b: $B, $c: $C| -> $R { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty| $body:expr) => {{
        let native = into_v8_fn3(move |$a: $A, $b: $B, $c: $C| { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    // arity 4
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty| -> $R:ty $body:block) => {{
        let native = into_v8_fn4(move |$a: $A, $b: $B, $c: $C, $d: $D| -> $R { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty| $body:expr) => {{
        let native = into_v8_fn4(move |$a: $A, $b: $B, $c: $C, $d: $D| { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    // arity 5
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty| -> $R:ty $body:block) => {{
        let native = into_v8_fn5(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E| -> $R { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty| $body:expr) => {{
        let native = into_v8_fn5(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E| { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    // arity 6
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty, $g:ident: $G:ty| -> $R:ty $body:block) => {{
        let native = into_v8_fn6(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E, $g: $G| -> $R { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty, $g:ident: $G:ty| $body:expr) => {{
        let native = into_v8_fn6(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E, $g: $G| { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    // arity 7
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty, $g:ident: $G:ty, $h:ident: $H:ty| -> $R:ty $body:block) => {{
        let native = into_v8_fn7(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E, $g: $G, $h: $H| -> $R { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
    ($sc:expr, $cx:expr, $st:expr, $name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty, $g:ident: $G:ty, $h:ident: $H:ty| $body:expr) => {{
        let native = into_v8_fn7(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E, $g: $G, $h: $H| { $body });
        register_v8_native($sc, $cx, $st, $name, native)?;
    }};
}

pub(super) use reg;
