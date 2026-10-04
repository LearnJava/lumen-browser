//! Хвост перерасчёта chrome-документа: BUG-405 срез 49 (`predict_same` против
//! байтовой идентичности `chrome_dl`) и BUG-625 (шрифт измерения хрома).

use super::*;

// ── BUG-405 slice 49: does `predict_same` really predict `chrome_dl` byte identity ──

/// BUG-405 slice 48's census (`scripts/chrome_dl_repeat_census.py`, driven over
/// MCP) could only ever produce `predict=false`: `new_tab`/`navigate` are the
/// only automation events that reach `relayout_chrome_host`, and both always
/// touch `bind_model`'s content, so `touched.is_empty()` is never true in that
/// sample. The one shape that *would* hit `predict=true` in real use − two
/// consecutive `relayout_chrome_host` passes with the exact same hover target
/// (a mouse jittering inside a still-hovered button, or any call reached for
/// an unrelated reason while hover happens not to have moved) − needs a real
/// `CursorMoved` (`crates/shell/src/lumen/cursor_moved.rs`) or a real chrome
/// click (`dispatch_chrome_action`), neither reachable through MCP's
/// hit-test-free `click` (see CLAUDE.md's "hovered/active nid ... cannot be
/// exercised by any automation surface" gotcha).
///
/// `relayout_chrome_host` itself cannot be unit-tested directly either — it
/// early-returns without a real `self.renderer`, and building a `Lumen` (246
/// fields: JS runtime, network stack, tab manager, …) for one diagnostic test
/// is out of proportion to what this slice can safely reach. So this
/// reproduces the *identical* four-input formula
/// (`touched.is_empty() && interactive_stable && viewport_stable &&
/// forced_colors_stable`) and the *identical* building blocks
/// `relayout_chrome_host`/`cc12_bench_cycle` use
/// (`bind_model_tracked`/`set_interactive_state`/`layout_measured_hyp`/
/// `paint_ordered`/`hash_display_list`) at the doc/sheet level − the same
/// level every other `bug341_s*` gate in this file already tests at − instead
/// of going through `Lumen`.
#[test]
fn bug405_slice49_chrome_dl_predict_same_holds_for_a_steady_state_hover_repeat() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let model = cc12_bench_model("");
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR).expect("chrome preview must have #sidebar");

    // Cycle 0 (cold): establishes the bound model and gives cycle 1 a
    // predecessor `dl` to compare against, exactly like the first call to
    // `relayout_chrome_host` in a live session.
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    lumen_layout::set_interactive_state(Some(sidebar), None, None);
    let layout0 = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    let dl0 = paint_ordered(&layout0);
    let hash0 = lumen_paint::hash_display_list(&[], &dl0, 0.0, 0.0, 0, 0);
    lumen_layout::clear_interactive_state();

    // Cycle 1 reproduces exactly the shape срез 48's census could not reach:
    // `relayout_chrome_host` invoked again with the SAME hover target and an
    // unchanged model. `touched` stays empty (nothing in `model` changed
    // since cycle 0) and `new_interactive == chrome_prev_interactive` holds
    // because the hover target is identical − both are the real preconditions
    // `predict_same` checks, not stand-ins for them.
    let touched1 = lumen_chrome::bind_model_tracked(&mut doc, &model);
    lumen_layout::set_interactive_state(Some(sidebar), None, None);
    let layout1 = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    let dl1 = paint_ordered(&layout1);
    let hash1 = lumen_paint::hash_display_list(&[], &dl1, 0.0, 0.0, 0, 0);
    lumen_layout::clear_interactive_state();

    // `viewport_stable`/`forced_colors_stable` are trivially true here −
    // neither viewport nor Forced-Colors Mode is touched by this fixture,
    // which mirrors the real precondition: either one flipping already
    // forces `relayout_chrome_host`'s full-layout fallback, a separate path
    // this slice does not need to reproduce.
    let interactive_stable = true; // hover target identical across both cycles
    let predict_same = touched1.is_empty() && interactive_stable;
    assert!(
        predict_same,
        "reproduction must land on predict=true − the exact case срез 48's MCP census (0/55 \
             predict=true calls) could not reach; touched1={touched1:?}",
    );

    let actual_same = hash0 == hash1;
    assert!(
        actual_same,
        "BUG-405 п.85: predict_same=true but chrome_dl bytes differ (hash0={hash0} hash1={hash1}) \
             − this is the dangerous direction (predict=true, actual=false) a content_epoch \
             skip-path for chrome_dl would need to rule out on the ONE shape срез 48's census left \
             unmeasured. If this ever fails, the skip-path proposed in `bugs/BUG-405-FIXED.md`'s \
             'Остаток' would have shown stale chrome on screen for a plain hover-hold.",
    );
}

// -- BUG-405 srez 50: ChromeOverlayFrameCache reuse-or-build decision --

