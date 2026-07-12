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
| ✅ BT-1 распространён на font/js/image/paint/network/layout/a11y (§4.1) | `crates/**/tests/all.rs` + `tests/cases/` | 22 интеграционных бинаря → 7 (−15 линковок); замер 2026-07-12 |
| ✅ `scripts/scoped-test.sh` — тесты только затронутых крейтов | `/lumen-task-finish` шаг 2 | вместо 30-минутного `test --workspace` |
| ✅ Правило «всегда `-p <crate>`, не `--workspace`» | CLAUDE.md | — (но см. §3.4 про побочный эффект) |
| ✅ `debug = "line-tables-only"` (dev) + `debug = false` (deps); профиль `debugging` для отладчика | `Cargo.toml` | S1 6.25с→4.24с; S3 тёплый 12м49с→8м25с; relink driver-тестов 7.6с→2.8с (§3.1, замер 2026-07-12) |
| ✅ `incremental = true` в dev-release | `Cargo.toml` | touch shell 47с→3.8с; каскад layout→shell 50с→8.6с (§4.3, замер 2026-07-12) |
| ✅ Фич-диета wgpu: `dx12+wgsl+std`, дефолт-фичи off (§4.2) | `Cargo.toml` | −8 крейтов из графа (ash/glow/gpu-alloc/…), схлопнут дубль glow; замер 2026-07-12 |

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

### 3.2 ✅ Исключения Windows Defender (применено 2026-07-12; замеренного выигрыша НЕТ)

**Итог замера 2026-07-12:** S1/S2/S3/S4 без изменений против уровня после 3.1 (в пределах шума). Причина: `D:\RustProjects\lumen-browser` (а значит и все `target/` worktree) **уже был в Defender-исключениях до базовых замеров** — базовая линия изначально снималась без сканирования target. Команды ниже выполнены (elevated, через UAC-самоподнятие): добавлены `.cargo`, `.rustup`, `D:\RustProjects` целиком, `D:\sccache-cache` и процессы rustc/sccache. Покрытие полнее (реестр крейтов, sccache-кэш), вреда нет, но ожидание «−30–60%» не реализовалось — оно уже было учтено в базе.

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

### 3.3 🚫 Кэшировать C-компиляцию (SQLite, QuickJS) через sccache — не работает в нашем окружении

**Проверено 2026-07-12 — откачено.** Сборка идёт из Git Bash без VS-окружения: `cc-rs` находит MSVC сам (vswhere/реестр), но с `CC = "sccache cl"` поиск `cl` делает уже sccache по PATH — и не находит («cannot find binary path»). Передать полный путь нельзя: `cc-rs` сплитит значение CC по пробелам, а путь к cl.exe содержит `Program Files`. Попутная находка: C-код компилирует ещё и `aws-lc-sys` (aws-lc + jitterentropy, десятки .c-файлов) — выигрыш был бы больше ожидаемого, если запускать сборки из VS Developer shell (тогда `cl` в PATH и обёртка сработает). Вариант отложен до смены окружения запуска.

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

### 3.5 ✅ NTFS-тюнинг (применено 2026-07-12; оба параметра уже были в нужном состоянии)

**Итог 2026-07-12:** `disablelastaccess` уже был 2 (system managed, отключён) — закреплён в 1 (user managed, отключён); генерация 8dot3 на D: уже была отключена на уровне тома — закреплена глобально (реестр = 1). Фактическое поведение ФС не изменилось, замеры без изменений (см. журнал §7).

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

### 4.1 ✅ Дожать консолидацию integration-тестов (паттерн BT-1) — внедрено 2026-07-12

Каждый файл в `tests/` = отдельный бинарь = отдельная полная линковка. После BT-1 (driver) оставались: font — 5 файлов, js — 4, image — 4, paint — 3, network/layout/a11y — по 2. Итого ~15 лишних линковок. Паттерн тот же: `tests/all.rs` (`mod cases;`) + файлы-модули в `tests/cases/` + `tests/cases/mod.rs` со списком `mod <файл>;`. Прецедент замера: у Cargo самого — сборка тест-суита −3×, диск −5× ([matklad: Delete Cargo Integration Tests](https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html)).

