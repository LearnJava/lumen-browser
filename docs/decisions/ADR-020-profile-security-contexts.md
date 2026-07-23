# ADR-020: Profiles as security contexts — isolation levels and the DS-16 ephemeral Anonymous slice

## Status

Accepted

## Date

2026-07-23

## Context

DS-14 (`docs/tasks/p1-design-v3.md`) added a profile switcher: an avatar
button + dropdown backed by `lumen_storage::ProfileRegistry`, seeding four
profiles (Личный/Рабочий/Анонимный/Гость) and letting the user switch the
*active* one. DS-15 gave each profile a visual signature (Anonymous — red
inset window outline; Guest — desaturated chrome). Both were explicit about
scope: **switching the active profile changes only the pointer and the
chrome's visual signature.** Neither touches where a tab's data — history,
cookies, bookmarks, downloads, and ~30 more `lumen-storage` stores — actually
lives. That gap is the subject of this ADR.

The DS track's own rationale (`docs/tasks/p1-design-v3.md` §Этап E) states
why this matters: a profile is not a cosmetic theme but a *privacy
guarantee*. Anonymous is supposed to be ephemeral — nothing survives the
session. Guest is supposed to be a separate person's data. Today, after
DS-14/DS-15, **none of that is true yet** — the signature is drawn, but
underneath it every profile shares exactly the same data. Shipping only the
signature without ever closing that gap would be actively misleading (a red
outline promising "nothing is saved" while everything is saved).

**A relevant fact discovered while implementing this slice:** most of the
`lumen-storage` stores the shell wires up are *already* in-memory-only for
every profile, not just Anonymous — `bookmarks`, `tab_groups`, `history_store`,
`cookie_jar`, `omnibox_aliases`, `read_later_store`, `a11y_store`, and more
are all `open_in_memory()` in `main.rs` today (`CAPABILITIES.md`: "⬜
history/search in-memory only"). Only a handful (`profiles.db`,
`settings.db`, `newtab_tiles.db`, panel layout) are wired to the portable
`<exe_dir>/data/` directory per [ADR-012](ADR-012-storage-partitioning.md).
Downloads has a dedicated `lumen_storage::Downloads` SQLite store defined in
the crate, but the shell's `download::DownloadManager` never uses it — the
downloads list is a plain in-process `Vec`, not persisted at all. **This is
pre-existing, general debt, unrelated to profiles**, and out of scope here.

Also relevant: `tabs::containers::ContainerStore` (7D.2) already models a
`(origin, ContainerKind) → store id` partitioning key for cookies/storage,
but its own doc comment is explicit that "the actual isolation pipeline
(cookie jar lookup, storage backend dispatch) is wired in later tasks" — it
mints ids but nothing consumes them yet. Building real per-profile isolation
on top of that stub is a separate, larger effort, not this slice.

## Decision

**1. Three isolation levels, in increasing order of what's implemented:**

| Level | Scope | Status |
|---|---|---|
| **0 — Profile** | Each profile gets its own on-disk data directory / vault (history, cookies, bookmarks, ..., all ~36 stores) | **Not implemented.** All profiles currently share one process-wide instance of every store. Requires a DB-manager-style re-open of every store on profile switch — see "No DB manager" below. |
| **Ephemeral Anonymous** | While Anonymous is active: no writes reach the shared history store/index; cookies go to a separate jar reset fresh every time Anonymous becomes active (so it never carries state into, or out of, other profiles or its own prior sessions) | **This slice (DS-16).** History + cookies only — see Scope below. |
| **Guest** | In-memory + monochrome signature (DS-15) | Visual signature only; no additional data behavior beyond "everything was already in-memory" — see Context. |

**2. No DB manager exists — link to [ADR-012](ADR-012-storage-partitioning.md).**
ADR-012 settled *how* individual stores are partitioned into files
(lifecycle/write-frequency), but there is no central component that opens
"the set of stores for profile N" as a unit and swaps it on profile switch.
Every one of the ~36 stores is a field on the `Lumen` shell struct, opened
once at startup. Full Level-0 isolation therefore cannot be one commit — it
is ~36 individual "does this store need a per-profile path, and does
switching profiles need to re-open it" decisions, each its own slice,
following the DS-16 pattern below as the template. This ADR intentionally
does not attempt to schedule all 36; each future slice picks the next store
that matters (history and cookies were picked here because they are the two
concrete DoD-visible surfaces named in the DS-16 brief).

**3. DS-16 scope — what this slice actually does:**

- **History**: `LoadDone` skips both the `history_fts` index write and
  `History::record_visit` while the active profile is Anonymous
  (`Lumen::active_profile_is_anonymous`, `crates/shell/src/main.rs`). Pages
  visited while Anonymous never appear in the shared in-memory history, so
  switching back to Personal shows no trace of them.
