# BUG-227

**Статус:** OPEN
**Компонент:** paint (ICC — домен P1)
**Тесты:** `color_management::tests::detects_p3_from_description`,
`color_management::tests::detects_rec2020_from_description`

## Описание

Два юнит-теста в `crates/engine/paint/src/color_management.rs` (строки 62 и 84)
красные на main. Они строят синтетический ICC-профиль с RGB-сигнатурой и текстовым
описанием («Display P3» / «Rec2020») и ожидают, что
`detect_color_space_from_icc(&profile)` вернёт `ColorSpace::DisplayP3` /
`ColorSpace::Rec2020`. После рефактора ICC-инфраструктуры в `lumen-core`
детектор перестал распознавать color space по text-описанию → возвращает другое
значение, `assert_eq!` падает.

Регрессия внесена не BUG-226 (он трогает только `display_list.rs::emit_svg_shape`),
а ICC-работой другой сессии:
- X-1 «Color management — ICC profile parsing» (`68927090`)
- H-2 Phase 3 «ColorSpace в lumen-core» (`23014125`)

Подтверждено: тесты красные на чистом main (`d30183a6`) до merge p3-bug-226.

## Воспроизведение

```bash
cargo test -p lumen-paint --lib color_management
# detects_p3_from_description ... FAILED
# detects_rec2020_from_description ... FAILED
```

## Как чинить

Либо вернуть text-description-fallback в `detect_color_space_from_icc` (после
парсинга tags сканировать `desc`-тег на «Display P3»/«Rec2020»), либо ре-базлайнить
тесты под новое поведение ICC-парсера, если детект по описанию намеренно убран.
Решение за владельцем ICC (P1).