Реализовано для всех 7 крейтов: 22 интеграционных бинаря → 7 (по одному `all` на крейт), −15 линковок. Готчи внедрения:
- `include_bytes!("fixtures/…")` в `lumen-image` резолвится относительно файла-исходника — при переносе на уровень глубже пути исправлены на `../fixtures/…` (47 путей в 3 файлах).
- Гейтованные `#![cfg(feature = "backend-wgpu")]` модули `lumen-paint` (`headless_tests`, `texture_pool_integration`) переносятся как есть; их `include_bytes!` шрифта поправлен `../../../../` → `../../../../../` (иначе ломается при включённой фиче — при выключенной модуль пуст и ошибка не видна).

Проверка: `cargo check --tests -p <crate>` зелёный на всех 7; `cargo test --no-run` даёт единственный `Executable tests\all.rs` на крейт (a11y/network подтверждено); `cargo test -p lumen-a11y` — 125 интеграционных тестов (оба бывших бинаря) проходят.

### 4.2 🧪 Феатуре-диета для тяжёлых зависимостей

- **wgpu 26 ✅** (применено 2026-07-12, merge — ниже): `default-features = false, features = ["dx12", "wgsl", "std"]`. Из дерева ушли **8 крейтов** — `ash` (Vulkan-байндинги, дорогой build.rs), `glow` 0.16, `gpu-alloc`(+`-types`), `gpu-descriptor`(+`-types`), `khronos-egl`, `spirv` (349→341 уникальных крейта в графе `-e no-dev`). Vulkan-fallback в рантайме признан ненужным: femtovg — основной рендерер, wgpu лишь fallback/WebGPU-бэкенд, а DX12 доступен на любом WDDM-2.0 GPU под Win10/11. **Готча:** wgpu 26 поддерживает `no_std` — снятие дефолт-фич убирает и фичу `std`, из-за чего `PollError` теряет impl `std::error::Error` (E0277 на `device.poll(...)?` в `renderer.rs`); поэтому `std` возвращён в список явно. **Бонус:** удаление glow 0.16 схлопнуло дубль `glow 0.13+0.16` (femtovg держит 0.13) — см. пункт про `cargo tree --duplicates` ниже.
- **image/кодеки ✅**: `lumen-image` уже с `default-features = false, features = ["avif"]` — `cargo tree -e features -i image` показывает только `avif`, лишнего хвоста кодеков нет. Правок не требуется.
- Проверить `rodio`/`cpal` фичи (rodio уже урезан до 4 форматов — ок).
- `cargo tree --duplicates`: `windows` 0.54+0.58, ~~`glow` 0.13+0.16~~ (0.16 убран фич-диетой wgpu — остался только 0.13 у femtovg), `thiserror` 1+2, `bitflags` 1+2 — посмотреть, какие наши прямые зависимости можно бампнуть, чтобы схлопнуть дубли (каждый дубль = двойная компиляция).