/// A cache HIT must return bytes identical to a fresh rebuild, and each of
/// the four key inputs (generation/host/viewport/caret) must, on its own,
/// force a MISS -- the correctness gate chrome_overlay_segment's doc comment
/// promises. Pure-function test, no Lumen/renderer needed (srez 49 found
/// that disproportionate for one gate).
#[test]
fn bug405_slice50_chrome_overlay_cache_hit_matches_fresh_build_and_key_changes_miss() {
    let chrome_dl = vec![lumen_paint::DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, 40.0, 20.0),
        color: lumen_layout::Color { r: 10, g: 20, b: 30, a: 255 },
    }];
    let host = Rect::new(200.0, 40.0, 800.0, 700.0);
    let (win_w, win_h) = (1024.0_f32, 768.0_f32);
    let caret = Some((
        Rect::new(300.0, 10.0, 2.0, 20.0),
        lumen_layout::Color { r: 0, g: 120, b: 220, a: 220 },
    ));

    // Cold: no cache yet -- must build fresh and hand back something to
    // remember.
    let (framed0, strips0, _digests0, cache0) =
        chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, true, None);
    let cache0 = cache0.expect("cold call must produce a cache to remember");

    // Same generation/host/viewport/caret, cache present -- must be a HIT:
    // no new cache (nothing changed to remember), bytes identical to cycle 0.
    let (framed1, strips1, _digests1, cache1) =
        chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, true, Some(&cache0));
    assert!(cache1.is_none(), "unchanged key must reuse the existing cache, not rebuild one");
    assert_eq!(framed1, framed0, "HIT must be byte-identical to the fresh build it reuses");
    assert_eq!(strips1, strips0);

    // Each key field, changed alone, must force a rebuild (a MISS) -- this
    // is what makes the cache SAFE: a stale entry is invalidated, not reused.
    let cases = [
        ("generation", 2, host, (win_w, win_h), caret),
        ("host", 1, Rect::new(210.0, 40.0, 800.0, 700.0), (win_w, win_h), caret),
        ("viewport", 1, host, (1025.0, 768.0), caret),
        ("caret", 1, host, (win_w, win_h), None),
    ];
    for (label, gen_, host2, vp2, caret2) in cases {
        let (_, _, _, new_cache) =
            chrome_overlay_segment(&chrome_dl, host2, vp2.0, vp2.1, caret2, gen_, true, Some(&cache0));
        assert!(
            new_cache.is_some(),
            "changing only `{label}` must miss the cache and rebuild -- a false HIT here would \
             show stale chrome pixels on screen",
        );
    }

    // cache_enabled=false must never consult (or update) the cache, even
    // when one that would otherwise match is passed in -- the
    // LUMEN_NO_CHROME_OVERLAY_CACHE=1 A/B lever's whole point.
    let (framed_disabled, _, _, cache_disabled) =
        chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, false, Some(&cache0));
    assert_eq!(framed_disabled, framed0, "disabled arm must still build the correct bytes");
    assert!(cache_disabled.is_none(), "disabled arm must not remember a cache either");
}

// -- BUG-405 срез 51: re-measure the net win at strips_used=4 --

/// Срез 50's "Остаток" promised a re-measurement on a multi-band layout
/// (sidebar + narrow window, all four overlay strips active) — the
/// `chrome_mix` multiplier its `bench-text-scroll.html` stand never
/// exercised (window maximized there → only the top strip is
/// non-degenerate, `strips_used=[1]`).
///
/// A live-window census for that layout turns out to be unreachable through
/// the existing MCP automation surface: `AutomationCommand::Click` resolves
/// its `Target` (`Point`/`NodeId`/`Selector`, `crates/shell/src/lumen/
/// automation.rs::resolve_automation_target`) purely against
/// `self.layout_box`/`self.layout_source` — the PAGE document — and then
/// calls `Lumen::handle_click_at` directly. That is a different path from a
/// real winit `MouseInput` event, which checks `self.point_over_chrome`
/// FIRST and dispatches to `chrome_hit_test`/`dispatch_chrome_action`
/// before ever reaching page hit-testing
/// (`crates/shell/src/app/window_event/mouse_input.rs`). No chrome control
/// — the vertical-tabs toggle (`Ctrl+B`, no chrome UI equivalent either),
/// `data-action="open-web-sidebar"`, `data-action="open-ai-sidebar"` — is
/// reachable from a census script driving the live window over MCP, and
/// none of these toggles persist across a fresh launch to be pre-seeded via
/// config. So this slice measures the same pure function directly instead
/// — no live window, GPU or MCP needed at all, same reasoning as срез 49's
/// "a whole `Lumen` for one diagnostic is disproportionate".
///
/// Host/window numbers are copied from `bug405_slice50_...`'s correctness
/// fixture, which already happens to leave a margin on all four sides
/// (`strips_used == 4`, asserted below) — that test just never timed
/// anything.
///
/// `#[ignore]`d like срез 50's sibling gates — run explicitly:
/// `cargo test -p lumen-shell --profile dev-release bug405_slice51 -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf gate (BUG-405 срез 51) — doc comment has the run command"]
fn bug405_slice51_chrome_overlay_cache_net_win_at_four_active_strips() {
    // ~130 commands − `build: chrome` census (срез 50) logged `cmds=130` at
    // strips_used=1 on the real chrome document, so this fixture's `chrome_dl`
    // matches that order of magnitude instead of the single-`FillRect` toy
    // срез 50's correctness test used (fine for byte-equality, too small for
    // a stable timing signal here).
    let chrome_dl: Vec<lumen_paint::DisplayCommand> = (0..130)
        .map(|i| lumen_paint::DisplayCommand::FillRect {
            rect: Rect::new(i as f32, 0.0, 40.0, 20.0),
            color: lumen_layout::Color { r: 10, g: 20, b: 30, a: 255 },
        })
        .collect();
    let host = Rect::new(200.0, 40.0, 800.0, 700.0);
    let (win_w, win_h) = (1024.0_f32, 768.0_f32);
    let caret = Some((
        Rect::new(300.0, 10.0, 2.0, 20.0),
        lumen_layout::Color { r: 0, g: 120, b: 220, a: 220 },
    ));

    let (_, strips_used, _, cache) =
        chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, true, None);
    assert_eq!(
        strips_used, 4,
        "fixture must exercise all four strips — the chrome_mix multiplier this slice measures",
    );
    let cache = cache.expect("cold build must produce a cache to remember");

    const WARMUP: usize = 20;
    const SAMPLES: usize = 500;

    let mut hit_stats = lumen_paint::FrameStats::new();
    let mut rebuild_stats = lumen_paint::FrameStats::new();
    // Interleaved HIT-then-rebuild each round (docs/perf-method.md) — both
    // arms see the same cache/allocator warmth instead of one racing first.
    for i in 0..WARMUP + SAMPLES {
        let t0 = std::time::Instant::now();
        let (framed, _, _, new_cache) =
            chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, true, Some(&cache));
        let hit_ms = t0.elapsed().as_secs_f32() * 1000.0;
        assert!(new_cache.is_none(), "must stay a HIT for the whole loop");
        std::hint::black_box(&framed);

        let t1 = std::time::Instant::now();
        let (framed2, strips2, _, _) =
            chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, false, None);
        let rebuild_ms = t1.elapsed().as_secs_f32() * 1000.0;
        assert_eq!(strips2, 4);
        std::hint::black_box(&framed2);

        if i >= WARMUP {
            hit_stats.record(hit_ms);
            rebuild_stats.record(rebuild_ms);
        }
    }

    let hit_summary = hit_stats.summary().expect("samples collected");
    let rebuild_summary = rebuild_stats.summary().expect("samples collected");
    eprintln!("{}", hit_summary.display_with("BUG405_S51_HIT_4STRIP"));
    eprintln!("{}", rebuild_summary.display_with("BUG405_S51_REBUILD_4STRIP"));
    let saved = (1.0 - hit_summary.min_ms / rebuild_summary.min_ms) * 100.0;
    // срез 50 measured -20% on the live stand at strips_used=1 (one strip's
    // worth of `chrome_dl` copied either way) — compare this number against
    // that baseline, not against 0%.
    eprintln!("cache-on saves {saved:.1}% at strips_used=4 (by min of {SAMPLES} interleaved samples)");
}

