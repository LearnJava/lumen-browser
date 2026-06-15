# BUG-036

**Статус:** FIXED 2026-05-26
**Компонент:** layout
**Файл:** `crates/engine/layout/src/style.rs:13479`

## Описание

border-radius: % значения (50%, etc.) не резолвятся → radius=0; только px работает

## Детали

`border-radius: 50%` и любые % значения оставляют радиус = 0.0. Только пиксельные значения (4px, 32px, 999px) работают корректно.

**Корень:** `resolve_box_length()` возвращает `None` для `Length::Percent(_)`:

```rust
fn resolve_box_length(val: &str, em_basis: f32, viewport: Size, is_quirks: bool) -> Option<f32> {
    let len = parse_length_q(val, is_quirks)?;
    match len {
        Length::Percent(_) => None,   // ← здесь баг
        other => other.resolve(em_basis, None, viewport),
    }
}
```

По спеке CSS Backgrounds L3 §5.5: % для border-radius — относительно border-box (ширина для H-радиуса, высота для V-радиуса). Нужно хранить типизированное `Length` и резолвить при layout, когда известен размер бокса.

**Где смотреть:**
- `crates/engine/layout/src/style.rs:13479` — `resolve_box_length`
- `crates/engine/layout/src/style.rs:10684` — применение `border-radius` shorthand