Инструменты: `cargo machete` (быстрый поиск неиспользуемых зависимостей, вписывается в P5 `/lumen-health-check deps`), `cargo tree -e features -i <crate>` («кто включил фичу»).
Источники: [wgpu#6949](https://github.com/gfx-rs/wgpu/pull/6949), [Cargo Book: timings checklist](https://doc.rust-lang.org/cargo/reference/timings.html).

### 4.3 ✅ `incremental = true` в dev-release (замерено 2026-07-12: touch shell 47с→3.8с, каскад layout 50с→8.6с)

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

### Состояние и план следующей сессии (обновлено 2026-07-12)

Сделано: **3.1 ✅** и **4.3 ✅** влиты в main (merge `aec62446`), **3.3 🚫** откачено (детали в §3.3), **3.2 ✅ + 3.5 ✅** применены 2026-07-12 — замеренного выигрыша нет (ключевые пути уже были исключены/отключены до базовой линии; детали в §3.2/§3.5). Замеры — в таблице ниже.

Сделано также: **4.1 ✅** — BT-1 распространён на все 7 крейтов (§4.1, merge 2026-07-12), 22 интеграционных бинаря → 7. **4.2 ✅** — фич-диета wgpu (`dx12+wgsl+std`, дефолт-фичи off), −8 крейтов из графа (детали в §4.2, merge 2026-07-12).

Следующая сессия, по порядку:
1. **3.4 cargo-hakari** — отдельная ветка `p1-hakari`; новый крейт `workspace-hack` через `/lumen-new-crate` + обоснование зависимости по §5-политике. **Предусловие:** `cargo install cargo-hakari` (пишет в `~/.cargo/bin`, вне папки браузера — требует согласия пользователя). A/B-замер: `-p lumen-driver` сразу после `-p lumen-shell` (фич-трэшинг paint-стека) до/после.

Памятка: после подтяжки main каждый старый worktree/target один раз пересоберёт мир (~19 мин, sccache-кэш под новые флаги наполняется заново) — это ожидаемо, не баг. Для пошаговой отладки теперь `--profile debugging`.

| Дата | Изменение | S1 инкремент | S2 check каскад | S3 clean | S4 test --no-run | Примечание |
|---|---|---|---|---|---|---|
| 2026-07-12 | Базовая линия (до изменений) | 6.25с | 6.23с | 12м49с (тёплый sccache, 81.7% hit) | 5м32с первая / 0.72с no-op | worktree `build-speed-experiments`, свежий target; первый `cargo check` в worktree = 5м19с (свой кэш check-артефактов) |
| 2026-07-12 | 3.1 debuginfo: dev=`line-tables-only`, deps=`false` | **4.24с (−32%)** | 4.9–7.9с (шум, без изменений — check без codegen) | **8м25с (−34%,** тёплый sccache 92.8% hit) | 5м57с первая (холодный кэш новых флагов); **relink после touch driver = 2.82с** (до BT-1-замер давал 7.6с, −63%) | Одноразовая цена внедрения: пересборка мира под новые флаги = 18м52с (sccache-кэш под новые флаги пуст). Выигрыш = codegen debuginfo + PDB-линковка |
| 2026-07-12 | 3.3 `CC/CXX = "sccache cl"` | — | — | — | — | **Откачено:** вне VS-окружения sccache не находит cl.exe (см. §3.3). Находка: C-код компилирует и aws-lc-sys |
| 2026-07-12 | 4.3 dev-release `incremental = true` | touch shell: 39–47с → **3.8–12с** | — | полная dev-release: 7м57с (для справки) | — | touch layout (каскад до shell): 50с → **8.6–15с**. Одноразово: первая пересборка после смены профиля = 2м42с (только workspace-members). Главный выигрыш — итерация graphic_tests |
| 2026-07-12 | 3.2 Defender + 3.5 fsutil (применены elevated) | 4.22–4.34с (без изм.) | 6.13с (без изм.) | 9м05с (тёплый sccache 92.8% hit; 1-й прогон свежего worktree 10м29с при 81.7%) | 3м47с первая (тёплый кэш) / 0.72с no-op / relink 2.80с | **Эффекта нет — база уже была с исключённым `D:\RustProjects\lumen-browser` и отключённым lastaccess/8dot3.** Worktree `build-speed-32-35`; первый check в worktree = 1м33с |
| 2026-07-12 | 4.1 BT-1 на font/js/image/paint/network/layout/a11y | — | — | — | 1 `all`-бинарь на крейт (было 5/4/4/3/2/2/2) | 22 интеграционных бинаря → 7, −15 линковок. `cargo test --no-run` для a11y/network подтвердил единственный `Executable tests\all.rs`; полный S4-цикл по каждому крейту не гонялся (дорого, эффект структурно эквивалентен BT-1 driver 14×) |
| 2026-07-12 | 4.2 фич-диета wgpu: `dx12+wgsl+std`, дефолт off | — | — | — | — | Структурный замер (`cargo tree -e no-dev`): 349→341 уникальных крейта, −8: ash, glow 0.16, gpu-alloc(+types), gpu-descriptor(+types), khronos-egl, spirv. `ash` — самый дорогой (Vulkan build.rs). Стоп-часный S3 не гонялся (дорого, per-flags холодный sccache шумит; эффект структурно очевиден — как в строке 4.1). `check -p lumen-paint --features backend-wgpu` / `lumen-shell` / `lumen-driver` / `lumen-js --features webgpu` — зелёные |

Готча замера: `[env] SCCACHE_SERVER_PORT=4150` из `.cargo/config.toml` действует только на cargo-процессы — CLI-вызовы `sccache --show-stats`/`--zero-stats` без `SCCACHE_SERVER_PORT=4150` уходят на другой сервер (дефолтный порт) и показывают нули.

Правила замера: закрыть фоновые cargo-процессы других сессий; sccache-статистику снимать `sccache --zero-stats` перед сценарием; каждый сценарий ×2, берём второй прогон; S3 мерить и с холодным (`sccache --stop-server; SCCACHE_RECACHE=1`), и с тёплым кэшем.
