# BUG-078

**Статус:** FIXED 2026-06-11
**Компонент:** layout/paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs`

## Описание

object-fit contain/cover image quality ~13% deviation — same scaling issue as BUG-077; TEST-19: 12.68%. Root cause: femtovg backend (default, ADR-010 RB-9) ignored object_fit/object_position in DrawImage — always stretched the texture over the content box (fill). Fix: draw_image_in_rect computes the placement rect via fit_image_rect (CSS Images L3 §5.5), scissor-clips cover/none overflow to the content box, and resamples (BUG-077 area-avg) against the placed size instead of the box. TEST-19 12.68%->9.05%; the residual is interior resample-kernel divergence (box-average vs Edge bicubic) on high-frequency images — same accepted AA class as the TEST-18 residual after BUG-077, geometry now matches Edge
