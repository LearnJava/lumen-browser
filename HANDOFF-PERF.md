# Handoff: ускорение загрузки сайтов в Lumen (временный документ)

> Временный файл для переноса задачи на другую машину/сессию. Можно удалить после закрытия эпика.
> Дата: 2026-11-23. Ветка: `main`, remote `github.com:LearnJava/lumen-browser.git`.

## Обновление: текущая сессия (после влития BUG-1837/okko.tv)

Ниже — старые числа; актуальное состояние:

**Запушено в этой сессии (habr, measured):**

- `8a52ce69d` — JS-профилирование (`js.job/run/compile/call/ptask/module_*` скоупы).
- `0be522a7d` — cq sweep change-detection (relayout только при реальных изменениях).
- `74cecf135` — pseudo-store не перезаписывает snapshot-hit.
- `a80a13fa7` — spine при quote/counter: walk ~5000→~700 узлов/flush.
- `0bedd01d9`,`61a64a965` — census pre_match hit/miss.
- `1016888a0` — pre_match full-table (subset промахивался по покрытию).
- `ed9f1e2cc` — drain `pseudo_records` вместо клона (522→81ms).
- `75201e632` — общий пустой `Arc` для ~90% pseudo-записей.
- `4c85f605b` — фиксация отказа от size-порога subset (замерено: хуже).
- `79eb6a516` — **QS-кэш**: querySelector/getElementById по эпохе DomTouched; `_lumen_query_selector` 2986→309ms native-time.
- `766bd62c0` — PseudoCache IdMap (u32-ключи, IdHasher).
- **`1b5c56734` — ключевой фикс**: `pre_match_needed` в `counters.rs` (~1647) — экспансия `needed` останавливалась на узлах spine, боковые поддеревья forced-рекомпутов не достигались → 99% `miss_norules`. Отдельный `expanded` set; теперь **100% pre_match hits на всех flush'ах**, subset-таблица. `pre_match` ~1.44→0.96s, `precompute_counters` ~5.5→~4.3s.
- Влит чужой BUG-1837 (okko.tv): общий CascadeIndex для media/supports, точный диф листа, псевдо-селекторы в инкрементальном пути — на habr нейтрально, на okko load 9.2→5.0s.

**Актуальная карта bottleneck (habr, LUMEN_PROFILE_TREE=1, валидный прогон):**

```
js.run              ~25s   ≈ js.job — V8 + ВСЕ native-колбэки (DOM, таймеры)
maybe_flush         ~14s   ~93 реальных flush'а за загрузку, все по flush_stale
  precompute         ~4.3s = cascade_walk ~3.0 + pre_match ~0.96 + pseudo_store ~0.7-0.9
  post_layout        ~2.6s → cq.relayout ~2.2s (реальные промахи layout-cache)
  build_box          ~2.0s
  collectors         ~2.8s = computed_styles ~1.6 + pseudo_styles ~0.8 (уже scope-pruned)
wall-clock (без профайлера, scripts/perf/wall_site.py): interactive ~10-16s
```

**Native-time (LUMEN_NATIVE_CALL_STATS=1):** style/rect-энтри — это стоимость flush'ов,
приписанная вызывающим нэйтивам (не отдельный резерв). `_lumen_get_parent` ×200k —
0.2-2мкс/вызов, потолок ~1-2s суммарно.

**Остаток до Chrome (~2-3s):**

1. **Forced-рекомпуты ~2400/flush** — habr мутирует `html`/`head` (инжект скриптов, атрибуты)
   → deep roots → `child_force` пересчитывает всё поддерево, `forced_same ≈ 96%`.
   Ранний выход уже доказанно небезопасен (14 тестов). Путь — расширение
   invalidation-индекса (`style/invalidation.rs`, MatchRestriction) на deep-roots,
   уровень BUG-935-срезов.
2. `compute_style` остаток ~1.8s: cs_apply/cs_init на forced-идентичных узлах
   (~10-25мкс/узел — честная стоимость ComputedStyle).
3. `cq.relayout` ~2.2s — per-child lay_out в size-контейнерах; partial-skip чистых
   детей требует translate + pixel-гейта (graphic_tests).
4. `collect_computed_styles` ~1.6s — уже scope-pruned, StyleMap ленивый
   (сериализация только при чтении JS) — остаток это `apply_used_geometry` + HashMap.
5. Число flush'ей (~93/загрузку) — coalescing между layout-чтениями JS ограничен
   семантикой синхронного layout.