- **Cookies**: Anonymous gets its own `anonymous_cookie_jar`
  (`Arc<lumen_storage::CookieJar>`, in-memory), separate from the shared
  `cookie_jar` every other profile uses. `Lumen::active_cookie_jar()` picks
  which one every HTTP-client construction site uses. The jar is replaced
  with a fresh instance every time Anonymous *becomes* the active profile
  (`ProfileMenuHit::SwitchTo`) — so a second, later Anonymous session starts
  clean rather than accumulating cookies across sessions.
- **History-panel banner**: `history_panel.rs` draws a fixed banner
  ("История не сохраняется — Анонимный профиль") between the search box and
  the row list while Anonymous is active, so the user is told this in the
  same surface where they'd otherwise expect to find what they just
  browsed — not just inferred from the red window outline.
- **Downloads**: brief step 2 asks for "downloads history not written" — no
  code change was needed, since (see Context) downloads are already
  in-memory-only for every profile. Documented here, not re-implemented.
- **Switching to Anonymous and back does not lose the regular profile**:
  guaranteed structurally — Personal/Work/Guest always read/write
  `self.cookie_jar`/`self.history_store`, which DS-16 never touches;
  Anonymous is additive (`anonymous_cookie_jar` + a write-skip), not a
  swap-out.

**4. What is explicitly deferred (not this slice):**

- Full per-profile isolation of bookmarks, tab_groups, downloads, Web
  Storage, IndexedDB, Service Workers, or any of the other ~30 stores.
- Real Guest data isolation (today Guest is visual-only, same shared stores
  as Personal/Work — see Context on why this was already effectively true
  pre-DS-16 for most stores).
- Wiring `tabs::containers::ContainerStore`'s partition ids into an actual
  cookie-jar/storage dispatch (7D.2, tracked separately) — DS-16 solves
  Anonymous's cookie isolation with a second dedicated jar instead of
  reusing/extending the container mechanism, because the container pipeline
  itself is not wired to anything yet (see Context).
- Cross-tab isolation *within* a single Anonymous session (e.g. two
  Anonymous tabs open at once still share `anonymous_cookie_jar` with each
  other — same-profile tabs are not expected to be isolated from one
  another, only from other profiles).
- On-disk persistence for history/cookies for Personal/Work at all — a
  pre-existing gap this slice does not touch.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Wire `ContainerStore` ids into a real per-(origin, container) cookie/storage dispatch, then key Anonymous off a reserved container | Correct long-term direction, but the dispatch pipeline itself doesn't exist yet (7D.2) — building it as a side effect of DS-16 would blow the slice size and duplicate/conflict with that separate, already-tracked task |
| One ADR + one commit for full Level-0 (all ~36 stores, real per-profile SQLite files) | No DB-manager abstraction exists to do this atomically; each store has its own open-path/lifetime assumptions baked into `main.rs` — see ADR-012. Attempting it in one slice risks a half-migrated, inconsistent state |
| Clear the shared `cookie_jar` when leaving Anonymous instead of using a separate jar | Would also delete Personal/Work's cookies from the *same* session, violating the explicit DS-16 requirement that switching to Anonymous and back must not lose the regular profile |
| Treat "already in-memory for everyone" as satisfying the whole brief, skip the history/cookie code changes | Doesn't satisfy the DoD: today Anonymous and Personal share the *same* instance of `history_store`/`cookie_jar`, so Anonymous browsing is currently visible to Personal within one running session — the brief's manual test scenario would fail without this slice's changes |

## Consequences

- **Positive:** the Anonymous signature (DS-15's red outline) is no longer
  purely cosmetic — it now corresponds to a real, testable guarantee for the
  two things a user would most concretely check (history, cookies).
  `active_cookie_jar()`/`active_profile_is_anonymous()` give future slices a
  single place to extend the same pattern to more stores.
- **Negative / trade-offs:** the isolation is partial and profile-specific
  (only Anonymous, only two stores) — a user could reasonably expect
  "Anonymous" to also hide downloads/bookmarks from other profiles within
  the same session; it currently doesn't, beyond the pre-existing fact that
  none of that persists to disk for anyone. This must stay documented
  wherever the feature is user-facing (the banner text is intentionally
  narrow — "История", not a blanket "nothing is saved").
- **Future:** each additional store that needs real per-profile isolation
  gets its own slice following this ADR's pattern (a dedicated
  profile-scoped instance + a `Lumen::active_<store>()` accessor), starting
  with whichever store the next concrete privacy complaint or DoD names.
  Level-0 (full per-profile data directories) is the eventual target but has
  no committed timeline — it depends on a DB-manager-style abstraction that
  does not exist yet.
