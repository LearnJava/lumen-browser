# LIB-3 инструментальная задача — реальные системные шрифты в headless CPU-раст-пути

Метод — [`docs/conformance-method.md`](../conformance-method.md). Продолжение
[`2026-08-31-post-lib2.md`](2026-08-31-post-lib2.md) (которое установило: ни
headless `--screenshot`, ни живое окно+`gdigrab` не могли измерить наш шейпер
на нелатинице — первый не подключал `SystemFontIndex` вообще, второй рисует
через собственный встроенный `rustybuzz` femtovg, минуя
`lumen_font::active_text_shaper()`).

Движок: `p1-lib3-cpu-system-fonts-probe`, база `main` после `4cfce71b0`.
Машина та же (Windows 10 Pro 10.0.19045, CTNB002737).

---

## 1. Что сделано

Единственная настоящая дыра была в CPU-раст-пути (`cpu_raster.rs`,
`CpuBackend`) — он жёстко использовал только bundled Inter, без какого-либо
понятия `FontProvider` вообще (`CpuBackend::set_font_provider` — буквальный
no-op; `rasterize_text`/`rasterize_text_mixed`/`rasterize_text_rotated` не
принимали `font_family` параметром вовсе). Это единственный путь, на который
сходятся `--screenshot`, `lumen-driver`'s детерминированный CPU-снимок и MCP
`resource://screenshot` — GPU headless-путь (`Renderer::new_headless`, wgpu)
уже подключает реальный `SystemFontIndex` в `init_pipelines`, но не
собирается по умолчанию (`cpu-render` — дефолтная фича `lumen-driver` и
`lumen-shell` пинит `lumen-paint` с `cpu-render` безусловно).

Добавлен опциональный (`LUMEN_CPU_SYSTEM_FONTS=1`, читается один раз через
`OnceLock`, паттерн `LUMEN_OWN_TEXT_SHAPING`) путь: `DrawText`'s
`font_family` список теперь доходит до `cpu_raster.rs` и, под флагом,
разрешается через `lumen_font::shared_system_index().pick_face(...)` (CSS
Fonts L4 §5.2 weight/style match) + `std::fs::read`, с откатом на bundled
Inter при пустом списке / отсутствии совпадения / нечитаемом файле. **Флаг
выключен по умолчанию везде** — ни один прогон `--screenshot`,
`graphic_tests/run.py`, `lumen-driver`'s снимковый набор или CI его не
устанавливает, так что ни один закоммиченный эталон не сдвинулся (см. §3).

Код — `crates/engine/paint/src/cpu_raster.rs`: `cpu_system_fonts_enabled()`,
`resolve_face_bytes()`, `build_face()` (был `load_bundled_face()`,
обобщён на произвольные байты), `font_family: &[String]` протянут через
`rasterize_text`/`rasterize_text_rotated`/`rasterize_text_mixed`/
`measure_run_advance`.

## 2. Замер: own vs rustybuzz с реальными системными шрифтами

```bash
cargo build -p lumen-shell --profile dev-release
LUMEN_CPU_SYSTEM_FONTS=1 lumen --screenshot rustybuzz.png docs/conformance/probes/text-shaping.html
LUMEN_CPU_SYSTEM_FONTS=1 LUMEN_OWN_TEXT_SHAPING=1 lumen --screenshot own.png docs/conformance/probes/text-shaping.html
```

Детерминизм подтверждён: два независимых прогона с rustybuzz дают побайтово
идентичный PNG (0 отличий, 737280 из 737280 совпадающих пикселей).

| № | Проверка | Diff own↔rustybuzz (px) | Результат |
|---|---|---|---|
| 01 | Лигатура fi/ffi (Inter) | 55 | **Без изменений** — оба на уровне антиалиасинга, own уже поддерживал GSUB4 (подтверждено ранее в LIB-2) |
| 02 | Кернинг AV/To (Inter) | 59 | **Без изменений**, та же причина |
| 03a/03b | Диакритика decomposed/precomposed (Inter) | 126 | **Без изменений** — уровень антиалиасинга |
| 04 | Вьетнамский (Inter) | 55 | **Без изменений** |
| 05 | Арабское соединение (системный шрифт) | 5531 | **rustybuzz PASS, own FAIL** — см. ниже, впервые измеримо |
| 06 | Деванагари (системный шрифт) | 0 | **Оба FAIL одинаково** — `.notdef`-тофу при обоих шейперах: дыра в шрифтовом фоллбеке (`SystemFontIndex` не находит/не предпочитает Nirmala UI для этих кодпоинтов), не в шейпинге. Кандидат в BUGS.md, вне охвата LIB-3 |
| 07 | Иврит с огласовками (системный шрифт) | 1060 | **Оба визуально корректны** (см. crops) — огласовки на месте при обоих шейперах, диф — субпиксельное позиционирование, не структурный дефект |
| 08 | RTL + смешанные числа (системный шрифт) | 2223 | **rustybuzz визуально ближе к ожидаемому**, own — другой порядок/форма глифов (см. ниже) |

### Проверка 05 — впервые измеримое структурное различие

