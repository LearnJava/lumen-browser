# BUG-227

**Статус:** FIXED 2026-06-21
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

## Резолюция (2026-06-21, ICC-2)

Выбран ре-базлайн тестов. Сниффинг color space по подстроке описания убран
**намеренно** в ICC-1: настоящий ICC-парсер (`lumen_core::icc::IccProfile`)
классифицирует RGB-профили по колорант-примариям (`rXYZ/gXYZ/bXYZ` → xy-хроматичность),
а не по тексту. Синтетический буфер с подстрокой «Display P3»/«Rec2020», без
`'acsp'`-сигнатуры и без реальных тегов — невалидный профиль, корректный результат
для него — `ColorSpace::Srgb` (graceful fallback), не сниффинг.

Тесты `detects_*_from_description` переписаны в `description_text_is_not_sniffed_*`
(`crates/engine/paint/src/color_management.rs`): они теперь проверяют, что текст
описания **не** является сигналом цветового пространства. Возврат строкового
сниффинга отвергнут как регресс к багу, который ICC-1 устранил.

Дополнительно: тот же класс провала был и в `lumen-image`
(`crates/engine/image/src/lib.rs::detect_color_space_with_display_p3_icc`) —
`Image::detect_color_space` делегирует в `lumen_core::detect_color_space_from_icc`,
так что text-only фейк-профиль тоже падал. P3 при заведении бага заметил только
копии в paint. Тест ре-базлайнен в `detect_color_space_description_text_is_not_sniffed`
(→ Srgb). Обе копии закрыты.
