//! Применение разобранных значений к `ComputedStyle`, вынесенное из
//! `style.rs` батчами SPLIT-ST6/ST8.
//!
//! `apply_declaration` — единственная точка, через которую декларация каскада
//! попадает в `ComputedStyle`. Батч SPLIT-ST8 разложил её `match prop` (369
//! меток) на четыре тематических помощника: одна функция на 3 000 строк не
//! помещается в потолок файла (2 000 строк, `scripts/check_file_sizes.py`), а
//! резать её по счёту строк — значит получить файлы, границы которых ничего не
//! значат. Порядок веток внутри `match` семантики не несёт: все 369 меток
//! уникальны, поэтому свойство обрабатывает ровно один помощник.
//!
//! Форма помощника (`-> bool`, `_ => return false`, завершающее `true`) выбрана
//! ради побайтового переноса тел веток. Единственная правка в них — 18 выходов
//! `return` → `return true`: в исходной функции `return` означал «декларация
//! разобрана, дальше делать нечего», и цепочка `if …() { return; }` ниже даёт
//! ровно это.

pub mod css_wide;
mod layout;
mod motion;
mod paint;
mod text;

use std::cell::RefCell;
use std::collections::HashMap;

use lumen_core::geom::Size;
use lumen_css_parser::Declaration;

use crate::style::math_value::substitute_numeric_math;
use crate::style::substitute::{
    contains_env_call, env_calls_well_formed, substitution_value_well_formed,
};
use crate::style::{
    expand_vars_and_env, parse_css_wide_keyword, ComputedStyle, CssWideKeyword, FontWeight,
};

use css_wide::{apply_all_shorthand, apply_css_wide_keyword};

const SLOT_LAYOUT: u8 = 0;
const SLOT_TEXT: u8 = 1;
const SLOT_PAINT: u8 = 2;
const SLOT_MOTION: u8 = 3;
const SLOT_UNKNOWN: u8 = 4;

thread_local! {
    static PROP_SLOT: RefCell<HashMap<String, u8>> = RefCell::new(HashMap::new());
}

fn prop_slot(prop: &str) -> Option<u8> {
    PROP_SLOT.with(|c| c.borrow().get(prop).copied())
}

fn set_prop_slot(prop: &str, slot: u8) {
    PROP_SLOT.with(|c| {
        let mut m = c.borrow_mut();
        if m.len() >= 8192 {
            m.clear();
        }
        m.insert(prop.to_string(), slot);
    });
}

