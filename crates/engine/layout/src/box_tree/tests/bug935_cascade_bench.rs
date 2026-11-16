//! BUG-935 срез 66 — стенд стоимости селекторного матчинга полного каскада.
//!
//! `#[ignore]`: ничего не проверяет, печатает мкс на элемент — сравнивать до/после правки
//! матчера на одной машине (`cargo test -p lumen-layout --profile dev-release
//! cascade_matching_cost -- --ignored --nocapture`). Страница сделана как BEM-вёрстка
//! реального сайта: ~2 000 элементов на глубине ~10, ~1 500 правил, из них больше
//! половины — с комбинаторами, у большинства подлежащее совпадает по классу, а предка нет.

use lumen_core::geom::Size;

fn page() -> (String, String) {
    const BLOCKS: usize = 60;
    let mut html = String::from("<html><body>");
    for k in 0..8 {
        html.push_str(&format!("<div class=\"wrap{k}\">"));
    }
    for i in 0..BLOCKS {
        html.push_str(&format!("<div class=\"b{i}\">"));
        for _ in 0..6 {
            html.push_str(&format!("<div class=\"b{i}__row\">"));
            for _ in 0..3 {
                html.push_str(&format!("<span class=\"b{i}__cell\"><a href=\"#\" class=\"link\">x</a></span>"));
            }
            html.push_str("</div>");
        }
        html.push_str("</div>");
    }
    for _ in 0..8 {
        html.push_str("</div>");
    }
    html.push_str("</body></html>");
    let mut css = String::new();
    for i in 0..BLOCKS {
        css.push_str(&format!(
            ".b{i}{{display:block;color:#111}}\n\
             .b{i}__row{{display:flex;margin:1px}}\n\
             .b{i}__cell{{padding:2px}}\n\
             .b{i} .b{i}__cell{{color:#222}}\n\
             .wrap1 .b{i} a{{color:#333}}\n\
             .b{i}__row:hover{{color:red}}\n\
             .b{i} > .b{i}__row .b{i}__cell:first-child{{margin:0}}\n\
             div .b{i}__cell{{font-weight:bold}}\n\
             .wrap0 .wrap3 .b{i}__row{{gap:3px}}\n\
             .b{i}__row .link{{text-decoration:none}}\n\
             .page .b{i} .link{{color:#444}}\n\
             .b{i}.is-open .b{i}__cell{{margin:4px}}\n\
             .b{i}__row + .b{i}__row{{margin-top:2px}}\n\
             .wrap5 .b{i}__row > .b{i}__cell{{border:0}}\n\
             .b{i}__row .b{i}__cell .link:hover{{color:blue}}\n\
             .x{i} .y{i} .z{i}{{color:green}}\n\
             .b{i}__cell.sel{{color:orange}}\n\
             .wrap2 .b{i}__cell{{line-height:1.2}}\n\
             .l{i} a{{color:#555}}\n\
             .m{i} span{{color:#666}}\n\
             .n{i} div{{color:#777}}\n\
             .o{i} .b{i}__row{{color:#888}}\n\
             .p{i} > div{{color:#999}}\n\
             .q{i} .link{{color:#aaa}}\n\
             .r{i}__row{{color:#bbb}}\n"
        ));
    }
    css.push_str("a{color:#00e}span{display:inline}div{display:block}body div div span a{color:#123}\n");
    (html, css)
}

/// BUG-1800 срез 9 — та же страница, что у [`cascade_matching_cost`], но с пробами,
/// вооружёнными честно (форма нагрузки pikabu/ebay): счётчики на псевдоэлементах
/// (`sheet_has_pseudo_counters` → `pseudo_probe=true`, проба `::before`/`::after` на
/// КАЖДОМ элементе) и `content: counters(...)` читающим их. Пробный заголовок
/// (`node_classes`/`candidates`) и снапшот стеков счётчиков на элемент — статья среза.
fn probed_page() -> (String, String) {
    let (mut html, mut css) = page();
    html = html.replace("<div class=\"b0\">", "<div class=\"b0 feed\">");
    css.push_str(
        "div::before{counter-increment:d}\n\
         .feed .b0__cell::after{content:counters(d, \".\")}\n\
         .feed .b0__row::after{content:counter(d)}\n",
    );
    (html, css)
}