**Инструменты сессии:** `scripts/perf/wall_site.py` (wall-clock), `prof_site.py`,
`agg_prof.py`, `site_ab.py`. Флаги: `LUMEN_PROFILE_TREE`, `LUMEN_PROFILE_DETAIL`,
`LUMEN_NATIVE_CALL_STATS=1`, `LUMEN_V8_FLAGS` (BUG-1826), `LUMEN_V8_PROFILE` (V8 --prof → v8.log),
`LUMEN_DUMP_RECOMPUTE=1`, `LUMEN_DUMP_BARE_RECOMPUTE`, `EXTRA_ENV` в perf_audit.py.


## Цель пользователя (дословно)

- «Составь себе план и реализуй укорение сайтов, минимум скорость как у chrome, а лучше быстрее».
- «делай все варианты, пока не получится. Ничего не спрашивай, только когда будет положительный результат — сообщи».
- «Должно же всё идти параллельно и не блокировать друг друга» — UI не должен виснуть.
- «Не запускай много копий браузера — по одной, с логированием».
- `.ru`-домены могут не работать через прокси Hiddify (`127.0.0.1:12334`) — тогда иностранные домены или ru без прокси.
- «Делай всё одним раундом, без промежуточных отчётов» — работать до измеренного результата.

## Что уже сделано и запушено (main, ~20 перф-коммитов)

Ключевые вехи (все в `origin/main`):

- **Pseudo match-record pipeline** (7 коммитов, `655bde5f`…`7f3d87f8`): параллельный pre-match псевдо-элементов, `Arc<[PseudoMatch]>`, reuse между проходами, rayon-safe кэш, пустые записи для exempt, fallback для functional UA-деклараций. Итог на habr: sequential `ps_match` ~105000 → **0**, `pseudo_style` ~1.6с → ~0.15с.
- **MatchedScan** (`e26b52c4`,`57f04bca`): битовая карта `matched` гейтит ~9 pre-pass'ов каскада.
- **PERF-SHADOW-HOST, PERF-SHARE-PROBE, PERF-INLINE-CACHE, PERF-DECL-FLAGS** — аллокации/сканы в каскаде.
- **PERF-APPLY-ALLOC серия**: align/box-sizing/text-decoration/border-radius/margin/padding/inset парсеры без per-call аллокаций; `parse_length_q` thread-local кэш `(value,quirks)`; `Quotes::Pairs` и `Content::Items` → `Arc` (наследуемый clone = refcount).
- Ранее: PERF-WIDE-TICK (широкие мутации off-thread), PERF-LATE-CSS, PERF-CQ-SKIP, PERF-QS-FIRST.

Подробная карта — `subsystems/layout.md` §«осты» (там же дизайн-ограничения каскада, инвариант pre_match/parallel pass на строке ~240 и план declaration-cache ~317).

## Измеренная база (habr.com/ru/, живое окно, `--maximized`, `LUMEN_NO_ADBLOCK=1`)

- TTFB ~0.5–1.2с — сеть не проблема. ~250 ресурсов, ~127 JS-бандлов + ~58 css/font с assets.habr.com + картинки habrastorage + рекламный хвост.
- `readyState=interactive` ~37–73с; engine thread busy-окно ~85–240+с; `complete` ~160с (лучший прогон). Под `LUMEN_PROFILE_TREE=1` — ещё медленнее (1M строк лога).
- Eval через MCP (`eval` tool) возвращает `JS context not available: page is still loading`, затем `engine thread busy` — само по себе симптом: engine thread (V8+DOM+style flush) — узкое место.

### Профиль-скоупы (агрегат за частичный прогон с инструментированием)

| scope | ms |
|---|---|
| compute_style | 8214 (внутри: pre_match 4708, cs_match 2525, cs_init 1744, cs_apply 1324, cs_ua_hints 975, cs_post 742, cascade_walk 722) |
| precompute_counters | 1620 |
| pseudo_style | 1523 |
| layout_measured_hyp | 1462 |
| mtws/lo_wrap/lw_word (text) | ~1100 |
| layout_mutation_incremental_restyle | 608 |

### Apply-census (`LUMEN_APPLY_CENSUS=1`, топ ns/call)

`transition` 2.83 (166мс суммарно — №1), `border-radius` 3.08, `text-decoration` 2.9, `color` 1.73, `content` 1.66, `quotes` 1.23, `width` 1.44 (после length-кэша; было ~3.7).

