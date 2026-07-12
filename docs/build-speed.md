# Ускорение компиляции Lumen

Документ-план по ускорению сборки workspace (22 крейта, ~433 уникальных зависимости, Windows 10 Pro 19045, MSVC, stable Rust 1.96). Составлен 2026-07-11 по результатам веб-исследования (состояние экосистемы на середину 2026) и аудита текущей конфигурации. Каждый пункт — кандидат на A/B-замер по протоколу из §2.

Статусы: ⬜ не внедрено · 🧪 внедрить и замерить · ✅ уже внедрено · 🚫 отвергнуто.

---

## 1. Что уже сделано (базовая линия)

| Мера | Где | Эффект (замерен ранее) |
|---|---|---|
| ✅ sccache (`rustc-wrapper`) | `.cargo/config.toml` | тёплый кэш ~2× (6.4с→3.0с); кэш `D:\sccache-cache`, лимит 50 GiB |
| ✅ rust-lld вместо link.exe | `.cargo/config.toml` | линковка 2–5× быстрее |
| ✅ `[profile.dev] opt-level = 1` + deps `opt-level = 3` | `Cargo.toml` | быстрый инкремент своего кода, быстрый рантайм зависимостей |
| ✅ Профиль `dev-release` (cgu=8, без LTO) | `Cargo.toml` | 2–3× быстрее `--release` |
| ✅ Консолидация 64 driver-тестов в 1 бинарь (BT-1) | `crates/driver/tests/all.rs` | `test -p lumen-driver --no-run`: 1м50с → 7.6с (~14×) |
| ✅ `scripts/scoped-test.sh` — тесты только затронутых крейтов | `/lumen-task-finish` шаг 2 | вместо 30-минутного `test --workspace` |
| ✅ Правило «всегда `-p <crate>`, не `--workspace`» | CLAUDE.md | — (но см. §3.4 про побочный эффект) |
| ✅ `debug = "line-tables-only"` (dev) + `debug = false` (deps); профиль `debugging` для отладчика | `Cargo.toml` | S1 6.25с→4.24с; S3 тёплый 12м49с→8м25с; relink driver-тестов 7.6с→2.8с (§3.1, замер 2026-07-12) |

Известные факты о workspace:
- `cargo test --workspace` до BT-1 = ~110 линковок; линковка и сборка доминируют, прогон тестов — секунды.
- Корневой `target/` = **38 GiB**; каждый worktree держит свой `target/` (свои полные копии артефактов).
- Зависимости с C-кодом: `rusqlite bundled` (амальгама SQLite), `rquickjs` (QuickJS) — компилируются `cc` на каждой чистой сборке.
- Расхождение фич `lumen-paint` между потребителями: shell = `cpu-render`+`backend-femtovg`+`backend-wgpu`, driver = `backend-wgpu`, js = `backend-wgpu` (через `webgpu`) → сборки `-p` с разными наборами фич перекомпилируют paint-стек (§3.4).
- Дубликаты в дереве: `windows` 0.54/0.58, `glow` 0.13/0.16, `thiserror` 1/2, `bitflags` 1/2, `hashbrown` ×4, `webpki-roots` 0.26/1.0 — каждый дубль компилируется дважды.

---

## 2. Протокол замера (перед любым изменением)

Одно изменение — один замер. Сценарии гоняем из этого worktree, время — `time` в Git Bash, по 2 прогона (второй — чистый от шумов ФС).

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"

# S1: инкрементальная пересборка после правки (главный сценарий разработки)
touch crates/shell/src/main.rs && time cargo build -p lumen-shell

# S2: cargo check после правки глубокого крейта (каскад layout→paint→shell)
touch crates/engine/layout/src/lib.rs && time cargo check -p lumen-shell

# S3: чистая сборка (цена нового worktree)
cargo clean && time cargo build -p lumen-shell

# S4: сборка тест-бинарей (цена гейта)
time cargo test -p lumen-driver --no-run