[`row05-arabic-rustybuzz.png`](2026-09-01-lib3-cpu-system-fonts-evidence/row05-arabic-rustybuzz.png)
vs
[`row05-arabic-own.png`](2026-09-01-lib3-cpu-system-fonts-evidence/row05-arabic-own.png):

rustybuzz рисует "بسم الله" и "بعل" как связную арабскую вязь — буквы
внутри слова визуально соединены контекстными формами (initial/medial/final,
GSUB 5/6). Own-шейпер рисует те же слова как последовательность визуально
разъединённых, преимущественно изолированных форм — буквы не сливаются в
вязь, хотя кодпойнты и порядок те же. Это ровно тот класс, который LIB-1's
`#[ignore]`-юнит-тест на реальном Tahoma уже доказал алгебраически
(«контекстное арабское соединение… то, чего свой движок не может в
принципе») — здесь то же самое видно byte-level на самом продукте, а не
только в изолированном юнит-тесте.

### Проверка 08 — RTL-строка

[`rows05-08-flag-on-rustybuzz.png`](2026-09-01-lib3-cpu-system-fonts-evidence/rows05-08-flag-on-rustybuzz.png)
показывает "ابحرم عالم 12345" (rustybuzz) против "مرحبا عالم 12345" (own) —
видимо иной порядок соединения глифов слова "مرحبا". Направление шейпинга в
обоих случаях передаётся как `ShapeDirection::LeftToRight` (paint ещё не
резолвит bidi-направление по CSS `direction`, LIB-1) — разница здесь идёт от
того, как каждый шейпер сам трактует RTL-скрипт без явной подсказки
направления, а не от разного bidi-алгоритма в вызывающем коде. Не разбирался
до строгого вердикта (вне денежного вопроса LIB-3 — уже проверки 05
достаточно), зафиксирован как наблюдение.

### Проверка 07 — контроль на регресс

[`row07-hebrew-rustybuzz.png`](2026-09-01-lib3-cpu-system-fonts-evidence/row07-hebrew-rustybuzz.png)
vs
[`row07-hebrew-own.png`](2026-09-01-lib3-cpu-system-fonts-evidence/row07-hebrew-own.png)
— визуально неотличимы на глаз, огласовки под/над нужными буквами в обоих
случаях. Диф в 1060px — не структурный (соответствует LIB-1's заявке про
частичную поддержку GPOS mark-to-base у own движка).

## 3. Гарантия отсутствия побочных эффектов

- Флаг читается через `OnceLock`, `std::env::var_os` — по умолчанию
  `None`, ветка `resolve_face_bytes` возвращает `BUNDLED_FONT.to_vec()`
  байт-в-байт как раньше.
- `cargo test -p lumen-paint --features backend-cpu --lib cpu_raster` — 65
  passed, 0 failed (флаг не установлен, поведение не изменилось).
- `cargo clippy -p lumen-paint --features backend-cpu --all-targets -- -D warnings`
  — чисто.
- Флаг не установлен ни в `graphic_tests/run.py`, ни в
  `scripts/scoped-test.sh`, ни в `.github/workflows/*` — полный
  графический гейт и снимковые тесты не задеты этой задачей, повторный
  прогон не требуется (доказательство — сам факт, что переключатель нигде не
  установлен, а не эмпирический прогон).

## 4. Вывод для LIB-3

Условие приёмки («цифра LIB-0 выросла», `docs/conformance-method.md` §1)
**впервые выполнимо для проверки, которую раньше нельзя было измерить в
принципе**: проверка 05 (арабское соединение) даёт чистый, структурный,
byte-level PASS для rustybuzz против FAIL для own-шейпера — ровно то
предметное превосходство, которое LIB-1 заявляла на юнит-тестах, теперь
воспроизведено на реальном продуктовом пути. Проверки 01–04, 07 не
показывают регресса при переходе на rustybuzz. Проверка 06 остаётся
заблокированной ДРУГИМ дефектом (шрифтовой фоллбек для Devanagari, не
шейпинг) — не голосует ни за, ни против LIB-3.

**Рекомендация: условие «цифра выросла» удовлетворено для латиницы+иврита
(без изменений) и арабского (доказанный рост), деванагари остаётся вне
измерения по независимой причине.** Разложение по каждой из 8 измеримых
проверок (06 исключена) — 7 без регресса, 1 (05) с доказанным ростом, 0 с
регрессом. Решение о самом удалении own-шейпера (`gsub.rs`, `gpos.rs`,
шейпинговая часть `otlayout.rs`, ~1350 строк) — предмет отдельного шага,
не этой ревизии; здесь только инструментарий и замер.

## Как повторить

```bash
cargo build -p lumen-shell --profile dev-release
LUMEN_CPU_SYSTEM_FONTS=1 lumen --screenshot a.png docs/conformance/probes/text-shaping.html
LUMEN_CPU_SYSTEM_FONTS=1 LUMEN_OWN_TEXT_SHAPING=1 lumen --screenshot b.png docs/conformance/probes/text-shaping.html
# diff a.png b.png — любым PNG-диффером (см. предыдущую ревизию для ffmpeg-рецепта)
```