## Таблица приоритетов (по измеренной отдаче)

1. **JS-исполнение на engine thread** — доминанта busy-окна, до сих пор не измерена напрямую. Только что добавлена инструментация: скоупы `js.run` (classic scripts, `eval.rs`) и `js.module_eval` (`v8_esm.rs`) — **незакоммичено, в рабочем дереве**. После билда прогнать habr-профиль и посмотреть долю js.* против layout-скоупов.
2. **`pre_match` 4.7с** — параллельный pre-match стал сам по себе дорогим; проверить, не дублирует ли он работу cs_match, и стоимость StyleEnvSnapshot::install на воркерах.
3. **`transition` apply** — 5 Vec-клонов в ComputedStyle; план: поля `transition_*` → `Arc<Vec<T>>` (~145 сайтов, read через deref, писать через `Arc::make_mut`; ~15 тестовых assert_eq поправить `.as_ref()`).
4. **`precompute_counters` 1.6с / `pseudo_style` 1.5с / `layout_measured` 1.5с** — counter/quote walk, first-computation misses.
5. `border-radius` 3.08мкс — 8×parse_radius_length на декларацию (length уже кэширован — remaining = 8 вызовов + slash-split).
6. Maybe_flush JS-side ~1.3–2.3с — `style_flush.rs` коллекторы (уже частично scoped; first-collect fallback обязателен).

## Как продолжить (команды)

```bash
cd C:\kostja\Rust_Projects\Lumen_browser   # или аналог на новой машине
git pull -q origin main
# сборка с профилем (нужен LIBCLANG_PATH для boring-sys):
LIBCLANG_PATH="C:\Users\konstantin\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\v8-150.1.0\third_party\rust-toolchain\bin" \
  cargo build --profile dev-release -p lumen-shell
# живой профиль одного сайта (одно окно, логи в .tmp/prof-run — gitignored):
python scripts/perf/prof_site.py "https://habr.com/ru/" 90     # python = pythoncore-3.14-64
# агрегация scope-ов:
python scripts/perf/agg_prof.py .tmp/prof-run/live.stderr.prof.0.log
# apply-census: LUMEN_APPLY_CENSUS=1 в env перед prof_site.py
```

- Eval-коллектор resource-timing: `scripts/perf/probe_collect.py` + `scripts/perf/habr_collect.js` (JS собирает navigation/resource/paint/longtask entries; парсит `r["result"]` → двойной json.loads).
- Важно: у eval-ошибок `engine thread busy` — не баг скрипта, а symptom busy-окна; просто ретраить.
- После КАЖДОГО изменения: `cargo test -p lumen-layout --lib -- <фильтр>` + `cargo clippy -p lumen-layout --all-targets -- -D warnings`, коммит explicit paths, push в main. Пиксельные изменения → `python graphic_tests/run.py --continue-on-fail` + golden в том же коммите.
- Числа проверять по логам/тестам, не по памяти; measured vs inferred разделять.

## Что сказать новой сессии (промпт для продолжения)

> Продолжи эпик ускорения Lumen Browser. Прочитай `HANDOFF-PERF.md` (этот файл) и `subsystems/layout.md`. Задача: довести загрузку реальных сайтов до скорости Chrome или быстрее, работая по таблице приоритетов из handoff — начни с пункта 1 (JS-исполнение: только что добавлены скоупы `js.run`/`js.module_eval`, их надо проверить в живом профиле habr и затем оптимизировать найденный bottleneck). Не останавливайся на промежуточных отчётах — работай до измеренного положительного результата, регулярно перемеряя habr. Один браузер за раз, `--maximized`, `LUMEN_NO_ADBLOCK=1`. Коммить и пушь в main после каждого проверенного шага.

## Известные ловушки

- `cargo build` требует `LIBCLANG_PATH` (boring-sys) — путь выше.
- Пакет shell называется `lumen-shell`, бинарь `target/dev-release/lumen.exe`.
- `.tmp/prof-run` логи большие (сотни МБ) — чистить периодически.
- Осиротевшие `lumen.exe` копятся при падении скриптов — `tasklist | grep lumen`, убивать старые PID.
- `LUMEN_PROFILE_TREE=1` сам по себе сильно замедляет страницу — для wall-clock замеров не использовать, только для CPU-разложения.
- Каскадные инварианты (var/attr/env, shorthand, revert, css-wide) — нельзя дедуплицировать apply без typed values; детали в `subsystems/layout.md`.
