//! Разбор значения `display` (CSS Display 3 §2, MathML Core, CSS Grid L3 `grid-lanes`).
//!
//! Грамматика: `[ <display-outside> || <display-inside> ] | <display-listitem> | <display-internal>
//! | <display-box> | <display-legacy>`. Многословные формы (`inline flow-root`, `list-item block`,
//! `block ruby`) сводятся к тем же вариантам [`Display`], что и короткие (`inline-block`,
//! `list-item`); то, что `Display` выразить не может (`inline list-item`, `ruby`, `math`,
//! `run-in …`, `ruby-text`), едет в `ComputedStyle::display_form` как каноническая строка —
//! по ней `getComputedStyle` отдаёт значение, а раскладка берёт ближайший `Display`.

use crate::style::values::typography::Display;

/// Результат разбора `display`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::style) struct ParsedDisplay {
    /// Вариант, по которому работает раскладка.
    pub display: Display,
    /// `grid-lanes` / `inline-grid-lanes`: контейнер остаётся `Grid`/`InlineGrid`, раскладка — masonry.
    pub lanes: bool,
    /// Каноническая строка, если её нельзя восстановить из `display` (см. модульный комментарий).
    pub form: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outside {
    Block,
    Inline,
    RunIn,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Inside {
    Flow,
    FlowRoot,
    Table,
    Flex,
    Grid,
    GridLanes,
    Ruby,
    Math,
}

fn outside_kw(w: &str) -> Option<Outside> {
    match w {
        "block" => Some(Outside::Block),
        "inline" => Some(Outside::Inline),
        "run-in" => Some(Outside::RunIn),
        _ => None,
    }
}

fn inside_kw(w: &str) -> Option<Inside> {
    match w {
        "flow" => Some(Inside::Flow),
        "flow-root" => Some(Inside::FlowRoot),
        "table" => Some(Inside::Table),
        "flex" => Some(Inside::Flex),
        "grid" => Some(Inside::Grid),
        "grid-lanes" => Some(Inside::GridLanes),
        "ruby" => Some(Inside::Ruby),
        "math" => Some(Inside::Math),
        _ => None,
    }
}

/// Односложные значения, не входящие в `<display-outside> || <display-inside>`.
fn single_keyword(w: &str) -> Option<ParsedDisplay> {
    let plain = |display| Some(ParsedDisplay { display, lanes: false, form: None });
    match w {
        "none" => plain(Display::None),
        "contents" => plain(Display::Contents),
        "inline-block" => plain(Display::InlineBlock),
        "inline-table" => plain(Display::InlineTable),
        "inline-flex" | "-webkit-inline-flex" => plain(Display::InlineFlex),
        "inline-grid" => plain(Display::InlineGrid),
        "-webkit-flex" => plain(Display::Flex),
        "-webkit-box" => plain(Display::WebkitBox),
        "-webkit-inline-box" => plain(Display::WebkitInlineBox),
        "inline-grid-lanes" => Some(ParsedDisplay { display: Display::InlineGrid, lanes: true, form: None }),
        "table-row-group" => plain(Display::TableRowGroup),
        "table-header-group" => plain(Display::TableHeaderGroup),
        "table-footer-group" => plain(Display::TableFooterGroup),
        "table-row" => plain(Display::TableRow),
        "table-column-group" => plain(Display::TableColumnGroup),
        "table-column" => plain(Display::TableColumn),
        "table-cell" => plain(Display::TableCell),
        "table-caption" => plain(Display::TableCaption),
        // CSS Ruby L1 §2: внутренние типы рубина. Раскладка рубина идёт по имени элемента,
        // поэтому для `Display` это инлайн, а слово сохраняется для computed value.
        "ruby-base" => Some(ParsedDisplay { display: Display::Inline, lanes: false, form: Some("ruby-base") }),
        "ruby-text" => Some(ParsedDisplay { display: Display::Inline, lanes: false, form: Some("ruby-text") }),
        "ruby-base-container" => {
            Some(ParsedDisplay { display: Display::Inline, lanes: false, form: Some("ruby-base-container") })
        }
        "ruby-text-container" => {
            Some(ParsedDisplay { display: Display::Inline, lanes: false, form: Some("ruby-text-container") })
        }
        _ => None,
    }
}

/// Разобрать значение `display`; `None` — значение недопустимо (декларация отбрасывается).
pub(in crate::style) fn parse_display(val: &str) -> Option<ParsedDisplay> {
    let lower = val.trim().to_ascii_lowercase();
    let words: Vec<&str> = lower.split_ascii_whitespace().collect();
    if words.is_empty() || words.len() > 3 {
        return None;
    }
    let mut outside = None;
    let mut inside = None;
    let mut list_item = false;
    for w in &words {
        if *w == "list-item" {
            if list_item {
                return None;
            }
            list_item = true;
        } else if let Some(o) = outside_kw(w) {
            if outside.replace(o).is_some() {
                return None;
            }
        } else if let Some(i) = inside_kw(w) {
            if inside.replace(i).is_some() {
                return None;
            }
        } else if words.len() == 1 {
            return single_keyword(w);
        } else {
            return None;
        }
    }
    // `<display-listitem>` допускает только `flow`/`flow-root` внутри.
    if list_item && !matches!(inside, None | Some(Inside::Flow) | Some(Inside::FlowRoot)) {
        return None;
    }
    let inside = inside.unwrap_or(Inside::Flow);
    // `ruby`/`math` без внешнего слова — инлайновые (`display: ruby` = `inline ruby`).
    let default_outside = if matches!(inside, Inside::Ruby | Inside::Math) { Outside::Inline } else { Outside::Block };
    let outside = outside.unwrap_or(default_outside);
    Some(resolve(outside, inside, list_item))
}

fn resolve(outside: Outside, inside: Inside, list_item: bool) -> ParsedDisplay {
    use Inside::{Flex, Flow, FlowRoot, Grid, GridLanes, Math, Ruby, Table};
    use Outside::{Block, Inline, RunIn};
    // (display, canonical-строка, если её не вывести из display).
    let (display, form): (Display, Option<&'static str>) = if list_item {
        match (outside, inside) {
            (Block, Flow) => (Display::ListItem, None),
            (Block, FlowRoot) => (Display::ListItem, Some("flow-root list-item")),
            (Inline, Flow) => (Display::ListItem, Some("inline list-item")),
            (Inline, FlowRoot) => (Display::ListItem, Some("inline flow-root list-item")),
            (RunIn, Flow) => (Display::ListItem, Some("run-in list-item")),
            (RunIn, _) => (Display::ListItem, Some("run-in flow-root list-item")),
            _ => (Display::ListItem, None),
        }
    } else {
        match (inside, outside) {
            (Flow, Block) => (Display::Block, None),
            (Flow, Inline) => (Display::Inline, None),
            (Flow, RunIn) => (Display::Inline, Some("run-in")),
            (FlowRoot, Block) => (Display::FlowRoot, None),
            (FlowRoot, Inline) => (Display::InlineBlock, None),
            (FlowRoot, RunIn) => (Display::InlineBlock, Some("run-in flow-root")),
            (Table, Block) => (Display::Table, None),
            (Table, Inline) => (Display::InlineTable, None),
            (Table, RunIn) => (Display::InlineTable, Some("run-in table")),
            (Flex, Block) => (Display::Flex, None),
            (Flex, Inline) => (Display::InlineFlex, None),
            (Flex, RunIn) => (Display::InlineFlex, Some("run-in flex")),
            (Grid | GridLanes, Block) => (Display::Grid, None),
            (Grid | GridLanes, Inline) => (Display::InlineGrid, None),
            (Grid | GridLanes, RunIn) => (Display::InlineGrid, Some("run-in grid")),
            (Ruby, Block) => (Display::Block, Some("block ruby")),
            (Ruby, Inline) => (Display::Inline, Some("ruby")),
            (Ruby, RunIn) => (Display::Inline, Some("run-in ruby")),
            (Math, Block) => (Display::Block, Some("block math")),
            (Math, Inline) => (Display::Inline, Some("math")),
            (Math, RunIn) => (Display::Inline, Some("run-in math")),
        }
    };
    ParsedDisplay { display, lanes: inside == GridLanes && !list_item, form }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Option<(Display, Option<&'static str>)> {
        parse_display(s).map(|d| (d.display, d.form))
    }

    #[test]
    fn two_word_forms_collapse_to_short_display() {
        assert_eq!(p("inline flow-root"), Some((Display::InlineBlock, None)));
        assert_eq!(p("flow-root inline"), Some((Display::InlineBlock, None)));
        assert_eq!(p("block flow-root"), Some((Display::FlowRoot, None)));
        assert_eq!(p("block flex"), Some((Display::Flex, None)));
        assert_eq!(p("inline grid"), Some((Display::InlineGrid, None)));
        assert_eq!(p("table block"), Some((Display::Table, None)));
        assert_eq!(p("flow"), Some((Display::Block, None)));
        assert_eq!(p("inline flow"), Some((Display::Inline, None)));
    }

    #[test]
    fn list_item_forms() {
        assert_eq!(p("list-item block"), Some((Display::ListItem, None)));
        assert_eq!(p("flow list-item block"), Some((Display::ListItem, None)));
        assert_eq!(p("inline list-item"), Some((Display::ListItem, Some("inline list-item"))));
        assert_eq!(p("list-item flow-root"), Some((Display::ListItem, Some("flow-root list-item"))));
        assert_eq!(
            p("list-item inline flow-root"),
            Some((Display::ListItem, Some("inline flow-root list-item")))
        );
        assert_eq!(p("list-item flex"), None);
        assert_eq!(p("list-item flow flow-root"), None);
    }

    #[test]
    fn ruby_math_run_in_keep_canonical_word() {
        assert_eq!(p("ruby"), Some((Display::Inline, Some("ruby"))));
        assert_eq!(p("inline ruby"), Some((Display::Inline, Some("ruby"))));
        assert_eq!(p("ruby block"), Some((Display::Block, Some("block ruby"))));
        assert_eq!(p("block math"), Some((Display::Block, Some("block math"))));
        assert_eq!(p("math"), Some((Display::Inline, Some("math"))));
        assert_eq!(p("ruby-text"), Some((Display::Inline, Some("ruby-text"))));
        assert_eq!(p("flow run-in"), Some((Display::Inline, Some("run-in"))));
        assert_eq!(p("run-in flex"), Some((Display::InlineFlex, Some("run-in flex"))));
    }

    #[test]
    fn grid_lanes_keeps_grid_display_and_flag() {
        let d = parse_display("inline grid-lanes").unwrap();
        assert_eq!((d.display, d.lanes), (Display::InlineGrid, true));
        let d = parse_display("grid-lanes").unwrap();
        assert_eq!((d.display, d.lanes), (Display::Grid, true));
        let d = parse_display("inline-grid-lanes").unwrap();
        assert_eq!((d.display, d.lanes), (Display::InlineGrid, true));
        assert!(!parse_display("grid").unwrap().lanes);
    }

    #[test]
    fn invalid_values_are_rejected() {
        for bad in ["", "bogus", "block block", "inline inline", "flex grid", "inline-block block", "block ruby-text",
            "table-cell inline", "list-item list-item", "block flow-root flex", "none block", "contents inline"]
        {
            assert_eq!(p(bad), None, "{bad:?}");
        }
    }
}
