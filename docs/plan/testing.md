## 15. Тестирование

### 15.1 Пирамида тестов

Lumen использует пять уровней с разной стоимостью и зоной ответственности. Чем выше уровень — тем дороже и реже запуск, тем шире зона покрытия. Реализация автоматизации через `lumen-driver` (§6.11, [ADR-006](docs/decisions/ADR-006-automation-api.md)) — обязательная база для уровней 2-4.

```
┌────────────────────────────────────────────────────────────┐
│ 5. Top sites / WPT — раз в релиз     ~минуты на тест       │
├────────────────────────────────────────────────────────────┤
│ 4. Cross-browser vs Edge — ночной job ~секунды на тест     │
├────────────────────────────────────────────────────────────┤
│ 3. Snapshot pixel in-process — на PR  ~миллисекунды        │
├────────────────────────────────────────────────────────────┤
│ 2. Structural asserts (via lumen-driver) — на cargo test   │
│                                       ~миллисекунды        │
├────────────────────────────────────────────────────────────┤
│ 1. Unit + парсер-тесты — на cargo check ~микросекунды      │
└────────────────────────────────────────────────────────────┘
```

#### Уровень 1 — Unit-тесты и парсер-тесты

- `cargo test` per-crate. Inline `#[test]` + integration tests в `tests/`.
- Парсер-тесты: `html5lib-tests` для HTML, WPT-style для CSS.
- ✅ **Display-list snapshot tests** (legacy уровень 1.5): `serialize_display_list` + 6 golden-файлов в `lumen-paint/tests/snapshots/`. `UPDATE_SNAPSHOTS=1` для регенерации. Остаётся как тонкий слой между unit и in-process pixel snapshot.

#### Уровень 2 — Structural asserts через `lumen-driver` (новое, основной слой)

Через `BrowserSession` trait (§6.11) тест получает структуры **прямо из движка**, без процесса/окна/пикселей. Локализация бага — до поля в `ComputedStyle` или координаты `LayoutBox`.

```rust
#[test]
fn test_05_margin() {
    let mut s = InProcessSession::new();
    s.navigate("file://graphic_tests/05-margin.html");

    let box1 = s.layout_box("#box1").unwrap();
    assert_eq!(box1.margin.top, 16.0);
    assert_eq!(box1.border_box.width, 200.0);

    let style = s.computed_style("#box1");
    assert_eq!(style.background_color, Color::rgb(0xff, 0x00, 0x00));

    let tree = s.a11y_tree();
    assert_eq!(tree.find_by_role("button").unwrap().name, "Submit");
}
```

Бегает на каждый `cargo test` (миллисекунды). Не зависит от шрифтов, GPU, антиалиасинга, ОС.

#### Уровень 3 — In-process pixel snapshot

`session.screenshot()` рендерит в off-screen surface, возвращает `Image` в RAM. Сравнение с PNG-эталоном в `graphic_tests/snapshots/`. Никакого ffmpeg, gdigrab, title bar offsets, calibration TEST-00 — буфер байт-точный.

Для кросс-OS детерминизма (избежать ±1 LSB от GPU драйверов) — software rasterizer (`tiny-skia`, opt-in dep) под `cfg(test)`. См. ADR-006 «Consequences → tiny-skia».

```rust
#[test]
fn test_05_margin_visual() {
    let mut s = InProcessSession::new();
    s.navigate("file://graphic_tests/05-margin.html");
    assert_snapshot!(s.screenshot(), "05-margin.png");
}
```

Файл-эталон коммитится в репо (`graphic_tests/snapshots/*.png`). При несовпадении тест сохраняет `*.actual.png` и `*.diff.png` рядом. Обновление: `cargo test --update-snapshots` (помечается в PR описании).

#### Уровень 4 — Cross-browser vs Edge

Текущая схема (`graphic_tests/run.py`) сохраняется, **но переходит в отдельный ночной CI-job** — не основной gate. Цель — обнаружение «оба дня неправильно одинаково» (когда уровень 3 не ловит, потому что snapshot закрепил баг). Edge как внешний якорь.

#### Уровень 5 — Top 1000 sites + Web Platform Tests

- **WPT subset** — DOM, CSS, fetch. Цель: 60% pass к v1.0.
- **Top sites test** — на каждом релизе автоматический прогон, скриншоты, сравнение с Chromium как baseline.
- **Fuzzing** — 10 минут на PR.

#### Что значит «тестирование пораньше»

Уровни 2 и 3 — это **прямое требование** к Phase 0 (см. §16). Они существуют **для нас самих**: мы пишем `lumen-layout`, мы и тестируем его структурными ассертами, без процесс-запусков и пиксельных сравнений. Это не «отдадим тестерам потом», это «работает уже сейчас, пока движок растёт». Phase 0 не закрыт без них.

### 15.2 CI

GitHub Actions: Linux / macOS / Windows, debug + release, `cargo test` (уровни 1-3) + `cargo clippy -- -D warnings` + `cargo deny` + fuzzing 10 минут на PR. Уровень 4 (cross-browser) — отдельный ночной workflow. Уровень 5 (top sites, WPT) — релизный workflow.

### 15.3 Performance gate

`lumen-bench` (см. §16 Phase 1, §11.4) — обязательный regression-guard в CI для PR-ов, затрагивающих automation, anti-detection, tab lifecycle или сетевые слои. Baseline (`bench/baseline.json`) включает **две оси**: time и RAM.

**Time-axis baseline:** cold start ≤ 300 ms на `samples/page.html`, ≤ 500 ms на `samples/heavy.html`.

**RAM-axis baseline (расширено [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)):**

| Метрика | Baseline |
|---|---|
| T0 simple page (`samples/page.html`) peak RSS | ≤ 100 MB |
| T0 heavy page (`samples/heavy.html`) peak RSS | ≤ 200 MB |
| T2 steady-state RSS per tab | ≤ 25 MB |
| T1 → T0 restore | ≤ 50 ms |
| T2 → T0 restore | ≤ 200 ms |
| T3 → T0 restore | ≤ 1500 ms |

**Правило (binding по [ADR-006](docs/decisions/ADR-006-automation-api.md), [ADR-007](docs/decisions/ADR-007-anti-detection-stack.md), [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)):** PR фейлится в CI при **любом** из условий:

- > 5% регресс time-median или time-p95.
- > 5% регресс peak_rss или steady_state_rss.
- > 20% регресс любого tier-transition restore time.
- Hard budget из таблицы §11.4 превышен.

Это применяется к **default-сборке** без `--mcp` / `--bidi-port` / `--cdp-port` и без Strict / Tor профилей. Default — то, что получает каждый пользователь, и оно должно оставаться лёгким.

Если PR регрессирует:

1. Перенести стоимость за runtime-флаг (транспорт не активен → нулевая стоимость) или за `cargo` feature с `default = false`.
2. Lazy-evaluate (считать только при вызове JS API, не на каждый paint-tick).
3. Снизить интенсивность на Standard, более тяжёлый вариант оставить на Strict.
4. Перевести данные за tier'ную границу (например, dropped image cache при переходе в T2 уже даёт RAM-экономию).
5. Если ни один путь не работает — явное архитектурное обоснование в PR-body и reviewer sign-off.

CI gate (задачи 9G.3 + 9G.5 в Roadmap): `cargo run -p lumen-bench --release` + сравнение time + RAM axes + tier transitions с `bench/baseline.json` → fail при регрессе. Обновление baseline — отдельный коммит с обоснованием (задача 9G.4, процедура в `bench/UPDATE.md`).

---

