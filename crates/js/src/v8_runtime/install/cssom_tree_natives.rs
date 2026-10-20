//! CSSOM-10: нативы дерева CSSOM — общие для владельческих листов
//! (`document.styleSheets`, реестр `stylesheet_nodes`) и конструируемых
//! (`new CSSStyleSheet()`, реестр `ConstructedStylesheets`).
//!
//! Первые два аргумента каждого натива — `constructed: bool, idx: u32`:
//! какой реестр и позиция листа в нём. Правило адресуется `id` узла дерева
//! ([`lumen_css_parser::cssom_tree`]); `0` — корень листа. Дерево — единственный
//! авторитет для правок: после успеха лист пересобирает свои коллекции, а для
//! владельческого листа журнал получает [`CssomOp::TreeEdit`] — каскад
//! шелла подменяет текст листа на текст дерева (`sheet_sync::patched_cascade`).
//!
//! Ошибки — отрицательные коды (исключений через `reg!` нет): `-1`
//! `IndexSizeError`, `-2` `SyntaxError`, `-3` `HierarchyRequestError`,
//! `-4` узла нет.

use super::reg;
#[allow(unused_imports)]
use super::super::*;
use super::constructed_stylesheets::ConstructedStylesheets;
use lumen_css_parser::cssom_tree::{CssomTreeError, RuleKind};
use lumen_css_parser::{CssomOp, Stylesheet};

use super::super::sheet_sync::SheetSync;

fn code(e: CssomTreeError) -> i32 {
    match e {
        CssomTreeError::IndexSize => -1,
        CssomTreeError::Syntax => -2,
        CssomTreeError::HierarchyRequest => -3,
        CssomTreeError::NotFound => -4,
    }
}

fn parent_of(id: u32) -> Option<u32> {
    (id != 0).then_some(id)
}

/// Доступ к листу по `(constructed, idx)`.
#[derive(Clone)]
struct Sheets {
    sync: SheetSync,
    constructed: ConstructedStylesheets,
}

impl Sheets {
    fn read<R>(&self, constructed: bool, idx: u32, f: impl FnOnce(&Stylesheet) -> R) -> Option<R> {
        if constructed {
            let guard = self.constructed.lock().unwrap_or_else(|e| e.into_inner());
            guard.get(idx as usize).map(|e| f(&e.sheet))
        } else {
            let guard = self.sync.nodes.lock().unwrap_or_else(|e| e.into_inner());
            guard.get(idx as usize).map(|e| f(&e.sheet))
        }
    }

    /// Правка листа; для владельческого при `ok(&результат)` пишет в журнал.
    fn write<R>(
        &self,
        constructed: bool,
        idx: u32,
        f: impl FnOnce(&mut Stylesheet) -> R,
        ok: impl FnOnce(&R) -> bool,
    ) -> Option<R> {
        if constructed {
            let mut guard = self.constructed.lock().unwrap_or_else(|e| e.into_inner());
            let entry = guard.get_mut(idx as usize)?;
            Some(f(std::sync::Arc::make_mut(&mut entry.sheet)))
        } else {
            let (node, result) = {
                let mut guard = self.sync.nodes.lock().unwrap_or_else(|e| e.into_inner());
                let entry = guard.get_mut(idx as usize)?;
                (entry.node, f(std::sync::Arc::make_mut(&mut entry.sheet)))
            };
            if ok(&result) {
                self.sync.record(node, CssomOp::TreeEdit);
            }
            Some(result)
        }
    }
}

/// JSON одного правила для JS-обёртки.
fn rule_json(sheet: &Stylesheet, id: u32) -> Option<String> {
    let tree = sheet.cssom_tree();
    let node = tree.get(id)?;
    let parent = tree.parent_of(id).flatten().unwrap_or(0);
    let mut json = serde_json::json!({
        "id": id,
        "parent": parent,
        "cls": node.kind.js_class(),
        "prelude": node.prelude,
        "cssText": tree.css_text(id).unwrap_or_default(),
        "styleCssText": tree.decls_text(id).unwrap_or_default(),
        "childCount": node.visible_len(),
    });
    if node.kind == RuleKind::Mixin
        && let Some(idx) = sheet.mixin_cssom_index(id)
        && let Some(lumen_css_parser::CssomRuleRef::Mixin(m)) = sheet.cssom_rules().get(idx).copied()
    {
        json["name"] = serde_json::Value::String(m.name.clone());
        json["mixinIndex"] = serde_json::json!(idx);
    }
    Some(json.to_string())
}

pub(crate) fn install_cssom_tree_natives(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    sync: SheetSync,
    constructed: ConstructedStylesheets,
) -> JsResult<()> {
    let sheets = Sheets { sync, constructed };
    // Видимые дети правила `parent` (`0` — корень листа).
    {
        let sh = sheets.clone();
        reg!(scope, ctx, store, "_lumen_cssom_children", move |c: bool, idx: u32, parent: u32| -> Vec<u32> {
            sh.read(c, idx, |s| {
                s.cssom_tree()
                    .visible_children_of(parent_of(parent))
                    .map(|v| v.into_iter().map(|n| n.id).collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
        });
    }
    {
        let sh = sheets.clone();
        reg!(scope, ctx, store, "_lumen_cssom_rule_json", move |c: bool, idx: u32, id: u32| -> Option<String> {
            sh.read(c, idx, |s| rule_json(s, id)).flatten()
        });
    }
    {
        let sh = sheets.clone();
        reg!(scope, ctx, store, "_lumen_cssom_insert", move |c: bool, idx: u32, parent: u32, text: String, index: u32| -> i32 {
            let outcome = sh.write(
                c,
                idx,
                |s| s.cssom_insert_rule(parent_of(parent), &text, index as usize),
                |r| r.is_ok(),
            );
            match outcome {
                Some(Ok(at)) => at as i32,
                Some(Err(e)) => code(e),
                None => -4,
            }
        });
    }
    {
        let sh = sheets.clone();
        reg!(scope, ctx, store, "_lumen_cssom_delete", move |c: bool, idx: u32, parent: u32, index: u32| -> i32 {
            let outcome = sh.write(c, idx, |s| s.cssom_delete_rule(parent_of(parent), index as usize), |r| r.is_ok());
            match outcome {
                Some(Ok(())) => 0,
                Some(Err(e)) => code(e),
                None => -4,
            }
        });
    }
    // `.style = …` / `style.cssText = …` правила: заменяет его декларации.
    {
        let sh = sheets.clone();
        reg!(scope, ctx, store, "_lumen_cssom_set_style", move |c: bool, idx: u32, id: u32, text: String| -> i32 {
            let decls = lumen_css_parser::parse_inline_style(&text);
            let outcome = sh.write(c, idx, |s| s.cssom_set_decls(id, decls), |r| r.is_ok());
            match outcome {
                Some(Ok(())) => 0,
                Some(Err(e)) => code(e),
                None => -4,
            }
        });
    }
    // Сеттеры прелюдии (`selectorText`, `conditionText`, `name`, …):
    // `1` — применено, `0` — невалидный текст молча проигнорирован.
    {
        let sh = sheets.clone();
        reg!(scope, ctx, store, "_lumen_cssom_set_prelude", move |c: bool, idx: u32, id: u32, text: String| -> i32 {
            let outcome = sh.write(c, idx, |s| s.cssom_set_prelude(id, &text), |r| matches!(r, Ok(true)));
            match outcome {
                Some(Ok(true)) => 1,
                Some(Ok(false)) => 0,
                Some(Err(e)) => code(e),
                None => -4,
            }
        });
    }
    Ok(())
}
