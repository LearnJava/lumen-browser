# WPT vendor notes — `webvtt`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webvtt`, `docs/wpt-status.md`), scope ⬜ (in scope). Confirmed
before vendoring: the cue parser + `<track>` collection + active-cue overlay
are a real, wired-end-to-end implementation (`lumen_dom::vtt`,
`crates/shell/src/tracks.rs`), not a stub — `CAPABILITIES.md` already listed
WebVTT as "✅ cue-парсер + сбор `<track>` + active_cues/…".

Same pinned upstream commit `35be3b44`, `git sparse-checkout add webvtt` at
that commit, `LICENSE-WPT.md` copied from a sibling category — 821 files (820
upstream + license). Cheap predictors: 0 `name="variant"` hits, 0
`testdriver.js` hits, 0 `.https.` files. `run_report.py`'s glob counts 72
testharness ids; the remaining 748 files are non-testharness — 581 of the
category's 820 files are reftests under `rendering/` (`run_report.py` reports
6 of these categorized as `Unsupported test type reftest`, the rest aren't
even glob-selected since they lack a `testharness.js` include).

### Run result

`run_report.py --all --root webvtt --recursive` (~8 min 17 s, single
process): **21/72 harness OK, 1/176 subtests passed**.

Three distinct, confirmed clusters explain almost every failure:

- **[BUG-570](../../bugs/BUG-570-OPEN.md) (already filed, not new)** — all 19
  `api/VTTCue/*` and `api/VTTRegion/*` files: harness completes (`Test OK`)
  but every subtest fails on `VTTCue is not defined` / `VTTRegion is not
  defined` — the constructors were never installed as JS globals. Also
  explains `api/VTTCue/snapToLines.html`/`text.html`/`vertical.html`
  (`TypeError: video.addTextTrack is not a function`, the already-documented
  `CAPABILITIES.md` gap "⬜ addTextTrack()").
- **[BUG-775](../../bugs/BUG-775-FIXED.md) (new)** — all 40
  `parsing/file-parsing/tests/*.html` files and 5 of 6
  `parsing/cue-text-parsing/tests/*.html` files: `HTMLTrackElement` never
  dispatches `load`/`error`, so the standard `track.onload = ...; video
  .appendChild(track)` pattern every one of these tests uses hangs until the
  external wptrunner timeout. The cue *data* pipeline itself
  (`crates/shell/src/tracks.rs::load_video_tracks`) is real and unit-tested,
  but it's a pure internal Rust layer with no JS-visible "ready" signal —
  confirmed by `grep -n "HTMLTrackElement" crates/js/src/dom.rs`, which shows
  only attribute reflection (`kind`/`src`/`srclang`/`label`/`default`), and
  `fireEvent()` in `video_bindings.rs`, which is called for `<video>`/
  `<audio>` events only, never for `<track>`.
- **Not a Lumen defect — missing shared WPT resource files.**
  `api/idlharness.window.html` TIMEOUTs on `idl_test is not defined`;
  `/resources/WebIDLParser.js` and `/resources/idlharness.js` 404. Neither
  file exists anywhere under `tests/wpt/resources/` in this repo yet (a
  vendoring-completeness gap in the shared `resources/` mirror, not specific
  to `webvtt` — every category's `idlharness.*` test will hit the same 404
  until those two files are vendored once, top-level). Not filed as an
  engine bug.

One outlier not in either cluster:
`api/VTTRegion/non-visible-cue-with-region.html` — `ReferenceError:
getVideoURI is not defined`, a category-local test helper referenced but not
found; not investigated further this session (single file, low priority next
to the two clusters above).

`rendering/` (581 files, reftest-only) — visual correctness of cue rendering
against upstream references not checked this session; `run_report.py` cannot
execute reftests at all (`Unsupported test type reftest`).

## Прогон и находки (`docs/wpt-status.md`)

Скоуп ⬜ подтверждён точно перед вендорингом (cue-парсер + `<track>`-сбор +
оверлей — реальный код, `lumen_dom::vtt`/`crates/shell/src/tracks.rs`, не
заглушка). Вендорена целиком 2026-08-18 (коммит `35be3b44`,
`tests/wpt/webvtt/`, 821 файл, 72 testharness-id; 581 из 820 файлов —
reftest в `rendering/`, раннер их не исполняет).

`run_report.py --all --root webvtt --recursive` — ~8 мин 17 с, **21/72
harness OK, 1/176 сабтестов**. Два подтверждённых корня объясняют
подавляющее большинство провалов: уже заведённый
[BUG-570](../../bugs/BUG-570-OPEN.md) (`VTTCue`/`VTTRegion`/`TrackEvent` не
установлены как глобалы — весь `api/VTTCue`/`api/VTTRegion`) и новый
[BUG-775](../../bugs/BUG-775-FIXED.md) (`HTMLTrackElement` никогда не диспатчит
`load`/`error` — весь `parsing/file-parsing/` и большая часть
`parsing/cue-text-parsing/` виснут до таймаута раннера). Остальное —
недостающие общие ресурсы WPT (`WebIDLParser.js`/`idlharness.js`, не
вендорены нигде в репозитории, не специфично для этой категории) и один
неисследованный файл-выброс (`getVideoURI is not defined`).

**Повторный прогон 2026-08-24** (P1, после фикса
[BUG-775](../../bugs/BUG-775-FIXED.md), та же команда, тот же слот): **88/322
harness OK, 31/178 сабтестов, 0 мин 58 с** против **42/322, 2/178, 8 мин 46 с**
на непосредственно предшествующем коммите. (Знаменатель harness — 322, а не 72:
он считает и reftest-ы `rendering/`, которых раннер не исполняет; сравнивать
осмысленно только сабтесты и время.) Обвал времени прогона — прямое следствие
исчезновения ~10-секундных таймаутов.

Освободившийся остаток перестал прятаться за TIMEOUT и распался на два новых
механизма, оба заведены: [BUG-902](../../bugs/BUG-902-OPEN.md) — нет
`VTTCue.getCueAsHTML()` (92 сабтеста `cue-text-parsing`), настройки cue не
отдаются странице, `VTTRegion` отсутствует (33 сабтеста); и
[BUG-903](../../bugs/BUG-903-OPEN.md) — конформность самого `parse_vtt`
(границы блока по WebVTT §5, строгость подписи), 6 сабтестов
`file-parsing/`.