// -- BUG-405 срез 52: is strips_used=4 even reachable on the real chrome layout? --

/// Builds the real chrome `(host_rect, chrome_dl)` pair `relayout_chrome_host`
/// itself would produce — срез 49's level (no live `Lumen`/renderer needed):
/// the actual `(doc, sheet)` asset, a `ChromeModel` with every panel that can
/// widen a strip turned on, run through the same `layout_measured_hyp`/
/// `take_content_area` pair production uses, `dl` built AFTER pruning like
/// `relayout_chrome_host` does. Shared by both srez-52 tests below so the
/// timing gate does not duplicate the fixture-building the correctness gate
/// already covers.
fn bug405_slice52_real_chrome_overlay_fixture() -> (Rect, lumen_paint::DisplayList, (f32, f32)) {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);

    // Every panel that can widen a strip, turned on at once — the real-UI
    // analogue of срез 51's "all four margins" intent: vertical sidebar
    // (left), `#rightSidebar` (right). `#findBar`/`#downloadsPanel` are
    // turned on too, but CC-9's own doc comment on `relayout_chrome_host`
    // says both are salvaged out of `#contentArea` BEFORE `page_host_rect` is
    // computed, i.e. they overlay the content area rather than resizing it,
    // so they should not be able to add a strip even in principle — the
    // `strips_used` assertion below exists to confirm that reading holds,
    // not to assume it.
    let mut model = cc12_bench_model("");
    model.layout_vertical = true;
    model.sidebar_collapsed = false;
    model.right_sidebar.open = true;
    model.find.open = true;
    model.downloads_open = true;

    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);

    let content_area = doc
        .find_by_id(lumen_chrome::ids::CONTENT_AREA)
        .expect("chrome preview must have #contentArea");
    let (host_rect, _detached) = take_content_area(
        &mut layout,
        content_area,
        &[
            lumen_chrome::ids::FIND_BAR,
            lumen_chrome::ids::DOWNLOADS_PANEL,
            lumen_chrome::ids::CP_OVERLAY,
            lumen_chrome::ids::CERT_OVERLAY,
            lumen_chrome::ids::PRINT_OVERLAY,
        ],
        &doc,
    )
    .expect("#contentArea must have a box to prune");

    // Same order as `relayout_chrome_host`: `dl` is built from `layout`
    // AFTER `#contentArea` is pruned out of it — this is the real
    // `chrome_dl`, not срез 51's synthetic 130-`FillRect` stand-in.
    let chrome_dl = paint_ordered(&layout);
    (host_rect, chrome_dl, (viewport.width, viewport.height))
}

/// Срез 51's own remainder flagged this: it measured the `chrome_mix`
/// multiplier at `strips_used=4` on a HAND-PICKED `Rect` (copied from срез
/// 50's correctness fixture, which "just happens" to leave a margin on all
/// four sides) — not on any host rect the real chrome layout ever produces.
///
/// Reading `assets/chrome/chrome.html`'s own CSS answers this ahead of the
/// test running: `.body-row{flex:1}` (the flex row holding `#contentArea` +
/// `#rightSidebar`) fills 100% of the height left under `.toolbar`, and
/// nothing downstream of it is shorter than its container — there is no
/// element anywhere in the asset that could leave a gap between
/// `#contentArea`'s bottom edge and the window's. `strips_used` should
/// therefore cap at 3 (top toolbar + left vertical-sidebar + right
/// `#rightSidebar`), never reach 4 — срез 51's `chrome_mix` scenario names a
/// shape the real UI cannot produce.
#[test]
fn bug405_slice52_real_chrome_layout_caps_at_three_active_strips() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (_, strips_used, _, _) = chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, false, None);
    eprintln!(
        "BUG405_S52 host_rect={host_rect:?} viewport=({win_w}, {win_h}) strips_used={strips_used} \
         cmds={}",
        chrome_dl.len(),
    );
    assert_eq!(
        strips_used, 3,
        "the asset's CSS (`.body-row{{flex:1}}` leaves no vertical gap below #contentArea) predicts a \
         structural ceiling of 3 non-degenerate strips (top/left/right) on the real chrome layout, never \
         срез 51's 4 — if this fails, either the asset changed to add a bottom-shrinking element (a new \
         strip is genuinely reachable, срез 51's number is back in play) or this reasoning was wrong \
         (investigate before trusting either arm's percentage)",
    );
}

