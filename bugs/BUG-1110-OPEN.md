# BUG-1110 — флак `frame_bridge::tests::inaccessible_bridge_mutation_does_not_mark_dirty`

**Статус:** OPEN
**Заведён:** 2026-09-23 (P1, пойман `scripts/scoped-test.sh` на ветке
`p1-bug1108-caret-trylock`, которая `lumen-js` не трогает).
**Область:** js (`crates/js/src/frame_bridge.rs` — `frame_dom_dirty()`,
тесты `frame_bridge::tests`).

## Симптом

В полном прогоне `cargo test -p lumen-js --lib --features v8-backend`
(4173 теста, параллельно) упало:

```
panicked at crates\js\src\frame_bridge.rs:3400:13:
assertion failed: !take_frame_dom_dirty(key)
```

Изолированный прогон того же теста — `ok`.

## Вероятный механизм (по коду, не доказан)

`frame_dom_dirty()` — процесс-глобальный `HashSet<usize>`, ключ — адрес
`Arc<Mutex<Document>>` (`Arc::as_ptr as usize`). Тест, который мутирует
документ через доступный мост и не забирает флаг (или падает/завершается до
`take`), оставляет ключ в сете; после освобождения его `Arc` аллокатор
вправе выдать тот же адрес документу параллельного теста — и
`inaccessible_bridge_mutation_does_not_mark_dirty` видит чужой флаг.
Это же возможно и в продакшене (флаг от закрытого фрейма «наследует» новый
документ по тому же адресу → лишний пересчёт фрейма), но там цена — одна
лишняя перекладка.

## Что сделать

1. Подтвердить: найти тесты, оставляющие флаг без `take_frame_dom_dirty`.
2. Чинить ключ, а не тест: снимать флаг при уничтожении документа/биндинга,
   либо ключевать не адресом, а монотонным id документа.
