# BUG-059

**Статус:** FIXED 2026-06-04
**Компонент:** font
**Файл:** `crates/engine/font/src/woff2.rs:260`

## Описание

WOFF2-декодер отклоняет шрифты с контурами из 0 точек («woff2: contour with zero points»): все 10 шрифтов CNN (cnn_sans_condensed, cnn_sans_display, helveticaneue, noto_sans_arabic, noto_serif*) не загружаются; пустые глифы (пробел и др.) легальны по спеке и принимаются всеми браузерами — нужно пропускать такой глиф, а не отклонять шрифт целиком