/// The net win at the real `strips_used` ceiling (срез 52's correctness gate
/// above measures it at 3, not срез 51's synthetic 4) — real `host_rect` and
/// real `chrome_dl` (actual command count from the real chrome layout, not
/// срез 51's 130-`FillRect` stand-in). `#[ignore]`d like срез 51's sibling
/// gate — run explicitly:
/// `cargo test -p lumen-shell --profile dev-release bug405_slice52 -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf gate (BUG-405 срез 52) — doc comment has the run command"]
fn bug405_slice52_chrome_overlay_cache_net_win_on_real_layout() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (_, strips_used, _, cache) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, None);
    let cache = cache.expect("cold build must produce a cache to remember");

    const WARMUP: usize = 20;
    const SAMPLES: usize = 500;

    let mut hit_stats = lumen_paint::FrameStats::new();
    let mut rebuild_stats = lumen_paint::FrameStats::new();
    // Same interleaved-per-round shape as срез 51 (docs/perf-method.md) — both
    // arms see the same cache/allocator warmth instead of one racing first.
    for i in 0..WARMUP + SAMPLES {
        let t0 = std::time::Instant::now();
        let (framed, _, _, new_cache) =
            chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, Some(&cache));
        let hit_ms = t0.elapsed().as_secs_f32() * 1000.0;
        assert!(new_cache.is_none(), "must stay a HIT for the whole loop");
        std::hint::black_box(&framed);

        let t1 = std::time::Instant::now();
        let (framed2, strips2, _, _) =
            chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, false, None);
        let rebuild_ms = t1.elapsed().as_secs_f32() * 1000.0;
        assert_eq!(strips2, strips_used);
        std::hint::black_box(&framed2);

        if i >= WARMUP {
            hit_stats.record(hit_ms);
            rebuild_stats.record(rebuild_ms);
        }
    }

    let hit_summary = hit_stats.summary().expect("samples collected");
    let rebuild_summary = rebuild_stats.summary().expect("samples collected");
    eprintln!("{}", hit_summary.display_with("BUG405_S52_HIT_REAL"));
    eprintln!("{}", rebuild_summary.display_with("BUG405_S52_REBUILD_REAL"));
    let saved = (1.0 - hit_summary.min_ms / rebuild_summary.min_ms) * 100.0;
    eprintln!(
        "cache-on saves {saved:.1}% at strips_used={strips_used} on the REAL chrome layout \
         (cmds={}, by min of {SAMPLES} interleaved samples)",
        chrome_dl.len(),
    );
}

// -- BUG-405 срез 55: how much of fold_overlay's cost is the chrome-segment rehash? --

/// Срез 54's census (`bugs/BUG-405-FIXED.md` "Остаток", вариант (б)) narrowed
/// п.85 to one concrete question: `fold_overlay` (`display_list.rs:1970`)
/// hashes EVERY overlay command every frame with `hash_one_command`,
/// including the `chrome_dl` segment a `ChromeOverlayFrameCache` HIT already
/// proves byte-identical to every earlier HIT since the cache was built — but
/// is that rehash actually a measurable share of `fold_overlay`'s own cost,
/// or (as срез 47's numbers suggest — "послекэша" 0.14→0.02мс after the
/// double-hash was deduplicated) too small already to justify the
/// `content_epoch`-style plumbing a real fix would need? Measured directly
/// (docs/perf-method.md: counter/identity, not wall-clock feel) — no engine
/// code touched, `fold_overlay` runs exactly as it stands on `main` today.
///
/// Fixture: срез 52's REAL chrome segment (cmds=292 chrome_dl → framed at
/// strips_used=3, not a synthetic stand-in) plus the real scrollbar overlay
/// (`scrollbar::build_scrollbar_overlay`, exactly 2 `FillRect`s) — the two
/// overlay sources срез 54's read of `redraw_requested.rs` confirmed are
/// actually present every frame on the hot scrolling path (the other 7
/// builders return an empty `Vec` on a plain page).
///
/// `cargo test -p lumen-shell --profile dev-release bug405_slice55 -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf gate (BUG-405 срез 55) — doc comment has the run command"]
fn bug405_slice55_fold_overlay_cost_is_mostly_chrome_segment() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (chrome_segment, strips_used, _, _) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, false, None);
    assert_eq!(strips_used, 3, "must match срез 52's real-layout ceiling");

    let scrollbar_cmds = scrollbar::build_scrollbar_overlay(400.0, 4000.0, win_w, win_h);
    assert_eq!(scrollbar_cmds.len(), 2, "срез 54's read: exactly track+thumb");

    let mut full_overlay = chrome_segment.clone();
    full_overlay.extend(scrollbar_cmds.iter().cloned());
    let chrome_len = chrome_segment.len();

    const WARMUP: usize = 20;
    const SAMPLES: usize = 500;
    let mut full_stats = lumen_paint::FrameStats::new();
    let mut tail_only_stats = lumen_paint::FrameStats::new();
    // Interleaved each round (docs/perf-method.md) — both arms see the same
    // allocator/cache warmth instead of one racing first.
    for i in 0..WARMUP + SAMPLES {
        let t0 = std::time::Instant::now();
        let digests_full = lumen_paint::display_list::fold_overlay(&full_overlay);
        let full_ms = t0.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_full);

        let t1 = std::time::Instant::now();
        // The best case a chrome-digest-reuse fix could reach: only the tail
        // (scrollbar) actually gets hashed, the chrome prefix's digests come
        // from `ChromeOverlayFrameCache` instead of `hash_one_command`.
        let digests_tail = lumen_paint::display_list::fold_overlay(&full_overlay[chrome_len..]);
        let tail_ms = t1.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_tail);

        if i >= WARMUP {
            full_stats.record(full_ms);
            tail_only_stats.record(tail_ms);
        }
    }

    let full_summary = full_stats.summary().expect("samples collected");
    let tail_summary = tail_only_stats.summary().expect("samples collected");
    eprintln!("{}", full_summary.display_with("BUG405_S55_FOLD_FULL"));
    eprintln!("{}", tail_summary.display_with("BUG405_S55_FOLD_TAIL_ONLY"));
    let attributable = (1.0 - tail_summary.min_ms / full_summary.min_ms) * 100.0;
    eprintln!(
        "chrome-segment rehash accounts for {attributable:.1}% of fold_overlay's cost \
         ({chrome_len} chrome cmds vs {} scrollbar cmds, min of {SAMPLES} interleaved samples, \
         full={:.4}ms tail={:.4}ms)",
        full_overlay.len() - chrome_len,
        full_summary.min_ms,
        tail_summary.min_ms,
    );
}