/// Применить одну декларацию каскада к `style`.
///
/// Преамбула (custom properties, подстановка `var()`/`env()`, CSS-wide
/// keyword-ы) осталась байт в байт из `style.rs`; за ней идёт цепочка
/// тематических помощников, заменившая один `match prop` на 369 меток.
#[allow(clippy::too_many_arguments)]
pub(in crate::style) fn apply_declaration(
    style: &mut ComputedStyle,
    decl: &Declaration,
    em_basis: f32,
    viewport: Size,
    parent_font_weight: FontWeight,
    inherited: &ComputedStyle,
    ua_baseline: &ComputedStyle,
    is_quirks: bool,
    dark_mode: bool,
) {
    let prop = decl.property.as_str();

    // Custom properties обрабатываются в отдельном pass до этого момента
    // (см. compute_style). Здесь — игнорируем.
    if prop.starts_with("--") {
        return;
    }

    // CSS Variables L1 §3: подстановка `var(--name [, fallback])` на этапе
    // применения. Если value содержит `var(` — пробуем expand с текущей
    // картой custom_props. При неудаче (имя не найдено и нет fallback,
    // глубина рекурсии превышена, синтаксическая ошибка) декларация
    // считается отсутствующей (CSS Variables L1 §3.3 «invalid at computed
    // value time»). `expanded` живёт до конца функции, чтобы `val` остался
    // валидным `&str`.
    //
    // BUG-514: здесь различаются два исхода, которые раньше сливались в один
    // `return`. Кривой `env()` (`env(10px)`, `env(x, {)`) — ошибка разбора:
    // декларации как будто не было, предыдущая декларация того же свойства
    // остаётся. Правильный `env()`/`var()`, который не раскрылся (или дал
    // значение, которое свойство не принимает), — invalid at computed-value
    // time: свойство вычисляется как `unset` (CSS Variables L1 §3.3,
    // CSS Environment Variables L1 §3), а не наследует чужое значение.
    let expanded;
    // PERF-DECL-FLAGS: `has_env_maybe` is a conservative substring probe —
    // `contains_env_call` still decides behind it; `has_var` is the exact
    // `contains("var(")` predicate, computed once at parse time.
    let has_env = decl.has_env_maybe() && contains_env_call(&decl.value);
    let val: &str = if decl.has_var() || has_env {
        // Ошибка разбора (кривой `env()`/`var()`, bad-string, лишний `!`) —
        // декларации как будто не было.
        if !substitution_value_well_formed(&decl.value) || !env_calls_well_formed(&decl.value) {
            return;
        }
        // CSS Environment Variables L1: env() раскрывается ПОСЛЕ var(),
        // потому что custom property может содержать `env(...)` — порядок
        // зафиксирован в `expand_vars_and_env`, общей с pre-pass-ом font-size.
        let result = expand_vars_and_env(&decl.value, &style.custom_props, em_basis, viewport);
        // `revert-layer`/`revert-rule`, пришедшие из fallback-а, каскад здесь
        // не разрешает (он делает это до apply по литеральному значению);
        // исторически декларация пропускалась, оставляя значение предыдущего
        // слоя, что совпадает с ожидаемым результатом
        // (`css-variables/revert-layer-in-fallback.html`). Сохраняем это.
        if let Some(v) = &result {
            let t = v.trim();
            if t.eq_ignore_ascii_case("revert-layer") || t.eq_ignore_ascii_case("revert-rule") {
                return;
            }
        }
        // IACVT = `unset`: сбрасываем свойство до property-parse. Если
        // подстановка не удалась или парсер свойства отверг результат, сброс
        // и останется итогом; удачное значение ниже просто перезапишет его.
        apply_css_wide_keyword(style, prop, CssWideKeyword::Unset, inherited, ua_baseline);
        match result {
            Some(v) => {
                expanded = v;
                expanded.as_str()
            }
            None => return,
        }
    } else {
        decl.value.as_str()
    };

    // DEVX-8a: `val` is the value about to reach property-specific parsing —
    // `var()`/`env()` expansion above must have fully resolved it (`expand_vars`
    // loops until `find_var_open` finds none, or bails to `None` and returns
    // early above). A literal `var(` surviving to here means the expansion loop
    // has a bug, not that this declaration is "still using variables".
    debug_assert!(
        !val.contains("var("),
        "DEVX-8a: unresolved var() reached property parser: {prop}={val}"
    );

    // `font-size` целиком принадлежит pre-pass-у `apply_font_size` — включая
    // CSS-wide keyword-ы. BUG-731: раньше keyword-ветка ниже применяла
    // `font-size: inherit` ещё раз, уже в main-pass, и затирала размер,
    // который pre-pass взял из более поздней декларации `font`-shorthand
    // (`.a{font-size:inherit} .c{font:700 44px/1.2 X}` на одном элементе →
    // 16px вместо 44px). Значения-длины и так уходили в no-op-арм `"font-size"`
    // ниже, так что асимметричной была именно keyword-ветка.
    if prop == "font-size" {
        return;
    }

    // CSS Cascade L4 §7: CSS-wide keywords (inherit / initial / unset /
    // revert) применимы к любому свойству. Делается ДО property-specific
    // парсинга, чтобы не дублировать проверку в 30+ branch-ах.
    if let Some(kw) = parse_css_wide_keyword(val) {
        if prop == "all" {
            apply_all_shorthand(style, kw, inherited, ua_baseline);
        } else {
            apply_css_wide_keyword(style, prop, kw, inherited, ua_baseline);
        }
        return;
    }
    // `all` принимает только CSS-wide keyword-ы — иное значение невалидно.
    if prop == "all" {
        return;
    }
    // BUG-1430: math functions with a number / angle / time result (`calc(1s + 1s)`,
    // `abs(-3)`, `calc(pi)`) are evaluated to a literal before the property parser, which
    // has no notion of a typed `calc()`. A `cq*`-unit value is left as written: its number
    // depends on the container, which only the container pass knows (BUG-1664).
    let math_literal = (prop != "tab-size" || !crate::style::values::length::contains_cq_unit_text(val))
        .then(|| substitute_numeric_math(prop, val, em_basis, viewport))
        .flatten();
    let val: &str = math_literal.as_deref().unwrap_or(val);
    // `zoom` scales these after the cascade (`apply_zoom_to_lengths`), but an `em`
    // length is baked to px right here against the already-zoomed `font_size` —
    // resolve it against the unzoomed one so the later multiply applies the factor once.
    let em_basis = if baked_then_zoomed(prop) && style.effective_zoom > 0.0 {
        em_basis / style.effective_zoom
    } else {
        em_basis
    };
    // PERF-PROP-SLOT — каждый из четырёх помощников внутри — `match prop`
    // на сотни строковых меток. Без кэша КАЖДАЯ декларация платила до четырёх
    // промахов (~450 str-cmp на habr по `LUMEN_APPLY_CENSUS`). Множество
    // свойств группы детерминировано строкой `prop` — ветки матчат только
    // имя, не значение, — поэтому слот запоминается с первого применения.
    // Неизвестные свойства тоже кэшируются (SLOT_UNKNOWN): все четыре
    // матча промахнулись бы одинаково на каждом повторе.
    match prop_slot(prop) {
        Some(SLOT_LAYOUT) => {
            census::timed(prop, || {
                layout::apply_decl_layout(style, prop, val, em_basis, viewport, is_quirks).then_some(())
            });
            return;
        }
        Some(SLOT_TEXT) => {
            census::timed(prop, || {
                text::apply_decl_text(
                    style, prop, val, em_basis, viewport, parent_font_weight, inherited, is_quirks,
                )
                .then_some(())
            });
            return;
        }
        Some(SLOT_PAINT) => {
            census::timed(prop, || {
                paint::apply_decl_paint(
                    style, prop, val, em_basis, viewport, inherited, is_quirks, dark_mode,
                )
                .then_some(())
            });
            return;
        }
        Some(SLOT_MOTION) => {
            census::timed(prop, || {
                motion::apply_decl_motion(style, prop, val, em_basis, viewport, is_quirks)
                    .then_some(())
            });
            return;
        }
        Some(_) => return, // SLOT_UNKNOWN
        None => {}
    }

    if census::timed(prop, || {
        layout::apply_decl_layout(style, prop, val, em_basis, viewport, is_quirks).then_some(())
    })
    .is_some()
    {
        set_prop_slot(prop, SLOT_LAYOUT);
        return;
    }
    if census::timed(prop, || {
        text::apply_decl_text(
            style,
            prop,
            val,
            em_basis,
            viewport,
            parent_font_weight,
            inherited,
            is_quirks,
        )
        .then_some(())
    })
    .is_some()
    {
        set_prop_slot(prop, SLOT_TEXT);
        return;
    }
    if census::timed(prop, || {
        paint::apply_decl_paint(
            style,
            prop,
            val,
            em_basis,
            viewport,
            inherited,
            is_quirks,
            dark_mode,
        )
        .then_some(())
    })
    .is_some()
    {
        set_prop_slot(prop, SLOT_PAINT);
        return;
    }
    // Последнее звено цепочки: `false` — свойство никому не известно,
    // декларация игнорируется (исходный `_ => {}`).
    let handled = census::timed(prop, || {
        motion::apply_decl_motion(style, prop, val, em_basis, viewport, is_quirks).then_some(())
    });
    set_prop_slot(prop, if handled.is_some() { SLOT_MOTION } else { SLOT_UNKNOWN });
}

