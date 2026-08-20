# Lumen fuzz harnesses (TEST-1)

`cargo-fuzz`/libFuzzer harnesses over Lumen's input-parsing entry points.
Not a member of the main workspace (`fuzz/Cargo.toml` has its own empty
`[workspace]` table) — this directory needs a nightly toolchain to run,
the browser build stays on stable. See `docs/tasks/p2-test-track.md` for
the track brief and `ROADMAP.md` (`TEST-1`) for the DoD.

Goal: no panics/UB on arbitrary input, not correctness of parsing.

## Targets

| Target | Crate entry point | Input |
|---|---|---|
| `fuzz_css_parser` | `lumen_css_parser::{parse, parse_inline_style, parse_selector_list}` | `&str` (stylesheet / declaration list / selector list) |
| `fuzz_html_parser` | `lumen_html_parser::parse` | `&str` (HTML document) |
| `fuzz_url` | `lumen_core::url::Url::{parse, resolve}` | `&str` (URL / URL reference) — the BUG-346/347 class of bugs |
| `fuzz_font` | `lumen_font::{maybe_decode_font, Font::parse}` | `&[u8]` (WOFF/WOFF2/raw TTF/OTF bytes) |
| `fuzz_image` | `lumen_image::decode` | `&[u8]` (PNG/JPEG/GIF/WebP/AVIF bytes, dispatched by magic bytes) |

## Where these actually run: CI

**The primary way to run these harnesses is the `Fuzz` GitHub Actions
workflow** ([../.github/workflows/fuzz.yml](../.github/workflows/fuzz.yml)),
not a dev machine. libFuzzer needs an ASan runtime and ELF section-boundary
symbols that Windows/MSVC does not provide (details in the "Setup" section
below and in `docs/tasks/p2-test-track.md#test-1-состояние`); a Linux runner
has both.

| Trigger | Budget per target | Purpose |
|---|---|---|
| push touching `fuzz/**` | 60 s | harness rot guard |
| weekly cron (Mon 05:00 UTC) | 300 s | the actual sweep |
| `workflow_dispatch` | `duration` input (default 300 s) | on-demand, after touching a parser |

`workflow_dispatch` also takes a `targets` input (space-separated, empty =
all five). To launch a longer run of one target:

```bash
gh workflow run fuzz.yml -f duration=1800 -f targets=fuzz_css_parser
gh run watch                     # or: gh run list --workflow=fuzz.yml
```

A crash uploads its repro input as the `fuzz-artifacts-<run-id>` artifact
(the only copy — `fuzz/artifacts/` is gitignored); every run uploads the
libFuzzer-grown corpus as `fuzz-corpus-<run-id>`, kept 7 days. Download with
`gh run download <run-id>`, then minimize and file per the "Crashes" section
below.

### Known-failing targets

A target with a filed, unfixed finding is listed in the workflow's
`KNOWN_FAILING` variable. It still runs and still uploads repros, but its
failure does not turn the job red — the same bargain as `KNOWN_DEBTORS` in
`graphic_tests/run.py`, and for the same reason: a permanently red job stops
being read. If such a target finds nothing in a run, the job prints a
`::notice` suggesting its bug may be fixed (weak evidence — fuzzing is
stochastic). **Remove a target from `KNOWN_FAILING` in the commit that fixes
its bug.**

Currently listed:

- `fuzz_css_parser` — [BUG-788](../bugs/BUG-788-OPEN.md), `parse` blows up to
  hundreds of MiB on a kilobyte of malformed CSS.
- `fuzz_image` — [BUG-790](../bugs/BUG-790-OPEN.md), a 78-byte GIF whose header
  declares a 65281×57098 logical screen asks the allocator for 14.9 GiB.

`fuzz_image` is worth a note, because its history is the argument for keeping
the two mechanisms separate. It was delisted on 2026-08-20 together with the
[BUG-787](../bugs/BUG-787-FIXED.md) fix (GIF LZW unpack never returned) and
re-listed the same day: the first run in which it was blocking immediately
found BUG-790 — a different defect in the same target. BUG-787 itself has not
regressed, and you can see that without reading a fuzz log: its repro lives in
`regressions/fuzz_image-gif-lzw-hang` and is replayed by the **blocking**
replay step, which `KNOWN_FAILING` does not cover. So a fixed bug keeps its
guarantee even while the target is muted for the next one.

The corpus is **not** persisted between CI runs — each run starts from the
committed seeds. That is why the sweep is weekly rather than nightly: without
accumulated coverage, consecutive runs re-explore the same shallow space.
Promoting genuinely interesting inputs from a `fuzz-corpus-*` artifact into
the committed seed set is a manual, reviewed step.

## Setup (local)

`cargo-fuzz` needs a nightly toolchain and (on Windows) is only recently
and partially supported — this repo's dev machines are Windows/MSVC, so
**WSL is the recommended way to run these locally**:

