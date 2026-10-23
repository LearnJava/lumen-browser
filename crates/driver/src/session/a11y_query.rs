//! Поиск узлов дерева доступности по `AxQuery` (`query_a11y` / `query_a11y_all`).

use crate::{A11yNode, AxQuery};

/// Find first accessibility node matching query.
pub(super) fn find_a11y_node(node: &A11yNode, query: &AxQuery) -> Option<A11yNode> {
    if matches_query(node, query) {
        return Some(node.clone());
    }
    for child in &node.children {
        if let Some(result) = find_a11y_node(child, query) {
            return Some(result);
        }
    }
    None
}

/// Find all accessibility nodes matching query (depth-first).
pub(super) fn find_all_a11y_nodes(node: &A11yNode, query: &AxQuery, results: &mut Vec<A11yNode>) {
    if matches_query(node, query) {
        results.push(node.clone());
    }
    for child in &node.children {
        find_all_a11y_nodes(child, query, results);
    }
}

/// Check if accessibility node matches query criteria.
fn matches_query(node: &A11yNode, query: &AxQuery) -> bool {
    match query {
        AxQuery::Role { role, name } => {
            let role_matches = node.role.eq_ignore_ascii_case(role);
            if !role_matches {
                return false;
            }
            if let Some(name_filter) = name {
                node.name.to_lowercase().contains(&name_filter.to_lowercase())
            } else {
                true
            }
        }
        AxQuery::NameContains(name_filter) => {
            node.name.to_lowercase().contains(&name_filter.to_lowercase())
        }
    }
}
