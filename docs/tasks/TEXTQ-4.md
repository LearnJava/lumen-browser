# TEXTQ-4 — шейпинг (кернинг, лигатуры) в GPU-пути отрисовки

Дорожка TEXTQ, срез 4. Владелец — P1. Крейт — `lumen-paint`. **Зависит от:** TEXTQ-1.

## Зачем

Живое окно не шейпит текст: `push_text_glyphs` ([glyph_raster.rs:501](../../crates/engine/paint/src/renderer/glyph_raster.rs#L501)) берёт глиф каждого символа из `cmap` по одному и двигает перо на advance из `hmtx`; `font_features` в `DrawText` игнорируется ([renderer.rs:1667](../../crates/engine/paint/src/renderer.rs#L1667), `font_features: _`). Нет кернинга (`AV`, `To`, `Ty` разъезжаются), лигатур, `font-feature-settings`, контекстных форм. CPU-путь уже шейпит через `lumen_font::active_text_shaper()` (rustybuzz) — [cpu_raster.rs:3336](../../crates/engine/paint/src/cpu_raster.rs#L3336); GPU должен делать то же.

## Что сделать

1. В `push_text_glyphs` и `push_text_glyphs_mixed` разбить текст на под-run-ы по резолвленному face (`pick_face_for_codepoint`, fallback по символам остаётся как есть), каждый под-run — через `active_text_shaper().shape(bytes, text, LeftToRight, None, font_features, coords)`.
2. Позиции брать из шейпера: `x_advance`, `x_offset`, `y_offset` × `advance_scale`; глиф — `glyph_id` из шейпера (а не из `cmap`). `letter-spacing` добавлять на кластер, а не на символ (CSS Text 3 §8.2); при `letter-spacing ≠ 0` отключать необязательные лигатуры (`liga`, `clig`), как требует спека.
3. Протянуть `font_features` из `DrawText` в glyph-код (сейчас отбрасывается).
4. **Кэш шейпинга.** GPU-путь перестраивает текст каждый кадр — шейпить каждый кадр нельзя. Кэшировать результат по `(face_id, text, size_q, features, coords_hash)`; посмотреть существующий `TextRunCache` в `glyph_raster.rs` и встроиться в него. Замер: время кадра на текстовой странице до/после — регрессия не больше 5 % (`[wgpu] adapter` в stderr, бэкенд зафиксировать через `WGPU_BACKEND`).
5. Ширины раскладки (layout) не меняются в этом срезе — нарисованная строка может отличаться по ширине от бокса на величину кернинга, как уже сейчас в CPU-пути. Выравнивание по ширинам раскладки — TEXTQ-5.

## Готово, когда

1. Юнит-тест: строка `AVATAR` в Inter (есть пары кернинга) даёт позиции, отличные от суммы advance-ов, и совпадающие с CPU-путём; `font-feature-settings: "liga" 0` убирает лигатуру `fi`.
2. Страница качества текста (TEXTQ-1) снята до/после, diff% против Edge — в ROADMAP-строке.
3. Замер времени кадра до/после записан в ROADMAP-строку.
4. Полный `python graphic_tests/run.py --continue-on-fail`, затронутые эталоны перегенерированы в том же коммите.
5. `cargo clippy -p lumen-paint --all-targets -- -D warnings` чисто.