/// Properties whose lengths are resolved to px at parse time and then multiplied by
/// the element's `zoom` in `cascade::apply_zoom_to_lengths` (border/outline widths,
/// shadows, scroll-margin/padding). Logical and image variants are not scaled there.
fn baked_then_zoomed(prop: &str) -> bool {
    matches!(
        prop,
        "box-shadow" | "text-shadow" | "border-spacing" | "letter-spacing" | "word-spacing" | "text-decoration"
            | "text-decoration-thickness" | "text-underline-offset" | "-webkit-text-stroke"
            | "-webkit-text-stroke-width" | "border" | "border-width" | "border-top" | "border-right" | "border-bottom" | "border-left"
            | "border-top-width" | "border-right-width" | "border-bottom-width" | "border-left-width"
    ) || prop.starts_with("outline")
        || prop.starts_with("scroll-margin")
        || prop.starts_with("scroll-padding")
}

// ── PERF-APPLY-CENSUS ──────────────────────────────────────────────────────
// `LUMEN_APPLY_CENSUS=1` instrumentation: per-property call count + nanoseconds
// spent inside whichever dispatch module actually handled the declaration
// (unhandled declarations are counted separately as `rejected`). Exists to
// rank the top-(prop,value) pairs for the selective typed-value cache — the
// full `CSSValue` refactor is only worth it where parse time concentrates.
// Off by default; the gate is a `OnceLock` bool so the hot path pays one
// atomic-load-free check per declaration.
#[cfg(not(test))]
mod census {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;