// -- BUG-405 срез 56: срез 55's fixture put chrome first — real order is scrollbar first --

/// Reading `redraw_requested.rs`'s Step 6 top-down (not just the caret/cache
/// doc comments срезы 50/54/55 already read) shows the common live-scrolling
/// case — no find-bar, no validation tooltip, no color/date picker, no
/// `<dialog>`, no view transition, no hint overlay, i.e. every block between
/// the chrome step and the scrollbar step is a conditional `if let`/`if` that
/// is false on a plain page — builds `overlay_buf` with exactly two
/// `Vec::append` calls:
///
/// 1. chrome step: `framed.append(&mut overlay_buf)` while `overlay_buf` is
///    still empty, then `overlay_buf = framed` — chrome only, so far.
/// 2. scrollbar step: `combined = scrollbar_cmds; combined.append(&mut
///    overlay_buf)`, then `overlay_buf = combined`.
///
/// `Vec::append(&mut other)` keeps `self`'s elements first and moves
/// `other`'s elements after them, so step 2 puts the **scrollbar first,
/// chrome second** — the reverse of срез 55's `full_overlay = chrome_segment;
/// extend(scrollbar_cmds)` fixture. Every later overlay builder in the same
/// function (tooltip/pickers/dialog/view-transition) uses the identical
/// `X.append(&mut overlay_buf); overlay_buf = X;` prepend shape, so any of
/// them firing that frame also lands in front of chrome — chrome's start
/// offset inside `overlay_buf` is not a fixed prefix length at all, it moves
/// with whichever of those builders ran, and even the bare scrollbar-only
/// frame this test measures puts chrome at offset 2, not 0.
///
/// This does not move срез 55's headline number (rehashing the ~292-command
/// chrome segment dominates `fold_overlay`'s cost regardless of which end of
/// the array it sits at — the command count `hash_one_command` walks is
/// invariant to order), but it does invalidate the "cache by prefix length"
/// framing срез 54 suggested: a real digest-reuse mechanism cannot ask the
/// `Renderer` to "reuse the first K digests", because the K commands at the
/// front are frequently NOT the cached chrome segment. It needs a
/// caller-declared `(start, len)` RANGE, recomputed every frame from the
/// actual composition (`scrollbar_cmds.len()` when the scrollbar drew, `0`
/// otherwise, before any prepending overlay builder ran) — a fixed prefix or
/// suffix slot is the wrong shape for this cache's key. No engine code
/// touched, `fold_overlay` runs exactly as it stands on `main` today.
///
/// `cargo test -p lumen-shell --profile dev-release bug405_slice56 -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf gate (BUG-405 срез 56) — doc comment has the run command"]
fn bug405_slice56_fold_overlay_cost_holds_with_real_command_order() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (chrome_segment, strips_used, _, _) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, false, None);
    assert_eq!(strips_used, 3, "must match срез 52's real-layout ceiling");

    let scrollbar_cmds = scrollbar::build_scrollbar_overlay(400.0, 4000.0, win_w, win_h);
    assert_eq!(scrollbar_cmds.len(), 2, "срез 54's read: exactly track+thumb");

    // Real `redraw_requested.rs` order on the common live-scrolling frame:
    // scrollbar (volatile, changes with `scroll_y` every frame) FIRST,
    // chrome (stable across a `chrome_layout_generation`) SECOND — the
    // reverse of срез 55's fixture.
    let mut full_overlay = scrollbar_cmds.clone();
    full_overlay.extend(chrome_segment.iter().cloned());
    let scrollbar_len = scrollbar_cmds.len();

    const WARMUP: usize = 20;
    const SAMPLES: usize = 500;
    let mut full_stats = lumen_paint::FrameStats::new();
    let mut volatile_only_stats = lumen_paint::FrameStats::new();
    // Interleaved each round (docs/perf-method.md) — both arms see the same
    // allocator/cache warmth instead of one racing first.
    for i in 0..WARMUP + SAMPLES {
        let t0 = std::time::Instant::now();
        let digests_full = lumen_paint::display_list::fold_overlay(&full_overlay);
        let full_ms = t0.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_full);

        let t1 = std::time::Instant::now();
        // Best case a range-aware chrome-digest-reuse fix could reach: only
        // the volatile prefix (scrollbar) gets hashed, chrome's digests are
        // assumed supplied by `ChromeOverlayFrameCache` at whatever offset it
        // actually starts at this frame.
        let digests_volatile =
            lumen_paint::display_list::fold_overlay(&full_overlay[..scrollbar_len]);
        let volatile_ms = t1.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_volatile);

        if i >= WARMUP {
            full_stats.record(full_ms);
            volatile_only_stats.record(volatile_ms);
        }
    }

    let full_summary = full_stats.summary().expect("samples collected");
    let volatile_summary = volatile_only_stats.summary().expect("samples collected");
    eprintln!("{}", full_summary.display_with("BUG405_S56_FOLD_FULL"));
    eprintln!("{}", volatile_summary.display_with("BUG405_S56_FOLD_VOLATILE_ONLY"));
    let attributable = (1.0 - volatile_summary.min_ms / full_summary.min_ms) * 100.0;
    eprintln!(
        "chrome-segment rehash accounts for {attributable:.1}% of fold_overlay's cost with the \
         REAL command order (scrollbar first, chrome second) — confirms срез 55's number under \
         the corrected fixture ({} chrome cmds vs {scrollbar_len} scrollbar cmds, min of \
         {SAMPLES} interleaved samples, full={:.4}ms volatile-only={:.4}ms)",
        chrome_segment.len(),
        full_summary.min_ms,
        volatile_summary.min_ms,
    );
}