/// Стенд: полный каскад через `walk` с активными пробами; лучший из 39 проходов,
/// `--profile dev-release` (`cargo test -p lumen-layout --profile dev-release
/// pseudo_probe_walk_cost -- --ignored --nocapture`).
#[test]
#[ignore = "стенд: печатает время, ничего не проверяет"]
fn pseudo_probe_walk_cost() {
    use crate::counters::{precompute_counters, take_cascade_stats};
    let (html, css) = probed_page();
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    let vp = Size::new(1280.0, 800.0);
    let flat = lumen_dom::build_flat_tree(&doc);
    let mut best = u64::MAX;
    let mut elements = 0;
    let mut probed = 0u32;
    for round in 0..40 {
        let _ = take_cascade_stats();
        let _ = precompute_counters(&doc, &sheet, vp, &flat, false);
        let s = take_cascade_stats();
        elements = s.recomputed;
        probed = s.candidates;
        if round > 0 {
            best = best.min(s.walk_ns);
        }
    }
    println!(
        "pseudo_probe_walk_cost: {} элементов, candidates {} за проход, лучший из 39 проходов {:.2} мс ({:.1} мкс/элемент)",
        elements,
        probed,
        best as f64 / 1e6,
        best as f64 / 1e3 / f64::from(elements.max(1)),
    );
}

/// PERF-INVALIDATION-S1 — та же BEM-страница, но меряется не полный каскад, а флаш:
/// одна запись класса на блоке `.b0` (`class="b0"` ↔ `class="b0 is-open"`, правило
/// `.b0.is-open .b0__cell` стилизует 18 внуков), 40 флашей подряд, каждый — от каскада
/// предыдущего. Печатает `candidates`, `match_calls`, `recomputed` и `walk_ns` на флаш —
/// ворота среза S1 («кандидатов в 3 раза меньше», «проход ≤ 50 % от HEAD»).
#[test]
#[ignore = "стенд: печатает время, ничего не проверяет"]
fn flush_walk_cost() {
    use crate::counters::{
        incremental_precompute_counters, precompute_counters, set_incremental_restyle, take_cascade_stats,
        ContentDirty, RestyleDelta,
    };
    use crate::style::{restyle_node_index, restyle_roots_for_node_changes, NodeChange};
    let (html, css) = page();
    let mut doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    let vp = Size::new(1280.0, 800.0);
    let flat = lumen_dom::build_flat_tree(&doc);
    // `.b0` — первый блок страницы; id у него нет, ищем по классу.
    let block = (0..doc.node_count() as u32)
        .map(lumen_dom::NodeId::from_raw)
        .find(|&n| doc.get(n).get_attr("class") == Some("b0"))
        .expect("block .b0");
    set_incremental_restyle(true);
    let mut map = precompute_counters(&doc, &sheet, vp, &flat, false);
    let (mut best, mut sum) = (u64::MAX, 0u64);
    let (mut cands, mut matches, mut recomputed) = (0u32, 0u32, 0u32);
    let (mut visited, mut skipped, mut confirmed) = (0u32, 0u32, 0u32);
    for round in 0..40 {
        let old = doc.get(block).get_attr("class").unwrap_or("").to_string();
        let new = if old.contains("is-open") { "b0".to_string() } else { "b0 is-open".to_string() };
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(block).data {
            for attr in attrs.iter_mut() {
                if attr.name.local == "class" {
                    attr.value = new.clone();
                }
            }
        }
        let index = restyle_node_index(&doc, &sheet);
        let roots = restyle_roots_for_node_changes(
            &doc,
            [(block, NodeChange::AttrFrom { name: "class", old: &old })],
            &index,
        );
        // Живой флаш пишет в content-запись узел, на котором была запись атрибута.
        let content: std::collections::HashSet<lumen_dom::NodeId> = [block].into_iter().collect();
        let delta = RestyleDelta {
            prev_styles: map.styles().clone(),
            dirty_roots: roots.deep,
            shallow_roots: roots.shallow,
            point_roots: roots.point,
            content_dirty: ContentDirty::Nodes(&content),
        };
        let _ = take_cascade_stats();
        let next = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, delta);
        let s = take_cascade_stats();
        if round > 0 {
            best = best.min(s.walk_ns);
            sum += s.walk_ns;
            cands += s.candidates;
            matches += s.match_calls;
            recomputed += s.recomputed;
            visited += s.visited;
            skipped += s.skipped_subtrees;
            confirmed += s.confirmed;
        }
        map = next;
    }
    set_incremental_restyle(false);
    let n = 39.0;
    println!(
        "flush_walk_cost: лучший флаш {:.2} мс, средний {:.2} мс; кандидатов {:.0}/флаш, matches {:.0}/флаш, recomputed {:.1}/флаш, visited {:.0}, skipped {:.0}, confirmed {:.0}",
        best as f64 / 1e6,
        sum as f64 / 1e6 / n,
        cands as f64 / n,
        matches as f64 / n,
        recomputed as f64 / n,
        visited as f64 / n,
        skipped as f64 / n,
        confirmed as f64 / n,
    );
}

