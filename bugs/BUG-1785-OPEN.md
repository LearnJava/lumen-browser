# BUG-1785 — cssText: border-image сериализует `url("a.png")`, тест BUG-1552 ждёт `url(a.png)` — конфликт канонов url()

**Статус:** OPEN
**Заведён:** 2026-11-04 (воркер 1 zshift, найден в проверке проекта задачи VTT-RENDER-S2 — к ней не относится)
**Область:** layout/style (`crates/engine/layout/src/style/values/border_image.rs:151,502`) vs тест `crates/js/src/dom/tests/v8_bug1552_style_float_shorthand.rs:67`

## Симптом

Тест `dom::tests::v8_bug1552_style_float_shorthand::css_float_and_border_shorthand_round_trip`
(введён фиксом [BUG-1552](BUG-1552-FIXED.md), e0c239761, 2026-10-27) падает на `main`.
Полное сравнение фактической и ожидаемой строк cssText (34 сегмента, механический diff
по логу прогона 2026-11-04) даёт **ровно одно** расхождение:

```
actual  : imageRaw=border-image: url("a.png") 30 / 10px round;
expected: imageRaw=border-image: url(a.png) 30 / 10px round;
```

Все остальные сегменты (cssFloat, сворачивание `border` в шорткат, `!important`,
n-сторонние `border-width`, `border-image: none`) посимвольно совпадают.

## Локализация (проверено по коду)

`border_image.rs:151` и `:502` сериализуют `BackgroundImage::Url` как
`url("…")` — с кавычками и экранированием `\` и `"`. Комментарий в
`background_cssom.rs:93` ([BUG-1551](BUG-1551-FIXED.md), 0406a3d947) называет
каноном ту же форму: `url(a)` / `url('a')` → `url("a")`. Тест BUG-1552 — написан
в тот же день, что и border-image-слив — ждёт `url(a.png)` **без** кавычек.
Итого в дереве два несовместимых канона сериализации url(), и старый тест
закрепляет устаревший.

## Падает на чистом main — проверено

2026-11-04, слот zs1-work: stash-эксперимент (правки VTT-RENDER-S2 сняты,
дерево `d107c6571` + строка TASKS.md) — exit 101 и с правками, и без.
Правки S2 не касаются cssText-сериализации.

## Воспроизведение

```
cargo test -p lumen-js --features v8-backend --lib v8_bug1552    # exit 101
```

## Подозрение на виновника (частично проверено)

Однодневный узел 2026-10-27: e0c239761 (тест BUG-1552, ждёт без кавычек) и
f60614cb3 (слив p4-border-image, BUG-492), далее 207e74f32f (PAINT-WORKLET-2,
2026-10-28 — переписывал сериализацию border-image). Какой коммит ввёл кавычки
конкретной строкой — не установлено (не проверено); установлено само расхождение
и его носители (см. Локализацию).

## Что делать

Выбрать один канон url()-сериализации и свести к нему обе стороны: если канон —
CSSOM/Chrome (`style.cssText` отдаёт `url("a.png")`, кавычки с экранированием),
обновить ожидание теста BUG-1552; если без кавычек — править
`border_image.rs:151,502` и разбираться с канонизацией BUG-1551. Решение —
за владельцем домена после сверки с Chrome.

## Как проверить фикс

`cargo test -p lumen-js --features v8-backend --lib v8_bug1552` зелёный;
`cargo test -p lumen-layout border_image` (существующие тесты сериализации)
не краснее.
