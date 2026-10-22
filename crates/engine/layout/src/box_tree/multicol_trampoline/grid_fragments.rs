//! `emit_grid_fragments` — нарезка grid-контейнера по колонкам multicol
//! (CSS Fragmentation L3 §5). Перенесено из `box_tree/multicol_trampoline.rs`
//! целиком, без правок тела: файл перерос потолок размера
//! (docs/lint-policy.md §5.1).

use super::*;

/// CSS Fragmentation L3 §5 / CSS Gap Decorations L1 §6.2 — a grid container laid out in full
/// (the Measure pass) is cut into one fragment per column of the multicol container
/// (`column-fill: auto`, a definite column height). A break that falls inside a row gap drops
/// the rest of that gap: the next fragment starts at the following track; a break inside a track
/// splits it (the track continues at the top of the next fragment). Each fragment keeps the parts
/// of the children that fall in its window (`box_tree` leaves — cutting one repeats nothing) and
/// the row tracks clipped to the window (`SubgridTracks::fragment`), so the painter draws a row
/// gap only between two tracks of the same fragment (a gap split by the break, or the last
/// content before it, is suppressed) and runs the column gaps over the fragment's height.
/// Returns `false` (nothing changed) when the box has no row tracks to cut by, and the segment
/// falls back to the atomic path.
pub(super) fn emit_grid_fragments(frame: &mut Frame) -> bool {
    let seg_i = frame.seg_i;
    let i = frame.init.segments[seg_i].item_idxs[0];
    let Some(limit) = segment_limit(&frame.init) else { return false };
    let grid = &frame.init.work[i];
    // The tracks are measured from the content box's top; the windows below are in border-box
    // coordinates (the border is cut with the box: `box-decoration-break: slice`).
    let border_top = grid.style.border_top_width;
    let gy0 = grid.rect.y;
    // A wrapped column flex container has no row tracks: its block axis is the main axis of every
    // line, so the tracks are the block extents of its items merged across the lines.
    let col_flex = is_column_flex(grid);
    let Some(rows) = (if col_flex {
        Some(column_flex_tracks(grid, gy0))
    } else {
        grid.subgrid_tracks
            .as_ref()
            .and_then(|t| t.rows.as_ref())
            .map(|r| r.iter().map(|&(a, b)| (a + border_top, b + border_top)).collect::<Vec<_>>())
    }) else {
        return false;
    };
    // A column flex container whose items overlap along the block axis (two lines of different
    // item sizes) merges into one track: it is still cut, by its items' line boxes
    // (`flex/fragmentation/013`).
    if rows.len() < if col_flex { 1 } else { 2 } || (frame.outer_hs[0] - grid.rect.height).abs() > 0.01 || limit < 1.0 {
        return false;
    }
    // Items that overflow a definite container height (`height: 140px` under 154px of lines)
    // still belong to the last fragment: the cut runs to the end of the last track.
    let total_h = grid.rect.height.max(rows.last().map_or(0.0, |r| r.1));
    let n_cols = frame.init.n_cols as usize;
    let forced = forced_row_starts(grid, gy0, &rows);
    // A wrapped row flex container with one forced break is cut at the column height whatever the
    // fill, its later lines pulled up (see [`row_flex_forced_pull`]).
    let pulled = row_flex_forced_pull(grid, gy0, &rows, &forced, total_h, limit);
    let quirk = pulled.is_some();
    let (pulled_grid, natural) = match pulled {
        Some((g, n)) => (Some(g), Some(n)),
        None => (None, None),
    };
    let grid = pulled_grid.as_ref().unwrap_or(grid);
    let forced = if quirk { Vec::new() } else { forced };
    // The windows see the container as one track (no gap to drop at the cut); the rules and the
    // visible tracks use the lines' item extents.
    let win_rows = if quirk { vec![(0.0, total_h)] } else { rows.clone() };
    let rows = natural.unwrap_or(rows);
    // A forced break is placed only in a `column-fill: auto` container; a balanced one keeps the
    // atomic path (the balanced height would have to account for the forced columns).
    if !forced.is_empty() && frame.init.balance {
        return false;
    }
    // `column-fill: balance` shrinks the columns below the height limit when the content is
    // shorter than `limit x columns` (Multicol L1 §7.1: the smallest height that still fits every
    // fragment into `column-count` columns); only a container that fills them all is cut at the
    // limit. The balanced height starts at `total / columns` and grows until the cut (which drops
    // the row gaps at the breaks) needs no more than `column-count` columns.
    // A balanced column is never shorter than the tallest monolithic item (Chrome stretches the
    // columns to it, capped by the height limit).
    // In a column flex container the monolithic item pushes the next column in `column-fill: auto`
    // too (`flex/fragmentation/014`: the next column starts at its bottom edge).
    let mono: Vec<(f32, f32)> = if frame.init.balance || col_flex {
        grid.children
            .iter()
            .filter(|c| is_monolithic(c) && !matches!(c.kind, BoxKind::Skip))
            .map(|c| (c.rect.y - gy0, c.rect.y - gy0 + c.rect.height))
            .collect()
    } else {
        Vec::new()
    };
    let cut_at = if !quirk && frame.init.balance && total_h < limit * n_cols as f32 - 0.01 {
        let fits = |h: f32| grid_windows(grid, gy0, &win_rows, &forced, &mono, total_h, h).len() <= n_cols;
        if !fits(limit) {
            return false;
        }
        let mut tallest = mono.iter().map(|m| m.1 - m.0).fold(0.0f32, f32::max);
        // Items of a column flex line are not cut by the balancing: a column is never shorter than
        // the tallest of them; an item taller than the limit is cut anyway and lifts nothing
        // (`flex/fragmentation/005`, `020`).
        if col_flex {
            let item_h = grid
                .children
                .iter()
                .filter(|c| !matches!(c.kind, BoxKind::Skip) && !matches!(c.style.position, Position::Absolute | Position::Fixed))
                .map(|c| c.rect.height)
                .filter(|&h| h <= limit)
                .fold(0.0f32, f32::max);
            tallest = tallest.max(item_h);
        }
        // Chrome balances in `LayoutUnit`s (1/64 px): the height rounds up to the next one.
        let snap = |h: f32| ((h * 64.0).ceil() / 64.0).min(limit);
        let mut lo = (total_h / n_cols as f32).max(tallest).max(1.0).min(limit);
        if fits(lo) {
            if fits(snap(lo)) { snap(lo) } else { lo }
        } else {
            // Bisect between a height that does not fit and the limit that does.
            let mut hi = limit;
            for _ in 0..16 {
                let mid = (lo + hi) * 0.5;
                if fits(mid) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            if fits(snap(hi)) { snap(hi) } else { hi }
        }
    } else {
        limit
    };
    let mut windows = grid_windows(grid, gy0, &win_rows, &forced, &mono, total_h, cut_at);
    // A grid with a definite `height` is fragmented over all of it: the row gaps the breaks dropped
    // do not shorten the container, so empty fragments (the background and the rules) follow the
    // last window, as many as the height still needs (Edge: `height: 140px` over five 20px rows
    // and four 10px gaps gives seven 20px columns, `subgrid/fragmentation/013`). Only a grid with
    // a row subgrid is padded: without one the dropped gaps do shorten the container
    // (`grid/fragmentation/022`).
    if !col_flex
        && forced.is_empty()
        && grid.children.iter().any(|c| !matches!(c.kind, BoxKind::Skip) && super::grid_row_subgrid::is_row_subgrid(c))
        && !frame.init.balance
        && grid.style.height.as_ref().is_some_and(|h| !matches!(h, Length::Percent(_)))
    {
        let mut extra = grid.rect.height - windows.iter().map(|w| w.1 - w.0).sum::<f32>();
        let mut at = windows.last().map_or(total_h, |w| w.1);
        while extra > 0.01 {
            let len = extra.min(cut_at);
            windows.push((at, at + len, false));
            at += len;
            extra -= len;
        }
    }
    let all_line_gaps = flex_line_gaps(grid, gy0, &rows);
    let column_lines = column_flex_lines(grid, gy0);
    // Chrome fragments a wrapped column flex container line by line: an item whose first line box
    // the break would cut moves to the next column alone, the items of the other lines keep filling
    // theirs (`flex/fragmentation/013`: `Two` leaves the column at 37px, `Three` runs on to 47px). So
    // when the lines would break differently each gets windows of its own. A monolithic item
    // overflows the column for the whole container (`flex/fragmentation/014`), so such a container
    // keeps the common windows.
    let line_windows = column_lines.as_ref().filter(|l| l.len() > 1 && forced.is_empty() && mono.is_empty()).and_then(|lines| {
        let per: Vec<Vec<(f32, f32, bool)>> = lines
            .iter()
            .map(|l| {
                let mut sub = grid.clone();
                sub.children = l.members.iter().map(|&k| grid.children[k].clone()).collect();
                let line_rows = column_flex_tracks(&sub, gy0);
                let line_total = grid.rect.height.max(line_rows.last().map_or(0.0, |r| r.1));
                grid_windows(&sub, gy0, &line_rows, &[], &[], line_total, cut_at)
            })
            .collect();
        per.iter().any(|w| *w != per[0]).then_some(per)
    });
    let mut line_of: Vec<Option<usize>> = vec![None; grid.children.len()];
    if let (Some(_), Some(lines)) = (&line_windows, &column_lines) {
        for (li, l) in lines.iter().enumerate() {
            for &k in &l.members {
                line_of[k] = Some(li);
            }
        }
    }
    // The window of the flex line `line` in fragment `f`, from the container's top edge.
    let win = |line: Option<usize>, f: usize| -> Option<(f32, f32, bool)> {
        match (&line_windows, line) {
            (Some(lw), Some(l)) => lw[l].get(f).copied(),
            _ => windows.get(f).copied(),
        }
    };
    let n_frag = line_windows.as_ref().map_or(windows.len(), |lw| lw.iter().map(Vec::len).max().unwrap_or(0));
    // An item with children is cut between them, never through one: a break inside a line box or
    // a nested block would need real fragmentation of the item, so such a grid stays atomic. A
    // subgrid item that drops its own row gap at the top of a window has that much more room at the
    // bottom of it (`drop_at`), so a child that ends within it is not cut.
    let straddles = grid.children.iter().enumerate().any(|(ci, c)| {
        let windows = match (&line_windows, line_of[ci]) {
            (Some(lw), Some(l)) => &lw[l],
            _ => &windows,
        };
        !matches!(c.kind, BoxKind::Skip)
            && !matches!(c.style.position, Position::Absolute | Position::Fixed)
            && !is_monolithic(c)
            && c.children.iter().any(|k| {
                let (top, bot) = (k.rect.y - gy0, k.rect.y - gy0 + k.rect.height);
                !matches!(k.kind, BoxKind::Skip)
                    && !is_clippable_leaf(k)
                    && !nested_cuttable(k, gy0, windows)
                    && windows.iter().enumerate().any(|(f, &(ws, we, _))| {
                        let room = room_at(c, gy0, ws, we);
                        (top < we - 0.01 && bot > we + room + 0.01)
                            || (f > 0 && ws > windows[f - 1].1 + 0.01 && top < ws - 0.01 && bot > ws + 0.01)
                    })
            })
    });
    if straddles {
        return false;
    }
    let (col_w, col_gap, content_x, cur_y) =
        (frame.init.col_w, frame.init.col_gap, frame.init.content_x, frame.init.cur_y);
    let (gx, gy) = (grid.rect.x, grid.rect.y);
    let mut seg_extent = 0.0f32;
    let mut out = Vec::with_capacity(n_frag);
    for f in 0..n_frag {
        // The fragment spans the windows of every line that still has one; it is as tall as the
        // tallest of them.
        let present: Vec<(Option<usize>, f32, f32, bool)> = match &line_windows {
            Some(lw) => (0..lw.len()).filter_map(|l| win(Some(l), f).map(|w| (Some(l), w.0, w.1, w.2))).collect(),
            None => win(None, f).map(|w| (None, w.0, w.1, w.2)).into_iter().collect(),
        };
        let ws = present.iter().map(|w| w.1).fold(f32::INFINITY, f32::min);
        let we = present.iter().map(|w| w.2).fold(f32::NEG_INFINITY, f32::max);
        let col_x = content_x + f as f32 * (col_w + col_gap);
        let mut frag = grid.clone();
        frag.rect.x = col_x;
        frag.rect.y = cur_y;
        // A column that ends in a forced break keeps the full column height (the column gaps run
        // through it); every other fragment is as tall as its window.
        let mut frag_h = 0.0f32;
        for &(line, lws, lwe, forced_end) in &present {
            let mut h = if forced_end { (lwe - lws).max(cut_at) } else { lwe - lws };
            // In a wrapped column flex container the part of a gap that a break dropped still belongs
            // to the next fragment's lines: they reach to the end of the remaining block size, which
            // the column rules run along (`flex/fragmentation/006`: 100px, not the 90px of content).
            if col_flex && f > 0 && !forced_end {
                let prev = win(line, f - 1).map_or(lws, |w| w.1);
                let dropped = (lws - prev).max(0.0);
                h = (h + dropped).min(h.max(cut_at));
            }
            frag_h = frag_h.max(h);
        }
        frag.rect.height = frag_h;
        // A fragment keeps the border edge it owns: the top one the first, the bottom one the last.
        let (first, last) = (f == 0, f + 1 == n_frag);
        if (!first && frag.style.border_top_width > 0.0) || (!last && frag.style.border_bottom_width > 0.0) {
            let st = std::sync::Arc::make_mut(&mut frag.style);
            if !first {
                st.border_top_width = 0.0;
            }
            if !last {
                st.border_bottom_width = 0.0;
            }
        }
        let content_top = if first { border_top } else { 0.0 };
        // A window that starts at the leading edge of a row gap kept by a bridging item has no
        // gap to draw at its top: the track after it grows upwards over the gap
        // (`grid-gap-decorations-fragmentation-028`), and so do the items that start in it.
        let pulled_track = (!first && line_windows.is_none())
            .then(|| rows.windows(2).find(|w| (ws - w[0].1).abs() <= 0.01 && ws < w[1].0 - 0.01))
            .flatten()
            .map(|w| w[1].0);
        // When no item starts in that track (only spanners that began above the break run through
        // it) the gap is not absorbed there: the first later track an item starts in grows upwards
        // by the gap instead, the tracks between keep their place (Chrome,
        // `subgrid-gap-decorations-fragmentation-017`: the item below the cut subgrid starts 10px
        // above its track). The painted gaps keep using `pulled_track`.
        let late_track = pulled_track.filter(|_| !col_flex).and_then(|t| {
            let starts = |a: f32, b: f32| {
                grid.children.iter().any(|c| {
                    let y = c.rect.y - gy;
                    !matches!(c.kind, BoxKind::Skip)
                        && !matches!(c.style.position, Position::Absolute | Position::Fixed)
                        && y >= a - 0.01
                        && y < b - 0.01
                })
            };
            let k = rows.iter().position(|r| (r.0 - t).abs() <= 0.01)?;
            if starts(rows[k].0, rows[k].1) {
                return None;
            }
            rows[k + 1..].iter().find(|r| starts(r.0, r.1)).map(|r| (r.0, t - ws))
        });
        let pull = |y: f32| if pulled_track.is_some_and(|t| (y - t).abs() <= 0.01) { ws } else { y };
        let pull_item = |y: f32| match late_track {
            Some((t, d)) if (y - t).abs() <= 0.01 => y - d,
            _ => pull(y),
        };
        frag.children = grid
            .children
            .iter()
            .enumerate()
            .filter_map(|(ci, c)| {
                let flow = !matches!(c.kind, BoxKind::Skip)
                    && !matches!(c.style.position, Position::Absolute | Position::Fixed);
                if !flow {
                    return (f == 0).then(|| {
                        let mut k = c.clone();
                        super::shift_tree(&mut k, col_x - gx, cur_y - gy);
                        k
                    });
                }
                // The window of the item's own line; a line that has run out of windows holds nothing here.
                let (ws, we, _) = win(line_of[ci], f)?;
                let (top, bot) = (pull_item(c.rect.y - gy), c.rect.y - gy + c.rect.height);
                // A monolithic item is not cut: it stays whole in the fragment its top edge falls
                // in and overflows that column (`contain: size`).
                if is_monolithic(c) {
                    return (top >= ws - 0.01 && top < we - 0.01).then(|| {
                        let mut k = c.clone();
                        super::shift_tree(&mut k, col_x - gx, cur_y - gy - ws);
                        k
                    });
                }
                // A subgrid item that drops its own row gap at the top of the window reaches that
                // much further down (`drop_at`); the part it took there is not drawn again in the
                // next window.
                let room = if f > 0 { room_at(c, gy, ws, we) } else { 0.0 };
                let prev = if f > 0 { win(line_of[ci], f - 1) } else { None };
                let taken = prev.map_or(0.0, |(ps, pe, _)| pe + room_at(c, gy, ps, pe));
                let (lo, hi) = (top.max(ws), bot.min(we + room));
                // A zero-height item at the very start of a window belongs to it too; one that the
                // room of the window before it took whole is not drawn again.
                let inside = (hi > lo && bot > taken + 0.01) || (c.rect.height == 0.0 && top >= ws && top < we);
                inside.then(|| {
                    let mut k = c.clone();
                    k.rect.x = col_x + (c.rect.x - gx);
                    k.rect.y = cur_y + (lo - ws);
                    k.rect.height = (hi - lo).max(0.0);
                    // An item that is a grid itself (a subgrid spanning the container's rows) is cut
                    // like the container: its row tracks are clipped to this window, so the gaps a
                    // break splits are not painted (`subgrid-gap-decorations-fragmentation-001`).
                    // The part of one of the item's own row gaps that the window starts in is not
                    // drawn at its top: the first track moves up to the fragment's edge and the
                    // item is that much shorter (`subgrid-gap-decorations-fragmentation-015`). When a
                    // child of the item spans that gap (a spanner over several rows) the gap stays
                    // inside it: the first track grows upwards over the gap instead and the children
                    // that start in it grow with it (`subgrid-gap-decorations-fragmentation-021`).
                    let mut dropped = 0.0f32;
                    let mut grown = None;
                    if top < ws - 0.01 || bot > we + room + 0.01 {
                        let spans: Vec<(f32, f32)> = c
                            .children
                            .iter()
                            .filter(|g| !matches!(g.kind, BoxKind::Skip))
                            .map(|g| (g.rect.y - gy, g.rect.y - gy + g.rect.height))
                            .collect();
                        // A forced break inside the item's own rows (`nested_forced_row`): the gap before
                        // that row is not drawn in the fragment the row starts in, and the next
                        // fragment lays its gaps out from the row's start (`rule_shift`).
                        let nested = (!col_flex).then(|| nested_forced_row(c, gy)).flatten();
                        let cut_at = nested.filter(|&r| r > lo + 0.01 && r < hi - 0.01);
                        let shift = nested
                            .filter(|&r| f > 0 && r > windows[f - 1].0 + 0.01 && r < lo - 0.01)
                            .map_or(0.0, |r| lo - r);
                        let cut = c.subgrid_tracks.as_ref().and_then(|t| {
                            item_fragment_tracks(t, c.rect.y - gy + c.style.border_top_width, lo, hi, &spans, cut_at)
                        });
                        if let Some((mut tracks, d, g)) = cut {
                            dropped = d;
                            grown = g;
                            tracks.rule_shift = shift;
                            k.subgrid_tracks = Some(tracks);
                        }
                    }
                    k.rect.height = (k.rect.height - dropped).max(0.0);
                    // The children of the item that fall in this window, moved with it.
                    let (dx, dy) = (col_x - gx, cur_y - gy - ws);
                    k.children = c
                        .children
                        .iter()
                        .filter_map(|g| {
                            let (t, b) = (g.rect.y - gy, g.rect.y - gy + g.rect.height);
                            let mut up = if dropped > 0.0 && t >= lo + dropped - 0.01 { dropped } else { 0.0 };
                            // A child that starts in the track that grew over the gap grows with it.
                            let grow = grown.filter(|&(at, _)| (t - at).abs() <= 0.01).map_or(0.0, |(_, gap)| gap);
                            up += grow;
                            // A childless block (an empty subgrid cell) that a window edge runs
                            // through is cut like the item: each window keeps its part
                            // (`subgrid-gap-decorations-fragmentation-007`).
                            if nested_cuttable(g, gy, &windows) && (t < ws - 0.01 || b > we + room + 0.01) {
                                return nested_fragment(g, gy, ws, we + room, dx, dy - up);
                            }
                            if is_clippable_leaf(g) && (t < ws - 0.01 || b > we + room + 0.01) {
                                let (glo, ghi) = (t.max(ws), b.min(we + room));
                                return (ghi > glo).then(|| {
                                    let mut g = g.clone();
                                    super::shift_tree(&mut g, dx, dy - up);
                                    g.rect.y += glo - t;
                                    g.rect.height = ghi - glo;
                                    g
                                });
                            }
                            (matches!(g.kind, BoxKind::Skip) || (t >= ws.max(taken) - 0.01 && t < we + room - 0.01)).then(|| {
                                let mut g = g.clone();
                                super::shift_tree(&mut g, dx, dy - up);
                                g.rect.height += grow;
                                g
                            })
                        })
                        .collect();
                    k
                })
            })
            .collect();
        // The leftover of a row subgrid with a nested one (`subgrid_tail`) is its last fragment.
        if !col_flex && line_windows.is_none() {
            frag.children.extend(grid.children.iter().filter_map(|c| {
                let (last, len) = subgrid_tail(c, gy, &windows)?;
                (last + 1 == f).then(|| {
                    let mut k = c.clone();
                    k.rect.x = col_x + (c.rect.x - gx);
                    k.rect.y = cur_y;
                    k.rect.height = len.min(frag_h);
                    // The nested subgrids stay in it as empty boxes: they keep the item's column
                    // gaps they span from being ruled.
                    let (len, dx) = (k.rect.height, k.rect.x - c.rect.x);
                    k.children = c
                        .children
                        .iter()
                        .filter(|n| !matches!(n.kind, BoxKind::Skip) && super::grid_row_subgrid::is_row_subgrid(n))
                        .map(|n| {
                            let mut n = n.clone();
                            n.children.clear();
                            n.subgrid_tracks = None;
                            n.rect.x += dx;
                            n.rect.y = cur_y;
                            n.rect.height = len;
                            n
                        })
                        .collect();
                    k.subgrid_tracks = Some(Box::new(crate::subgrid::SubgridTracks {
                        cols: c.subgrid_tracks.as_ref().and_then(|t| t.cols.clone()),
                        rows: Some(vec![(0.0, k.rect.height)]),
                        fragment: true,
                        ..Default::default()
                    }));
                    k
                })
            }));
        }
        // A line that holds a monolithic item starting in this window reaches to the item's bottom
        // edge, past the window's (the item overflows the column).
        let mono_bottom = |a: f32, b: f32| {
            grid.children
                .iter()
                .filter(|c| is_monolithic(c) && !matches!(c.kind, BoxKind::Skip))
                .map(|c| (c.rect.y - gy, c.rect.y - gy + c.rect.height))
                .filter(|&(t, _)| t >= ws - 0.01 && t < we - 0.01 && t >= a - 0.01 && t < b - 0.01)
                .map(|(_, bt)| bt.min(b))
                .fold(0.0f32, f32::max)
        };
        let visible: Vec<(usize, (f32, f32))> = rows
            .iter()
            .enumerate()
            .filter_map(|(k, &(a, b))| {
                let (lo, hi) = (pull(a).max(ws), b.min(we));
                let hi = if hi > lo { hi.max(mono_bottom(a, b)) } else { hi };
                (hi > lo).then_some((k, (lo - ws - content_top, hi - ws - content_top)))
            })
            .collect();
        // The gap numbers of this fragment continue those of the fragments before it.
        let row_gap_base = if col_flex { None } else { visible.first().map(|v| (v.0, rows.len() - 1)) };
        let visible_idx: Vec<usize> = visible.iter().map(|v| v.0).collect();
        // A window past the last track holds none: its single empty track carries the rules.
        let clipped: Vec<(f32, f32)> =
            if visible.is_empty() && ws >= total_h - 0.01 { vec![(0.0, we - ws)] } else { visible.into_iter().map(|v| v.1).collect() };
        let mut cols = grid.subgrid_tracks.as_ref().and_then(|t| t.cols.clone());
        // A gap is kept when the item after it has a part in this window.
        let keeps = |lg: &LineGap, ws: f32, we: f32| {
            let (t, b) = (pull(lg.next_top), lg.next_bot);
            if lg.next_mono { t >= ws - 0.01 && t < we - 0.01 } else { b.min(we) > t.max(ws) }
        };
        // A column flex fragment keeps every line (its `cols`, the extent across the main axis) and
        // the block-axis gaps of each, from the fragment's top: one that starts in this window, the
        // item after it has a part of it (a gap that ends above the window is not drawn).
        let column_gaps = column_lines.as_ref().map(|lines| {
            cols = Some(lines.iter().map(|l| (l.lo - gx, l.hi - gx)).collect());
            lines
                .iter()
                .enumerate()
                .map(|(li, l)| {
                    // A line that has run out of windows has no gap in this fragment.
                    let Some((ws, we, _)) = win(line_windows.is_some().then_some(li), f) else { return Vec::new() };
                    l.gaps.iter().filter(|g| g.start >= ws - 0.01 && keeps(g, ws, we)).map(|g| (g.start - ws, g.end - ws)).collect()
                })
                .collect()
        });
        let line_gaps = column_gaps.or_else(|| all_line_gaps.as_ref().map(|g| {
            visible_idx
                .iter()
                .map(|&k| g[k].iter().filter(|lg| keeps(lg, ws, we)).map(|lg| (lg.start - gx, lg.end - gx)).collect())
                .collect()
        }));
        frag.subgrid_tracks = Some(Box::new(crate::subgrid::SubgridTracks {
            cols,
            rows: Some(clipped),
            fragment: true,
            row_gap_base,
            line_gaps,
            rule_shift: 0.0,
        }));
        seg_extent = seg_extent.max(frag_h);
        out.push(frag);
    }
    frame.init.out.extend(out);
    frame.init.consumed[i] = true;
    frame.init.cur_y += seg_extent;
    true
}
