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

use lumen_core::geom::Size;
use lumen_css_parser::Declaration;

use crate::style::substitute::{
    contains_env_call, env_calls_well_formed, substitution_value_well_formed,
};
use crate::style::{
    expand_vars_and_env, parse_css_wide_keyword, ComputedStyle, CssWideKeyword, FontWeight,
};

use css_wide::apply_css_wide_keyword;

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
    let has_env = contains_env_call(&decl.value);
    let val: &str = if decl.value.contains("var(") || has_env {
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
        apply_css_wide_keyword(style, prop, kw, inherited, ua_baseline);
        return;
    }
    if layout::apply_decl_layout(style, prop, val, em_basis, viewport, is_quirks) {
        return;
    }
    if text::apply_decl_text(
        style,
        prop,
        val,
        em_basis,
        viewport,
        parent_font_weight,
        inherited,
        is_quirks,
    ) {
        return;
    }
    if paint::apply_decl_paint(
        style,
        prop,
        val,
        em_basis,
        viewport,
        inherited,
        is_quirks,
        dark_mode,
    ) {
        return;
    }
    // Последнее звено цепочки вызывается без `if`: его `false` — это исходный
    // `_ => {}`, то есть «свойство неизвестно, декларация игнорируется».
    motion::apply_decl_motion(style, prop, val, em_basis, viewport, is_quirks);
}
