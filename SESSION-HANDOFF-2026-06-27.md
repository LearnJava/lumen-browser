# Session Handoff — 2026-06-27 (Full)

<environment_details>
Current time: 2026-06-27T22:09:39+03:00
Working directory: D:\RustProjects\lumen-browser
Workspace root folder: D:\RustProjects\lumen-browser
</environment_details>

## Git state
- **Branch:** `main`
- **Upstream:** `origin/main` (last push: `bbd9802c`)
- **Last commit:** `bbd9802c` — fix BUG-255/256/257: navigation state serialization, key collision, GDI caching

## Recent commit history (10 newest)
1. `bbd9802c` — fix BUG-255/256/257: navigation state serialization, key collision, GDI caching
2. `15b8b908` — Влить p3-review-fix-252: BUG-251..254 — красные гейты + Rec.2020 OETF
3. `09df3bb1` — Починить BUG-251..254: красные clippy/test-гейты + знак Rec.2020 OETF
4. `02a351c3` — Влить review-followup-20260627: BUG-252..257 по ревью коммитов 26-27.06
5. `28c69b63` — review 2026-06-27: завести BUG-252..257 по итогам ревью вчера/сегодня
6. `510b0e6d` — ph3-color-management step 6: target-aware image decode pipeline
7. `72a7dfaa` — ph3-color-management Step 3: carry ColorSpace in GradientStop
8. `ff18c93a` — ph3-color-management Step 2: target ICC transform + ColorFloat::to_display
9. `65744d0e` — STATUS-P3: note ph3-color-management Step 1 + Step 3-8 blocker
10. `c486539c` — ph3-color-management Step 1: DisplayColorProfile trait + Windows GDI OS query

## Worktrees (registered)
| Path | Branch | Commit |
|------|--------|--------|
| `D:/RustProjects/lumen-browser` | `main` | `bbd9802c` |
| `D:/RustProjects/lumen-browser/.claude/worktrees/graphic-followup` | `graphic-followup-local` | `c6c6fc0f` |
| `D:/RustProjects/lumen-browser/.claude/worktrees/p1-laguna-140314` | `p1-laguna-t1-140314` | `37b8f185` |
| `D:/RustProjects/lumen-browser/.claude/worktrees/p1-laguna-143746` | `p1-laguna-t1-143746` | `7d47a81a` |
| `D:/RustProjects/lumen-browser/.claude/worktrees/ph3-bfcache` | `p1-ph3-bfcache` | `54ecd6c3` |
| `D:/RustProjects/lumen-browser/.claude/worktrees/ph3-navapi` | `p1-ph3-navapi` | `451ba19f` |

## Working tree status
- Modified (unstaged): `BUGS.md` (6 changes), `STATUS-P3.md` (67 changes — 56 insertions, 17 deletions)
- Untracked: `SESSION-HANDOFF-2026-06-27.md`
- No staged changes
- Source code: **clean** (all changes committed)

## What we did in this session

### Commits pushed
1. **`510b0e6d`** — ph3-color-management Step 6: target-aware image decode pipeline
   - `lumen-core/icc.rs`: added `cached_rgb_transform_to(profile_bytes, target)` with new `rgb_targeted` cache keyed by `(profile_bytes, ColorSpace)`
   - `lumen-image/src/lib.rs`: added `decode_to(bytes, target)` public API; `color_manage_in_place` now target-aware using ICC matrix-shaper or per-pixel converter; added `p3_to_rec2020_pixel`, `rec2020_to_p3_pixel`, `rec2020_gamma_encode`, and linear P3↔Rec2020 matrices; renamed `convert_packed_to_srgb` → `convert_packed`
   - `shell/src/main.rs`: threaded `self.target_color_space()` through `decode_image`, `fetch_and_decode_images`, `fetch_and_decode_background_images`, `parse_and_layout`, `render_bytes`, and streaming `apply_loaded_page` closure
   - Driver: updated `new_headless()` callers to pass `lumen_core::ColorSpace::Srgb`
   - All tests pass: lumen-core 278, lumen-image 152, lumen-paint 813

2. **`bbd9802c`** — fix BUG-255/256/257: navigation state serialization, key collision, GDI caching
   - **BUG-255:** `commit_nav_state` now passes navigation state JSON as a quoted string literal via `serde_json::to_string`; state fields parsed back through `serde_json::from_str` — fixes `[object Object]` coercion in JS
   - **BUG-256:** Added `Lumen::current_nav_key: String`; history entries keep their existing `nav_key` when moved between stacks; only new pages get fresh key — eliminates `traverseTo(key)` collision
   - **BUG-257:** Rewrote `PlatformDisplayColorProfile` to use `std::sync::OnceLock<ColorSpace>`; removed incorrect early-return on `GetICMProfileA == 0` (NULL buffer returns FALSE per MSDN but still fills `buf_size`); GDI query now runs exactly once
   - All tests pass: lumen-core 278, lumen-image 152, lumen-paint 813, lumen-layout 3011, lumen-js 2324

## Test run results
| Crate | Result |
|-------|--------|
| lumen-core | 278 passed |
| lumen-image | 152 passed |
| lumen-paint | 813 passed |
| lumen-layout (lib doctests) | 3011 passed |
| lumen-layout (snapshot_tests) | 36 passed |
| lumen-layout (svg_layout) | 33 passed |
| lumen-js (lib) | 2324 passed |
| lumen-css-parser | all checks pass |
| lumen-driver (--test all) | 111 passed, 2 pre-existing failures |

