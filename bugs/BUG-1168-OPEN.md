# BUG-1168 — флейк `bug908_rendered_buffer_differs_across_sessions_when_noise_is_on`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/web_audio.rs`, тест BUG-908)
**Найден:** 2026-09-25, P3, при прогоне `scripts/scoped-test.sh` для BUG-636

## Симптом

В полном прогоне `cargo test -p lumen-js --features v8-backend --lib`
тест упал:

```
assertion `left != right` failed: two sessions must not render a bit-identical buffer
  left: 64.00000154972076
 right: 64.00000154972076
```

Изолированный запуск (`--lib bug908_rendered_buffer_differs`) проходит.

## Предполагаемая причина (не проверена)

Сид шума — `canvas2d::document_noise_seed(origin)`, производный от
процессной сессии (часы стены) и origin. Шум ±1e-7 на сэмпл при значении
0.5 в `Float32Array` квантуется до нескольких ULP (~6e-8), так что суммы
128 сэмплов для двух origin-ов могут совпасть при части сидов — тест
вероятностный, а утверждает детерминированное неравенство. Проверить:
перебрать сессионные сиды и посчитать долю совпадений; либо сравнивать
буферы поэлементно, а не сумму.

## Как повторить

```bash
cargo test -p lumen-js --features v8-backend --lib   # иногда
```