#[test]
#[ignore = "стенд: печатает время, ничего не проверяет"]
fn cascade_matching_cost() {
    use crate::counters::{precompute_counters, take_cascade_stats};
    let (html, css) = page();
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    let vp = Size::new(1280.0, 800.0);
    let flat = lumen_dom::build_flat_tree(&doc);
    let mut best = u64::MAX;
    let mut elements = 0;
    for round in 0..40 {
        let _ = take_cascade_stats();
        let _ = precompute_counters(&doc, &sheet, vp, &flat, false);
        let s = take_cascade_stats();
        elements = s.recomputed;
        if round > 0 {
            best = best.min(s.walk_ns);
        }
    }
    println!(
        "cascade_matching_cost: {} элементов, {} правил, лучший из 39 проходов {:.2} мс ({:.1} мкс/элемент)",
        elements,
        sheet.rules.len(),
        best as f64 / 1e6,
        best as f64 / 1e3 / f64::from(elements.max(1)),
    );
}

/// Новый матчер даёт тот же ответ, что прежний, на каждой паре «элемент × селектор» страницы стенда
/// (включая `+`, `>`, потомков и `:first-child`); при `--ignored --nocapture` печатает ещё и
/// отношение времён обоих на одних и тех же парах, в одном процессе — шум машины сокращается.
#[test]
fn matches_complex_agrees_with_the_reference_and_is_not_slower() {
    use crate::style::matches_complex as new_impl;
    use crate::style::matches_complex_reference as old_impl;
    let (html, css) = page();
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&css);
    let elements: Vec<_> = (0..doc.node_count() as u32)
        .map(lumen_dom::NodeId::from_raw)
        .filter(|&n| matches!(doc.get(n).data, lumen_dom::NodeData::Element { .. }))
        .collect();
    let selectors: Vec<_> = sheet.rules.iter().flat_map(|r| r.selectors.iter()).collect();
    let (mut pairs, mut hits) = (0u64, 0u64);
    for &n in &elements {
        for sel in &selectors {
            let (a, b) = (new_impl(sel, &doc, n), old_impl(sel, &doc, n));
            assert_eq!(a, b, "{n:?}: {sel:?}");
            pairs += 1;
            hits += u64::from(a);
        }
    }
    assert!(hits > 100, "стенд должен что-то матчить, а не только отказывать: {hits}");
    if std::env::args().any(|a| a == "--nocapture") {
        let (mut best_new, mut best_old) = (u128::MAX, u128::MAX);
        for _ in 0..15 {
            let t = std::time::Instant::now();
            let mut k = 0usize;
            for &n in &elements {
                for sel in &selectors {
                    k += usize::from(new_impl(sel, &doc, n));
                }
            }
            best_new = best_new.min(t.elapsed().as_nanos());
            let t = std::time::Instant::now();
            for &n in &elements {
                for sel in &selectors {
                    k += usize::from(old_impl(sel, &doc, n));
                }
            }
            best_old = best_old.min(t.elapsed().as_nanos());
            std::hint::black_box(k);
        }
        println!(
            "matches_complex: {pairs} пар, {hits} совпадений; новый {:.1} мс, прежний {:.1} мс ({:.2}x)",
            best_new as f64 / 1e6,
            best_old as f64 / 1e6,
            best_old as f64 / best_new as f64,
        );
    }
}