### Pre-existing failures (not caused by this session's changes)
- **BUG-250:** `cases::test_04_color_alpha` — font baseline shift (sw[1,0] at y=124 vs expected y=125)
- **BUG-247:** `cases::test_47_svg_basic` — SVG AA edge case (980x140 at y=530.569 vs expected y=532.58)

## Outstanding issues (OPEN in BUGS.md)
| ID | Severity | Owner | Summary |
|----|----------|-------|---------|
| BUG-250 | P1 | paint/font | Font baseline shift broke TEST-02/04 (0.68%) and TEST-56 (1.83%). Root cause: FontMeasurer change from hardcoded 0.8/0.2 to OS/2 sTypoAscent/Descender ratios caused fractional pixel shifts. |
| BUG-247 | P2 | svg | SVG basic test AA edge case (test_47_svg_basic — 2px Y offset) |
| BUG-249 | P2 | paint | border-radius × overflow interaction (test_101) |

## Feature status (verified via tests)
- **P3-color-management (ph3):** Steps 1–7 complete. BUG-252/253/255/256/257 fixed.
- **P3-colormix:** `color_mix.rs` (12 interpolation spaces) + `parse_color_mix` wired in style.rs; 30 tests pass.
- **P3-navapi:** Shell history stacks + JS Navigation singleton + BUG-255/256 fixes; navigate_to/back/forward/navigate_by/replaceState/pushState fully wired.
- **P3-bfcache:** HTML-snapshot store/retrieve + scroll restore + pageshow persisted flag (JS heap freeze stub pending).
- **P3-varfonts:** BUG-109 fixed; vector outlines render path in femtovg + wgpu; variation axes parsed.
- **P3-regprop:** @property parsing + syntax validation + inherits/initial-value inheritance; 30+ tests pass.
- **P3-subgrid:** Grid L2 subgrid columns/rows inherited track sizes + `collect_subgrid_items`; 9 tests pass.
- **P3-has:** `:has()` selector parsing + cascade matching; 86 tests pass.
- **P3-nesting:** CSS Nesting L1 parser (explicit `&` + implicit combinators + nested at-rules); 17 parser tests + cascade tests pass.
- **P3-textwrap:** `text-wrap-mode` / `text-wrap-style` / `text-wrap` shorthand parsed + inherited; balance widening/narrowing tests pass (5 tests).
- **P3-multicol:** column-count / column-width / column-gap / column-fill balance/auto / column-span all; 9 tests pass + 6 column_rule paint tests.
- **P3-resizeobs:** ResizeObserver JS singleton + `_lumen_deliver_resize_observers` + border-box-size entries; tests pass.
- **P3-intersectobs2:** IntersectionObserver v2 (threshold, rootMargin, unobserve, lazy-image integration); 10 tests pass.
- **P3-streams:** WritableStream + sink/pipeThrough + backpressure; 59 stream tests pass.
- **P3-webcrypto:** SubtleCrypto HMAC + ECDSA + AES-GCM + import/export JWK/PKCS8; 16 tests pass.
- **P3-weblocks:** LockManager + query/request/ifAvailable; 6 tests pass.
- **P3-broadcast:** BroadcastChannel name-isolation + message delivery + close; 14 tests pass.
- **P3-clipboard:** Async Clipboard read/write text; tests pass.
- **P3-cookiestore:** Cookie Store API partitioned by origin; tests pass.
- **P3-cacheapi:** CacheStorage + Cache + match/put/delete/keys on sqlite backend; 32 tests pass.
- **P3-permissions:** Permissions.query + onchange; 10 tests pass.
- **P3-notifications:** Notification.requestPermission + show + SW getNotifications; 26 tests pass.
- **P3-offscreencanvas:** OffscreenCanvas transfer + 2D native from ImageData; 24 tests pass.

## Worktrees overview (do not modify unless asked)
| Worktree | Branch | Last commit | Likely purpose |
|----------|--------|-------------|----------------|
| `.claude/worktrees/graphic-followup` | `graphic-followup-local` | `c6c6fc0f` | Graphic/followup work |
| `.claude/worktrees/p1-laguna-140314` | `p1-laguna-t1-140314` | `37b8f185` | Laguna task 1 (stale snapshot) |
| `.claude/worktrees/p1-laguna-143746` | `p1-laguna-t1-143746` | `7d47a81a` | Laguna task 1 (later snapshot) |
| `.claude/worktrees/ph3-bfcache` | `p1-ph3-bfcache` | `54ecd6c3` | bfcache feature work |
| `.claude/worktrees/ph3-navapi` | `p1-ph3-navapi` | `451ba19f` | navapi feature work |

> Note: worktrees are Agent Manager artifacts. Their branches may be behind `main`. Do not merge/rebase worktrees without explicit user request.

## Next sessions — recommended order
1. **BUG-250** (P1, font baseline) — unblocks 2 driver graph tests; root cause already identified (FontMeasurer OS/2 ratio change)
2. **P3-vertical** (CSS writing-mode vertical text) — high-value, complex layout feature
3. **P3-initialletter** (drop-cap CSS) — smaller scope than full vertical
4. **P3-fragmentation** (break-inside/widows/orphans) — print/paging completeness
5. **P3-bfcache freeze** (JS heap snapshot upgrade) — build on existing HtmlSnapshot
6. **P3-structuredClone** (Transferable objects) — Web API completeness
7. **P3-pushapi / P3-reporting** — stub upgrades

## Constraints
- Root prompt language: Russian (do NOT switch to English mid-session)
- Do not commit without explicit user request
- Do not use `#[allow(dead_code)]` / `#[allow(unused)]` band-aids — fix root cause
- New public API must be unit-tested
- Do not mention tool names in final output
