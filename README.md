# Lumen

Приватный, лёгкий, прозрачный браузер на Rust с собственным движком.

> **Lumen** (лат. *свет*, единица светового потока) — показывает пользователю всё, что происходит, и не весит больше, чем нужно.

## Статус

**Phase 0 — прототип.** Цель этапа: открыть простую текстовую статью без стилей. Подробный план — [lumen-plan.md](lumen-plan.md).

## Сборка

Требуется Rust 1.95+ stable (см. `rust-toolchain.toml`) и MSVC Build Tools на Windows.

```bash
cargo check                                       # быстрая проверка
cargo build                                       # debug-сборка
cargo run -p lumen-shell                          # открыть пустое окно
cargo run -p lumen-shell -- samples/page.html     # распарсить HTML, напечатать DOM
cargo test --workspace                            # все тесты
```

## Структура

```
crates/
├── shell/                  бинарь lumen — UI и точка входа
├── common/                 общие типы (URL, ошибки, конфиг)
└── engine/
    ├── html-parser/        парсер HTML5
    ├── css-parser/         парсер CSS
    ├── dom/                DOM-дерево (арена)
    ├── layout/             block + inline layout
    └── paint/              display list → пиксели
```

## Лицензия

MPL-2.0.