// -- BUG-405 срез 57: thread ChromeOverlayFrameCache's digest into fold_overlay --

/// Срез 56 fixed the reuse mechanism's shape (a `(start, len)` range
/// recomputed from the actual composition, not a fixed prefix/suffix slot).
/// This slice implements it: `ChromeOverlayFrameCache` now also remembers
/// `fold_overlay(&framed)` (`chrome_ui.rs`'s `digests` field), and
/// `fold_overlay_with_reuse` (`lumen_paint::display_list`) hashes only the
/// commands OUTSIDE the declared range, splicing in the cached tail
/// unchanged.
///
/// Correctness gate: on the real command order (scrollbar first, chrome
/// second — срез 56), reusing the chrome segment's cached digest must give
/// BIT-IDENTICAL output to a full `fold_overlay` — a false hit here means a
/// wrong pixel comparison downstream (`overlay_cache_step`/the frame hash),
/// not just a slower frame.
#[test]
fn bug405_slice57_fold_overlay_with_reuse_matches_full_fold_on_real_order() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (chrome_segment, strips_used, digests0, cache) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, None);
    assert_eq!(strips_used, 3, "must match срез 52's real-layout ceiling");
    let cache = cache.expect("cold build must produce a cache to remember");

    // Second call, same key -- must be a HIT, and its digest must equal the
    // cold build's own fold (not just same length).
    let (chrome_segment2, _, chrome_digests, new_cache) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, Some(&cache));
    assert!(new_cache.is_none(), "unchanged key must stay a HIT");
    assert_eq!(chrome_segment2, chrome_segment, "HIT must reuse the exact same bytes");
    assert_eq!(chrome_digests, digests0, "HIT digest must equal the cold build's own fold");

    let scrollbar_cmds = scrollbar::build_scrollbar_overlay(400.0, 4000.0, win_w, win_h);
    let mut full_overlay = scrollbar_cmds.clone();
    full_overlay.extend(chrome_segment2.iter().cloned());
    let chrome_start = scrollbar_cmds.len();

    let expected = lumen_paint::display_list::fold_overlay(&full_overlay);
    let actual = lumen_paint::display_list::fold_overlay_with_reuse(
        &full_overlay,
        Some(&(chrome_start, chrome_digests)),
    );
    assert_eq!(
        actual, expected,
        "reused digest must be bit-identical to a full recompute -- a mismatch here would silently \
         feed a wrong per-command hash into overlay_cache_step/the frame hash, i.e. a false cache HIT \
         (wrong pixel forever, not just a slow frame)",
    );
}

/// A `(start, digests)` whose length does not fit `overlay.len()` -- a stale
/// hint from a shorter/longer buffer than the one it was computed for,
/// exactly what `overlay_len_after_prepend_phase` in `redraw_requested.rs`
/// guards against by construction, but this is the last line of defence
/// inside `fold_overlay_with_reuse` itself -- must fall back to a full
/// recompute, not panic or silently misalign.
#[test]
fn bug405_slice57_fold_overlay_with_reuse_falls_back_on_length_mismatch() {
    let overlay = vec![
        lumen_paint::DisplayCommand::FillRect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: lumen_layout::Color { r: 1, g: 2, b: 3, a: 255 },
        },
        lumen_paint::DisplayCommand::FillRect {
            rect: Rect::new(10.0, 0.0, 10.0, 10.0),
            color: lumen_layout::Color { r: 4, g: 5, b: 6, a: 255 },
        },
    ];
    let expected = lumen_paint::display_list::fold_overlay(&overlay);

    // Stale hint: claims a tail of 5 digests, buffer only has 2 commands.
    let stale = (0usize, vec![1u64, 2, 3, 4, 5]);
    let actual = lumen_paint::display_list::fold_overlay_with_reuse(&overlay, Some(&stale));
    assert_eq!(actual, expected, "length mismatch must fall back to a full recompute");
}

