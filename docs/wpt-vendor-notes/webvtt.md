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

- **[BUG-570](../../bugs/BUG-570-FIXED.md) (already filed, not new)** — all 19
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
[BUG-570](../../bugs/BUG-570-FIXED.md) (`VTTCue`/`VTTRegion`/`TrackEvent` не
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

## Разведка WPT-VTT-SCOUT (2026-10-27)

**Прогон** (`run_corpus.py --prefixes webvtt --processes 6`, `dev-release` от `main` `0d2c7b4d8`, 303 с): 322 id, **13.79 %**, балл 44.41, harness OK 90, сабтесты 44 / 233. Статусы: FAIL 229 (все — рефтесты), OK 69, PASS 21 (рефтесты), ERROR 3. Потеряно 277.6 балла.

**Главное.** Ни один рефтест не видит cue: все 229 FAIL-снимков теста — пустое видео (221 побайтно одинаковы). Причины:
1. Оверлей cue рисуется только в живом окне — `crates/shell/src/app/window_event/redraw_requested.rs:1172` (`tracks::build_cue_overlay` дописывается в display list перед GPU-кадром). Рефтесты идут через `--ipc-server` = однократный конвейер `--screenshot` (`dump_mode.rs`, `render_current_page_to_png` в `crates/shell/src/lumen/automation.rs:430` растеризует `self.display_list`) — cue туда не попадает ([BUG-1748](../../bugs/BUG-1748-OPEN.md)). Проверено пробником: в конвейере снимка `activeCues.length=1`, но cue не нарисован.
2. Даже в окне оверлей — самодельный (Inter, кегль 6 % высоты, свой фон), не CSS-боксы WebVTT §7.2 и без `::cue`. `::cue`/`::cue()`/`::cue-region` в CSS-парсере нет вовсе (`crates/engine/css-parser/src/parser/selectors.rs` — только упоминание в комментарии). `support/webvtt-test.css` почти всех тестов задаёт шрифт/цвет/фон через `::cue`.
3. `reftest-wait` висит до 5-секундного лимита (медиана длительности 5.3 с): `checkCueChanges` (`crates/js/src/shim/video_element.js:366`) зовётся по тику воспроизведения/seek и один раз `setTimeout(0)` в `patchVideoElement` (стр. ~1602) — до загрузки `<track>`, причём с ложным `cuechange` при пустом наборе (`_activeSig` undefined → ''); после загрузки трека/смены `mode` перепроверки нет, а видео webm/mp4 не играет (`networkState=3`, `currentSrc=''`) → `waitForActiveCueAndTakeScreenshot` не получает `cuechange`. → [BUG-1749](../../bugs/BUG-1749-OPEN.md).
4. Побочный дефект раскладки ([BUG-1750](../../bugs/BUG-1750-OPEN.md)): фон inline-элемента не рисуется под его вложенными inline-элементами — `<span style="background:#fc0">a <b>bold</b> c</span>` даёт `FillRect` только под «a» и «c» (`--dump-display-list`, т.е. и GPU, и CPU). Из-за него эталоны `bidi/*-ref.html` (`.cue > span {background; color:white}` + `<bdo>`) рендерятся пустыми → **7 ложных PASS** (`bidi/bidi_ruby`, `u0041_first`, `u05D0_first`, `u0628_first`, `u06E9_no_strong_dir`, `vertical_lr`, `vertical_rl`). Ещё замечено (не локализовано): пробел после `</bdo>`/`<span dir>` визуально теряется (`bdo-ltrend`).
Ещё 13 PASS — эталон сам на `<video>` (embedded_style_* ×9, non-standard-pseudo-elements, dom_override_remove_cue_while_paused) или cue по спеке скрыт (audio_has_no_subtitles, cue_too_long, media_404, regionanchor_y_50) — сравнение «пусто = пусто», после появления рендеринга могут стать FAIL.

### Кластеры (балл — потолок, id в первом совпавшем кластере, п.п. — от 322)

| Кл. | Группа | id | Сабт. | Статусы | Причина | Куда | Балл (п.п.) |
|---|---|---|---|---|---|---|---|
| R1 | `processing-model/` базовые + `bidi/` + `evil/` | 45 | — | FAIL 45 | нет рендеринга cue в конвейере снимка + нет CSS-боксов §7.2 + `::cue` | `VTT-RENDER` (P1) + `::cue` (P4) | 45 (14.0) |
| R2 | `dom_override_*` | 5 | — | FAIL 5 | после R1; изменения cue из JS → перерисовка, IDL-сеттеры VTTCue | после R1 + H6 | 5 (1.6) |
| R3 | `embedded_style_media_queries_resized` | 1 | — | FAIL 1 | STYLE-блоки VTT → таблица `::cue` | после R1/R5 | 1 (0.3) |
| R4 | `regions/` | 7 | — | FAIL 7 | после R1 + `VTTRegion` (BUG-902 п.3) + рендер регионов | после R1, BUG-902 | 7 (2.2) |
| R5 | `selectors/cue/`, `default_styles/` | 30 | — | FAIL 30 | `::cue` без аргумента, белый список свойств §8.2.1 | `::cue` в `CSS-SPECS.md` (P4) | 30 (9.3) |
| R6 | `selectors/cue_function/*`, `cue-region*` | 141 | — | FAIL 141 | `::cue(sel)` по узлам WebVTT (b/i/u/c/v/lang), `::cue-region` | `::cue` (P4), после R5 | 141 (43.8) |
| H1 | `cue-text-parsing/` | 6 | 0/79 | OK 6 | `getCueAsHTML` не у объектов движка | BUG-902 п.1 | 6 (1.9) |
| H2 | `file-parsing/settings-*` | 7 | 0/7 | OK 7 | настройки cue не сериализуются | BUG-902 п.2 | 7 (2.2) |
| H3 | `VTTRegion`, `regions-*`, `header-regions`, `idlharness` | 18 | 7/74 | OK 18 | нет `VTTRegion` | BUG-902 п.3 | 17.9 (5.6) |
| H4 | `arrows`, `nulls`, `whitespace-chars`, `timings-negative`, `signature-invalid` | 5 | 9/15 | OK 5 | `parse_vtt` | BUG-903 | 4.2 (1.3) |
| H5 | `api/VTTCue/{snapToLines,text,vertical,getCueAsHTML,align,line,lineAlign,region}`, `non-visible-cue-with-region` | 9 | 0/20 | OK 6, ERROR 3 | нет `video.addTextTrack()`/`TextTrack.addCue()/removeCue()` (⬜ в CAPABILITIES; «frag is not defined» ×12 — следствие) | `VTT-ADDTRACK` (P1) | 9 (2.8) |
| H6 | `api/VTTCue/{constructor,constructor-exceptions,position,positionAlign,size}`, `historical` | 6 | 7/17 | OK 6 | VTTCue — собственные data-свойства (`video_element.js:286`): нет IDL-аксессоров на прототипе, нет валидации enum/IndexSizeError, NaN не бросает TypeError; нет глобала `TextTrack` | [BUG-1751](../../bugs/BUG-1751-OPEN.md) | 4.5 (1.4) |
| — | PASSED | 42 | 21/21 | OK 21, PASS 21 | из них 7 ложных (bidi) | | 0 |

**Оформлено:** баги [BUG-1748](../../bugs/BUG-1748-OPEN.md) (cue вне живого окна), [BUG-1749](../../bugs/BUG-1749-OPEN.md) (`cuechange`), [BUG-1750](../../bugs/BUG-1750-OPEN.md) (фон inline), [BUG-1751](../../bugs/BUG-1751-OPEN.md) (IDL `VTTCue`) → `STATUS-P3.md`; `ROADMAP.md` `VTT-RENDER` (R1–R4) и `VTT-ADDTRACK` (H5) → `STATUS-P1.md`; строка `::cue` в `CSS-SPECS.md` (R3/R5/R6, после `VTT-RENDER`) → `STATUS-P4.md`. Раннер не трогали: длительность рефтестов (~5.3 с) — следствие BUG-1749, не бюджета; таймауты не нужны. Порядок по приросту: `::cue` (53 п.п., но после VTT-RENDER) → VTT-RENDER (18 п.п.) → BUG-902 (≈9.7 п.п. H1–H3) → VTT-ADDTRACK / BUG-1751 / BUG-903.

**Проверка/скрипты** (в `.tmp/` слота `p2-work`, не коммитятся): `vtt-run.ids.tsv.gz`, `vtt-run/webvtt.raw.jsonl` (снимки рефтестов base64), `vtt_cl.py [кластер]` — таблица, `vtt_shots.py list|dump <подстрока>`, `vtt_falsepass.py` (нужен `serve_wpt_like.py --port 18765`), пробники `.tmp/vtt-probe/*.html`.