```bash
# one-time, inside a WSL (Ubuntu) shell — does not touch the Windows toolchain
curl https://sh.rustup.rs -sSf | sh
rustup toolchain install nightly
cargo install cargo-fuzz
```

Then, from the repo root **inside WSL** (`/mnt/d/RustProjects/lumen-browser`
if the repo lives on `D:` in Windows):

```bash
cd fuzz
cargo +nightly fuzz run fuzz_css_parser -- -max_total_time=300
cargo +nightly fuzz run fuzz_html_parser -- -max_total_time=300
cargo +nightly fuzz run fuzz_url -- -max_total_time=300
cargo +nightly fuzz run fuzz_font -- -max_total_time=300
cargo +nightly fuzz run fuzz_image -- -max_total_time=300
```

Native Windows is possible via `cargo fuzz run --no-include-main-msvc` on a
nightly toolchain with MSVC AddressSanitizer support (`rust-fuzz/cargo-fuzz`
CHANGELOG, added recently) but is less battle-tested — prefer WSL unless
you have a specific reason to fuzz on native Windows.

### Verified end-to-end 2026-08-19

The WSL route above was written from documentation while WSL was unusable on
this machine (`CLAUDE.md` §Known gotchas carried a "cannot run locally, use CI"
note for a day). It has now been executed for real, from Windows, without
entering an interactive WSL shell:

```bash
wsl -- bash -lc 'cd /mnt/d/RustProjects/lumen-browser/fuzz && \
  cargo +nightly fuzz run fuzz_url -- -max_total_time=60'
```

Toolchain in that distro: `rustc 1.100.0-nightly (2026-08-18)`, `cargo-fuzz
0.13.2`, `clang 18.1.3`. Result: 651 579 executions in 61 s (~10 700 exec/s,
RSS 532 MB, no crash). Rebuilding just the harness crate against a warm
`fuzz/target/` took 8.6 s; a cold build of the dependency tree was not measured.

Three things worth knowing before you copy the command:

- **A run rewrites the seed corpus.** That 60-second `fuzz_url` run left **320
  new untracked files** in `corpus/fuzz_url/`. This is normal libFuzzer
  behaviour (see «Seed corpus» above) but it means `git add -A` after fuzzing
  silently commits a few hundred junk inputs. Clean up with
  `git clean -f fuzz/corpus/` unless you deliberately reviewed additions.
- **`/mnt/d` works.** Building and fuzzing straight against the Windows-side
  checkout over the 9p mount is fine — no need to clone into the WSL
  filesystem. `fuzz/target/` there holds Linux artifacts and is gitignored, so
  it does not disturb the Windows build.
- **A pool worktree works too** — the path is just
  `/mnt/d/RustProjects/lumen-browser/.claude/worktrees/p<N>-work/fuzz`. Each
  slot keeps its own `fuzz/target/`, so the first run in a fresh slot pays a
  cold build.

Local runs do not replace [`.github/workflows/fuzz.yml`](../.github/workflows/fuzz.yml):
CI is the unattended half (weekly sweep, and any push touching `fuzz/**`),
local is the one you reach for when checking a fix or minimizing an input.

## Seed corpus

`corpus/<target>/` holds a small curated seed set (checked in) sourced from
`graphic_tests/*.html`, `samples/`, `assets/fonts/`, `crates/engine/image/tests/fixtures/`,
and a handful of files from the vendored `tests/wpt/` tree. `fuzz_url`'s
seeds are hand-written (dot-segment collapsing, IDN/punycode, IPv6 host,
percent-escaping, `data:`/`file:` schemes) since there's no existing URL
corpus in-repo.

libFuzzer grows the corpus in-place while running (new coverage-increasing
inputs get written as hex-named files into `corpus/<target>/`) — this
growth is **not meant to be committed wholesale**. After a local run,
check `git status` and stage only genuinely useful, reviewed additions
(or none at all); don't `git add -A` the directory.

To point a run at the full WPT tree instead of the curated seed set (much
slower first pass, better initial coverage):

```bash
cargo +nightly fuzz run fuzz_html_parser -- /mnt/d/RustProjects/lumen-browser/tests/wpt -max_total_time=300
```

## Crashes

A crash writes a minimized-on-demand repro to `artifacts/<target>/` (not
committed, in `.gitignore`). Minimize and move genuine crashes into
`regressions/`:

```bash
cargo +nightly fuzz tmin fuzz_css_parser artifacts/fuzz_css_parser/crash-<hash>
cp artifacts/fuzz_css_parser/minimized-from-* ../fuzz/regressions/fuzz_css_parser-<short-desc>
```

Replay a saved regression (no fuzzing, single run):

```bash
cargo +nightly fuzz run fuzz_css_parser regressions/fuzz_css_parser-<short-desc>
```

Group findings into `BUG-NNN` by root cause per `docs/wpt-status.md`'s
methodology (don't file one bug per crashing input) and commit the repro
alongside the fix.