/// End-to-end perf gate: the actual win `fold_overlay_with_reuse` gives on
/// the real command order, chrome digest supplied the way
/// `redraw_requested.rs` now supplies it (a `ChromeOverlayFrameCache` HIT).
/// Comparable to срезы 55/56's headline number, but measuring the REAL
/// entry point instead of the "best case" `full_overlay[chrome_len..]`
/// slice those two used as a stand-in before this mechanism existed.
///
/// `cargo test -p lumen-shell --profile dev-release bug405_slice57 -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf gate (BUG-405 срез 57) — doc comment has the run command"]
fn bug405_slice57_fold_overlay_with_reuse_net_win_on_real_order() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (chrome_segment, strips_used, _, cache) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, None);
    assert_eq!(strips_used, 3, "must match срез 52's real-layout ceiling");
    let cache = cache.expect("cold build must produce a cache to remember");

    let scrollbar_cmds = scrollbar::build_scrollbar_overlay(400.0, 4000.0, win_w, win_h);
    let mut full_overlay = scrollbar_cmds.clone();
    full_overlay.extend(chrome_segment.iter().cloned());
    let chrome_start = scrollbar_cmds.len();

    const WARMUP: usize = 20;
    const SAMPLES: usize = 500;
    let mut full_stats = lumen_paint::FrameStats::new();
    let mut reuse_stats = lumen_paint::FrameStats::new();
    // Interleaved each round (docs/perf-method.md) — both arms see the same
    // allocator/cache warmth instead of one racing first.
    for i in 0..WARMUP + SAMPLES {
        let t0 = std::time::Instant::now();
        let digests_full = lumen_paint::display_list::fold_overlay(&full_overlay);
        let full_ms = t0.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_full);

        // The real entry point: a fresh HIT lookup (as `redraw_requested.rs`
        // does every frame) feeds its digest into `fold_overlay_with_reuse`.
        let t1 = std::time::Instant::now();
        let (_, _, chrome_digests, new_cache) =
            chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, Some(&cache));
        assert!(new_cache.is_none(), "must stay a HIT for the whole loop");
        let digests_reused = lumen_paint::display_list::fold_overlay_with_reuse(
            &full_overlay,
            Some(&(chrome_start, chrome_digests)),
        );
        let reuse_ms = t1.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_reused);

        if i >= WARMUP {
            full_stats.record(full_ms);
            reuse_stats.record(reuse_ms);
        }
    }

    let full_summary = full_stats.summary().expect("samples collected");
    let reuse_summary = reuse_stats.summary().expect("samples collected");
    eprintln!("{}", full_summary.display_with("BUG405_S57_FOLD_FULL"));
    eprintln!("{}", reuse_summary.display_with("BUG405_S57_FOLD_REUSE"));
    let saved = (1.0 - reuse_summary.min_ms / full_summary.min_ms) * 100.0;
    eprintln!(
        "chrome-digest reuse saves {saved:.1}% of fold_overlay's cost on the real entry point \
         ({} chrome cmds vs {} scrollbar cmds, min of {SAMPLES} interleaved samples, \
         full={:.4}ms reuse={:.4}ms)",
        chrome_segment.len(),
        scrollbar_cmds.len(),
        full_summary.min_ms,
        reuse_summary.min_ms,
    );
}

/// CC-18/BUG-1059: `#demoBar` (`position:fixed; left:18px; bottom:18px`)
/// gets a real, on-screen-sized layout box — the bug is in paint
/// compositing (`build_chrome_overlay_strips` clips it away because it sits
/// inside `chrome_page_host_rect` by design), not in layout. This pins the
/// half that already works, so a future regression in the OTHER half
/// doesn't get blamed on this one by a debugger re-deriving both from
/// scratch.
#[test]
fn bug1059_demo_bar_gets_a_correctly_positioned_layout_box() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1920.0, 1040.0);
    let model = lumen_chrome::ChromeModel::default();
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    let layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    let demo_bar = doc.find_by_id(lumen_chrome::ids::DEMO_BAR).expect("has #demoBar");
    let b = lumen_layout::find_box_by_node(&layout, demo_bar).expect("#demoBar must get a layout box");
    assert_eq!(
        b.style.display,
        lumen_layout::Display::Flex,
        "the default Card shape is a flex column"
    );
    assert!(b.rect.width > 0.0 && b.rect.height > 0.0, "box must have real size: {:?}", b.rect);
    assert!(
        b.rect.x >= 0.0
            && b.rect.y >= 0.0
            && b.rect.x + b.rect.width <= viewport.width
            && b.rect.y + b.rect.height <= viewport.height,
        "box must lie fully inside the viewport (bottom-left corner): {:?}",
        b.rect,
    );
}

/// BUG-1059 срез 6: the other half — `take_floating_panel` must actually
/// remove `#demoBar`'s box from the chrome tree (the fix's precondition:
/// `build_chrome_overlay_strips` can only skip painting something it never
/// receives) and the detached box must still paint real content on its own,
/// and `restore_floating_panel` must put it back exactly where it was — the
/// S22-shaped incremental-basis contract [`FloatingPanelDetachment`]'s doc
/// comment describes.
#[test]
fn bug1059_take_floating_panel_detaches_and_restores_demo_bar() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1920.0, 1040.0);
    let model = lumen_chrome::ChromeModel::default();
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);

    let demo_bar = doc.find_by_id(lumen_chrome::ids::DEMO_BAR).expect("has #demoBar");
    let before_rect = lumen_layout::find_box_by_node(&layout, demo_bar)
        .expect("#demoBar must get a layout box")
        .rect;

    let (rect, detached) =
        take_floating_panel(&mut layout, demo_bar, lumen_chrome::ids::DEMO_BAR).expect("#demoBar must be detachable");
    assert_eq!(rect, before_rect, "detach must report the tree's own rect");
    assert!(
        lumen_layout::find_box_by_node(&layout, demo_bar).is_none(),
        "#demoBar's box must be gone from the main tree after detach — this is exactly what keeps it \
         out of build_chrome_overlay_strips's clipped chrome_dl"
    );

    let floating_dl = paint_ordered(&detached.removed);
    assert!(!floating_dl.is_empty(), "the detached box must still paint real content standalone");

    assert!(
        restore_floating_panel(&mut layout, detached),
        "restore must succeed against the tree it was detached from"
    );
    let restored_rect = lumen_layout::find_box_by_node(&layout, demo_bar)
        .expect("#demoBar must be back in the tree after restore")
        .rect;
    assert_eq!(restored_rect, before_rect, "restore must put the box back at its original rect");
}