    // Global (not thread-local): precompute_counters fans declarations out
    // across rayon workers, and a per-thread map would need N dumps merged by
    // hand. Mutex cost is irrelevant — the whole module is env-gated off.
    static PROPS: OnceLock<Mutex<HashMap<String, (u64, u128)>>> = OnceLock::new();
    static CALLS: AtomicU64 = AtomicU64::new(0);

    pub(super) fn on() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("LUMEN_APPLY_CENSUS").is_some())
    }

    /// Times `f`; on `Some(())` (handled) attributes the elapsed ns to `prop`.
    /// `None` means the module did not handle the property — try the next one.
    pub(super) fn timed<T>(prop: &str, f: impl FnOnce() -> Option<T>) -> Option<T> {
        if !on() {
            return f();
        }
        let t = Instant::now();
        let out = f();
        let ns = t.elapsed().as_nanos();
        if out.is_some() {
            let map = PROPS.get_or_init(|| Mutex::new(HashMap::new()));
            if let Ok(mut m) = map.lock() {
                let e = m.entry(prop.to_string()).or_insert((0, 0));
                e.0 += 1;
                e.1 += ns;
            }
        }
        if CALLS.fetch_add(1, Ordering::Relaxed) % 100_000 == 99_999 {
            dump();
        }
        out
    }

    fn dump() {
        let Some(map) = PROPS.get() else { return };
        let Ok(m) = map.lock() else { return };
        let mut rows: Vec<(&String, &(u64, u128))> = m.iter().collect();
        rows.sort_by_key(|(_, v)| std::cmp::Reverse(v.1));
        eprintln!("[apply-census] calls={} top by ns:", CALLS.load(Ordering::Relaxed));
        for (k, (calls, ns)) in rows.iter().take(40) {
            eprintln!("  {ns:>12} ns  {calls:>8} calls  {k}");
        }
    }
}

#[cfg(test)]
mod census {
    pub(super) fn timed<T>(_prop: &str, f: impl FnOnce() -> Option<T>) -> Option<T> {
        f()
    }
}