# S5: профиль узких мест (не время, а структура)
cargo build -p lumen-shell --timings   # → target/cargo-timings/cargo-timing.html
```

Дополнительно: `sccache --show-stats` до/после S3 (hit rate), `cargo tree --duplicates`.

Замеры фиксировать в таблицу в конце этого файла (§7).

---

## 3. Ярус 1 — stable, низкий риск, внедрять первыми

### 3.1 ✅ Отключить лишний debuginfo (замерено 2026-07-12: S1 −32%, S3 −34%, relink тестов −63%)

Сейчас `[profile.dev]` использует дефолт `debug = 2` (полный debuginfo). Генерация debuginfo — главная скрытая стоимость dev-профиля; это же — основная работа линкера при сборке PDB. Официальная рекомендация Cargo Book (2025) и замер Kobzol (май 2025): −20–40% на инкрементальных пересборках, даже поверх lld.

```toml
# Cargo.toml
[profile.dev]
opt-level = 1
debug = "line-tables-only"   # file:line в backtrace остаются, остальное — нет

[profile.dev.package."*"]
opt-level = 3
debug = false                # зависимостям debuginfo не нужен вовсе

# Отдельный профиль для реальной отладки под дебаггером:
[profile.debugging]
inherits = "dev"
debug = true
```

Риск: пошаговая отладка в отладчике требует `--profile debugging`. Backtrace при панике сохраняет файл:строку.
Источники: [Cargo Book: build-performance](https://doc.rust-lang.org/cargo/guide/build-performance.html), [Kobzol: disable debuginfo](https://kobzol.github.io/rust/rustc/2025/05/20/disable-debuginfo-to-improve-rust-compile-times.html).

### 3.2 🧪 Исключения Windows Defender (ожидание: −30–60% на инкременте)

Крупнейший Windows-специфичный оверхед: `MsMpEng.exe` синхронно сканирует каждый файл, который создаёт rustc/линкер, а `target/` — это тысячи мелких файлов за сборку. Требует «прогона» пользователем (elevated PowerShell) — вне полномочий ассистента:

```powershell
Add-MpPreference -ExclusionPath "$env:USERPROFILE\.cargo"
Add-MpPreference -ExclusionPath "$env:USERPROFILE\.rustup"
Add-MpPreference -ExclusionPath "D:\RustProjects"
Add-MpPreference -ExclusionPath "D:\sccache-cache"
Add-MpPreference -ExclusionProcess "$env:USERPROFILE\.rustup\toolchains\*\bin\rustc.exe"
Add-MpPreference -ExclusionProcess "$env:USERPROFILE\.cargo\bin\sccache.exe"
# проверка:
$p = Get-MpPreference; $p.ExclusionPath; $p.ExclusionProcess
```

Риск: каталоги исключены из real-time-сканирования — не скачивать в них чужие бинари (архивы в корне и так gitignored).
Источники: [cargo#5028](https://github.com/rust-lang/cargo/issues/5028), [Microsoft Learn: Add-MpPreference](https://learn.microsoft.com/en-us/powershell/module/defender/add-mppreference).

### 3.3 🧪 Кэшировать C-компиляцию (SQLite, QuickJS) через sccache

`libsqlite3-sys bundled` и `rquickjs-sys` пересобирают C-код на каждой чистой сборке (каждый новый worktree!). Крейт `cc` официально поддерживает sccache-обёртку:

```toml
# .cargo/config.toml, секция [env]
CC = "sccache cl"
CXX = "sccache cl"
```

C-исходники лежат в `~/.cargo/registry/src/...` — путь стабилен между worktree, значит кэш-хиты работают между всеми копиями без настройки `SCCACHE_BASEDIRS`. Проверка: два `cargo clean && cargo build` подряд, во втором `sccache --show-stats` должен показать C/C++ hits.

Риск: sccache должен находить `cl.exe`; если сборка идёт вне VS-окружения и `cc-rs` находит MSVC сам, обёртка может не сработать — тогда откатить.
Источники: [cc crate docs](https://docs.rs/cc/latest/cc/), [sccache README](https://github.com/mozilla/sccache).

### 3.4 🧪 cargo-hakari (workspace-hack) — убрать фич-трэшинг `-p`-сборок

Проектное правило «всегда `-p <crate>`» имеет цену: набор фич зависимостей резолвится **от того, что собираешь** ([cargo#4463](https://github.com/rust-lang/cargo/issues/4463)). Конкретно у нас: `-p lumen-driver` (paint = `backend-wgpu`) после `-p lumen-shell` (paint = `cpu-render,backend-femtovg,backend-wgpu`) перекомпилирует paint-стек, и наоборот — постоянная взаимная инвалидация в общем `target/`. То же между `clippy --workspace` (гейт) и дневными `-p`-сборками.

Решение на stable (рекомендация индустрии и в 2026): `workspace-hack` крейт, который пришпиливает объединённый набор фич:

```bash
cargo install cargo-hakari
cargo hakari init workspace-hack
cargo hakari generate
cargo hakari manage-deps      # добавит workspace-hack в каждый member
cargo hakari verify
```

Ожидание: до 100× на отдельных сценариях «переключение между `-p`», ~1.7× кумулятивно (данные Oxide/hakari docs). Cargo.lock у нас закоммичен, resolver=3 — требования выполнены. Публикации крейтов нет — главный минус hakari нас не касается.

Требует: новый крейт в workspace (см. `/lumen-new-crate`), обоснование зависимости по §5-политике (категория: инструментальная, dev-инфраструктура).
Альтернатива на nightly — `-Zfeature-unification=workspace` ([RFC 3692](https://rust-lang.github.io/rfcs/3692-feature-unification.html), ещё unstable).
Источники: [cargo-hakari about](https://docs.rs/cargo-hakari/latest/cargo_hakari/about/index.html), [nickb.dev: feature unification pitfall](https://nickb.dev/blog/cargo-workspace-and-the-feature-unification-pitfall/).

### 3.5 🧪 NTFS-тюнинг (дёшево, безопасно)

```bat
:: elevated cmd; сначала посмотреть текущие значения
fsutil behavior query disablelastaccess
fsutil behavior set disablelastaccess 1   :: убирает metadata-write на каждый доступ к файлу
fsutil behavior query disable8dot3
fsutil behavior set disable8dot3 1        :: отключить генерацию коротких имён (НЕ strip существующих!)
```

На Win10 1803+ lastaccess часто «system managed» и включён — проверить обязательно. Эффект малый, но бесплатный. `fsutil 8dot3name strip` на C: **не делать** (инсталляторы хранят короткие пути в реестре).
Источник: [fsutil behavior](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/fsutil-behavior).

### 3.6 🧪 Отдельный target-dir для rust-analyzer (если используется IDE)

rust-analyzer и терминальный cargo делят lock на `target/` и вытесняют артефакты друг друга (у нас уже есть готча «фантомные ошибки от общего target»). Если на машине запущен VS Code/RustRover с RA:

```jsonc
// .vscode/settings.json
{
  "rust-analyzer.cargo.targetDir": true,        // → target/rust-analyzer
  "files.watcherExclude": { "**/target/**": true }
}
```

Сейчас `.vscode/` в репо нет — пункт актуален, только если IDE реально используется.
Источник: [rust-analyzer#10684](https://github.com/rust-analyzer/rust-analyzer/issues/10684).

---

## 4. Ярус 2 — stable, требует правок кода/структуры

### 4.1 🧪 Дожать консолидацию integration-тестов (паттерн BT-1)

Каждый файл в `tests/` = отдельный бинарь = отдельная полная линковка. После BT-1 (driver) остались: font — 5 файлов, js — 4, image — 4, paint — 3, network/layout/a11y — по 2. Итого ~15 лишних линковок. Паттерн тот же: `tests/all.rs` + `mod cases;`, файлы в `tests/cases/`. Прецедент замера: у Cargo самого — сборка тест-суита −3×, диск −5× ([matklad: Delete Cargo Integration Tests](https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html)).

Эффект меньше, чем у BT-1 (крейты легче driver), но линкуется каждый на полном стеке движка. Мерить S4-аналогом по каждому крейту.

### 4.2 🧪 Феатуре-диета для тяжёлых зависимостей

- **wgpu 26**: бэкенды феатуре-гейтятся с v24. Сейчас берём дефолт = `dx12+gles+metal+vulkan+webgpu+wgsl`. На Windows достаточно `default-features = false, features = ["dx12", "wgsl"]` — из дерева уходят **ash** (Vulkan, дорогой в сборке) и **glow** (GL). Решить: нужен ли Vulkan-fallback в рантайме (если да — оставить `vulkan`).
- **image/кодеки**: `lumen-image` уже точечно включает `avif` — проверить `cargo tree -e features -i image`, не притащен ли лишний хвост кодеков.
- Проверить `rodio`/`cpal` фичи (rodio уже урезан до 4 форматов — ок).
- `cargo tree --duplicates`: `windows` 0.54+0.58, `glow` 0.13+0.16, `thiserror` 1+2, `bitflags` 1+2 — посмотреть, какие наши прямые зависимости можно бампнуть, чтобы схлопнуть дубли (каждый дубль = двойная компиляция).

Инструменты: `cargo machete` (быстрый поиск неиспользуемых зависимостей, вписывается в P5 `/lumen-health-check deps`), `cargo tree -e features -i <crate>` («кто включил фичу»).
Источники: [wgpu#6949](https://github.com/gfx-rs/wgpu/pull/6949), [Cargo Book: timings checklist](https://doc.rust-lang.org/cargo/reference/timings.html).

### 4.3 🧪 `incremental = true` в dev-release

`dev-release` наследует release → инкремент выключен, каждая пересборка под graphic_tests почти полная. Инкремент в release-подобном профиле: пересборки в 1.4–5× быстрее, первая сборка ~+10%, рантайм ~−1–2% (для тестового профиля несущественно):

```toml
[profile.dev-release]
inherits = "release"
lto = false
codegen-units = 8
strip = false
debug = false
incremental = true    # ← новое
```

Источник: [rust#57968](https://github.com/rust-lang/rust/issues/57968).

### 4.4 ⬜ Понизить deps до `opt-level = 2` (только если чистые сборки болят)

`[profile.dev.package."*"] opt-level = 3` платится один раз на чистую сборку (инкремент не трогает), но с ~10 worktree чистые сборки случаются часто. `opt-level = 2` компилируется заметно быстрее при почти том же рантайме. **Против**: для браузера рантайм зависимостей (растеризация, кодеки, крипто) критичен — сначала замерить S3 с 2 vs 3 и посмотреть на graphic_tests-тайминги. Низкий приоритет.

### 4.5 ⬜ Форма DAG: следить, а не перестраивать

Cargo пайплайнит по `.rmeta`: правка **тела** функции в `lumen-layout` не перетайпчекивает потребителей — только релинк; правка **сигнатур/API** каскадит. Вывод: горячие экспериментальные правки держать за стабильными интерфейсами (обход через `lumen-core::ext` трейты уже это делает). Хвост `layout → paint → shell` последователен по природе — дробление слоёв даст параллелизм, но это архитектурное решение, не тюнинг (Feldera: 1106 крейтов = 30мин→3мин, но нам не нужно).

---

## 5. Ярус 3 — nightly / экспериментальное (отдельная ветка, не в main)

| Мера | Статус экосистемы | Ожидание | Риск |
|---|---|---|---|
| ⬜ `-Zthreads=8` (параллельный фронтенд) | nightly; стабилизация идёт (MCP #1005, интерфейс будет `rustc -j`) | −20–30% общего времени | **плохо сочетается с sccache** (замеры NeoSmart: до +50% с ним); только nightly |
| ⬜ Cranelift-бэкенд для dev | nightly; x86_64 Windows поставляется, но **panic=abort** на Windows (unwinding только Linux) | −20% codegen | `catch_unwind` меняет поведение; нет debuginfo переменных; несовместим с dylib-трюком на Windows |
| ⬜ `hint-mostly-unused` для `windows`-крейта | nightly, call-for-testing июль 2025 | у авторов: release 4м32с→2м06с на одном большом API-крейте | `windows` у нас транзитивный (0.54+0.58) — применимость под вопросом |
| ⬜ `-Zfeature-unification=workspace` | nightly ([cargo#14774](https://github.com/rust-lang/cargo/issues/14774)) | то же, что hakari, без крейта-хака | nightly-only; hakari решает то же на stable |
| ⬜ dylib-трюк (Bevy-style `lumen_dylib`) | stable, но **хрупко на MSVC** | релинк при итерации почти исчезает | LNK2019 на statics, CRT-mismatch с C-кодом (QuickJS/SQLite!), гигантские таблицы экспортов |

Что **мониторить** (может отменить ручную работу): стабилизация параллельного фронтенда; rust-lld как дефолт MSVC ([rust#71520](https://github.com/rust-lang/rust/issues/71520)); **Cargo cross-workspace build cache** ([цель 2026](https://rust-lang.github.io/rust-project-goals/2026/cargo-cross-workspace-cache.html)) — нативно решит проблему «N worktree × одинаковые зависимости»; wild-линкер (порт на Windows в roadmap v0.9+).

---

## 6. Отвергнуто (не тратить время)

| Мера | Почему |
|---|---|
| 🚫 RAM-диск для `target/` | Замеры: 0–3% на SSD; **ImDisk ломает cargo** (rust#90780: "failed to build archive"); rustc CPU-bound |
| 🚫 Перенос `target/` на другой SSD | Нет замеров с ускорением; помогает только HDD→SSD |
| 🚫 Dev Drive (ReFS) | **Только Windows 11** (22621+); на 19045 недоступен. Аргумент при апгрейде: +20–30% |
| 🚫 cargo-nextest ради скорости | Уже проверено на этом workspace (2026-06-21): ускоряет прогон (и так секунды), линковку не режет |
| 🚫 cachepot | Мёртв (форк sccache, заброшен на 0.1.0-rc.1) |
| 🚫 Precompiled proc-macros / watt | serde откатил precompiled в 1.0.184; механизма в Cargo нет; кэширование макрорасширений в rustc не шипнуто |
| 🚫 Общий `CARGO_TARGET_DIR` на все worktree | target-lock сериализует параллельные сессии P1–P5 + готовая готча «фантомные ошибки»; наш ответ — sccache |

---

## 7. План тестирования и журнал замеров

Порядок внедрения (по ожидаемому эффекту / трудозатратам): **3.1 → 3.2 → 3.3 → 3.5 → 3.4 → 4.3 → 4.1 → 4.2**. После каждого шага — сценарии S1–S4 из §2, результат в таблицу. Изменения 3.1/3.3/4.3 — правки конфигов в ветке; 3.2/3.5 — действия пользователя (elevated); 3.4/4.1/4.2 — отдельные задачи с ветками.

| Дата | Изменение | S1 инкремент | S2 check каскад | S3 clean | S4 test --no-run | Примечание |
|---|---|---|---|---|---|---|
| 2026-07-12 | Базовая линия (до изменений) | 6.25с | 6.23с | 12м49с (тёплый sccache, 81.7% hit) | 5м32с первая / 0.72с no-op | worktree `build-speed-experiments`, свежий target; первый `cargo check` в worktree = 5м19с (свой кэш check-артефактов) |
| 2026-07-12 | 3.1 debuginfo: dev=`line-tables-only`, deps=`false` | **4.24с (−32%)** | 4.9–7.9с (шум, без изменений — check без codegen) | **8м25с (−34%,** тёплый sccache 92.8% hit) | 5м57с первая (холодный кэш новых флагов); **relink после touch driver = 2.82с** (до BT-1-замер давал 7.6с, −63%) | Одноразовая цена внедрения: пересборка мира под новые флаги = 18м52с (sccache-кэш под новые флаги пуст). Выигрыш = codegen debuginfo + PDB-линковка |

Готча замера: `[env] SCCACHE_SERVER_PORT=4150` из `.cargo/config.toml` действует только на cargo-процессы — CLI-вызовы `sccache --show-stats`/`--zero-stats` без `SCCACHE_SERVER_PORT=4150` уходят на другой сервер (дефолтный порт) и показывают нули.

Правила замера: закрыть фоновые cargo-процессы других сессий; sccache-статистику снимать `sccache --zero-stats` перед сценарием; каждый сценарий ×2, берём второй прогон; S3 мерить и с холодным (`sccache --stop-server; SCCACHE_RECACHE=1`), и с тёплым кэшем.