/// BUG-1059 срез 6: end-to-end shape of the actual fix — a chrome_dl built
/// AFTER detaching `#demoBar` (mirroring `relayout_chrome_host`'s real
/// order: prune `#contentArea`, then detach floating panels, then
/// `paint_ordered`) must not contain `#demoBar`'s own background fill,
/// while the standalone floating display list does. Without the fix both
/// would be in the same (clipped-away) `chrome_dl` and neither assertion
/// would distinguish this from the pre-fix behaviour, so the test checks
/// both sides of the split, not just one.
#[test]
fn bug1059_chrome_dl_excludes_demo_bar_after_detach_but_floating_dl_includes_it() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1920.0, 1040.0);
    let model = lumen_chrome::ChromeModel::default();
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);

    let content_area = doc
        .find_by_id(lumen_chrome::ids::CONTENT_AREA)
        .expect("chrome preview must have #contentArea");
    let _ = take_content_area(
        &mut layout,
        content_area,
        &[
            lumen_chrome::ids::FIND_BAR,
            lumen_chrome::ids::DOWNLOADS_PANEL,
            lumen_chrome::ids::CP_OVERLAY,
            lumen_chrome::ids::CERT_OVERLAY,
            lumen_chrome::ids::PRINT_OVERLAY,
        ],
        &doc,
    )
    .expect("#contentArea must have a box to prune");

    let demo_bar = doc.find_by_id(lumen_chrome::ids::DEMO_BAR).expect("has #demoBar");
    let (demo_rect, detached) =
        take_floating_panel(&mut layout, demo_bar, lumen_chrome::ids::DEMO_BAR).expect("#demoBar must be detachable");
    let floating_dl = paint_ordered(&detached.removed);
    let chrome_dl = paint_ordered(&layout);

    // `.demo-bar{background:#16161c; border-radius:14px}` — its own box fill
    // at exactly the rect the tree had it at.
    let demo_bar_fill = |dl: &lumen_paint::DisplayList| {
        dl.iter().any(|cmd| {
            matches!(
                cmd,
                lumen_paint::DisplayCommand::FillRect { rect, .. }
                | lumen_paint::DisplayCommand::FillRoundedRect { rect, .. }
                    if *rect == demo_rect
            )
        })
    };
    assert!(
        demo_bar_fill(&floating_dl),
        "the standalone floating display list must contain #demoBar's own box fill"
    );
    assert!(
        !demo_bar_fill(&chrome_dl),
        "chrome_dl (what build_chrome_overlay_strips clips around chrome_page_host_rect) must NOT \
         contain #demoBar's box fill any more — it was detached before paint_ordered ran"
    );
}

// ── BUG-625: хром меряется тем же шрифтом, которым рисуется ─────────────

/// `<kbd>Ctrl</kbd>` в `.demo-hint` объявлен `font-family: var(--font-mono)`
/// (`'JetBrains Mono', …`), а рендер коротит это имя на bundled JetBrains
/// Mono. Раньше `relayout_chrome_host` мерил весь хром голым `FontMeasurer`
/// (bundled Inter, `font-family` отбрасывается), и ширина фрагмента была
/// пропорциональной ширины Inter-а — надпись рисовалась одним шрифтом, а
/// размечалась другим.
#[test]
fn bug625_chrome_mono_text_measured_with_bundled_jetbrains_mono() {
    let (doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let viewport = Size::new(1280.0, 800.0);
    let measurer = chrome_measurer().expect("измеритель хрома");
    let layout = lumen_layout::layout_measured(&doc, &sheet, viewport, measurer);

    let frag = lumen_layout::collect_visible_text(&layout)
        .into_iter()
        .find(|f| f.text == "Ctrl")
        .expect("в хроме есть видимый фрагмент <kbd>Ctrl</kbd>");
    // `.demo-hint kbd{font-size:10px}`; JetBrains Mono — моноширинный, у всех
    // четырёх букв одна ширина.
    let mono = lumen_paint::MultiFontMeasurer::new(
        &lumen_font::Font::parse(lumen_paint::chrome_fonts::JETBRAINS_MONO_REGULAR).expect("bundled JetBrains Mono"),
    )
    .expect("метрики JetBrains Mono");
    let expected = 4.0 * lumen_layout::TextMeasurer::char_width(&mono, 'C', 10.0);
    assert!(
        (frag.rect.width - expected).abs() < 0.05,
        "ширина «Ctrl» {} ≠ 4 моно-ячейкам JetBrains Mono {expected}",
        frag.rect.width
    );

    // И это действительно не ширина Inter-а — иначе тест ничего не ловит.
    let inter = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter");
    let inter_m = lumen_paint::FontMeasurer::new(&inter).expect("метрики Inter");
    let inter_w: f32 = "Ctrl".chars().map(|c| lumen_layout::TextMeasurer::char_width(&inter_m, c, 10.0)).sum();
    assert!((inter_w - expected).abs() > 0.5, "Inter {inter_w} vs mono {expected}");
}

/// BUG-1261: after N incremental chrome cycles (a typed omnibox — the content that moves clean
/// icons around), every `svg_paint_matrix` must equal the one
/// a fresh full layout of the same document gives. `rect` alone is not enough: `<path>` icons
/// paint through the matrix's translation, so a drifted matrix is an icon painted off place.
#[test]
fn incremental_typing_cycles_keep_svg_paint_matrix_equal_to_a_full_layout() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let mut state = Cc12IncrementalState::default();
    let mut typed = String::new();
    for _ in 0..8 {
        typed.push('a');
        let model = cc12_bench_model(&typed);
        cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, None, &mut state);
    }
    let incr = state.prev_pristine_layout.take().expect("persisted tree");
    // Fresh full layout of the document in the same final state.
    lumen_layout::set_interactive_state(None, None, None);
    let (fresh, _) = lumen_layout::layout_measured_hyp_with_counters(&doc, &sheet, viewport, &measurer, &hyp, false);
    fn mats(b: &lumen_layout::LayoutBox, out: &mut Vec<(lumen_dom::NodeId, [f32; 6])>) {
        if let lumen_layout::BoxKind::SvgShape { svg_paint_matrix, .. } = &b.kind {
            out.push((b.node, svg_paint_matrix.matrix));
        }
        for c in &b.children {
            mats(c, out);
        }
    }
    let (mut a, mut b) = (Vec::new(), Vec::new());
    mats(&incr, &mut a);
    mats(&fresh, &mut b);
    assert_eq!(a.len(), b.len());
    let bad: Vec<_> = a.iter().zip(&b).filter(|(x, y)| x != y).take(5).collect();
    assert!(bad.is_empty(), "svg_paint_matrix drifted: {} of {}; first: {:?}", a.iter().zip(&b).filter(|(x, y)| x != y).count(), a.len(), bad);
}
