# BUG-1028: `dom_helpers::import_node` рекурсивен по глубине DOM — латентный потолок на `innerHTML`

**Статус:** FIXED 2026-09-07 (P3)
**Дата:** 2026-09-07
**Компонент:** js (`crates/js/src/v8_runtime/dom_helpers.rs` — `import_node`,
`parse_html_fragment`; вызывается из `install/dom_core.rs`
`_lumen_parse_html_fragment` и ветки `innerHTML`)
**Найден:** 2026-09-07 при разборе [BUG-1027](BUG-1027-FIXED.md) (core dump
процесса, убитого посреди прогона WPT)

## Механизм

`innerHTML` / `outerHTML` / `insertAdjacentHTML` парсят фрагмент в
одноразовый `Document`, после чего переносят его в живой документ узел за
узлом: арены `NodeId` у документов свои, переиспользовать чужой id нельзя.
Перенос — прямая рекурсия:

```rust
for &child in &src.get(src_id).children.clone() {
    let new_child = import_node(dst, src, child);   // ← один кадр на уровень DOM
    dst.append_child(new_id, new_child);
}
```

Глубина рекурсии равна глубине присваиваемого фрагмента, ограничителя нет.
Первый живой след — core dump от 2026-09-07 02:41 (`lumen --bidi-port 44029`,
поток `lumen-v8`, SIGABRT): 56 кадров `import_node` подряд в обрезанном
systemd-трейсе, дно стека не сохранилось.

## Текущее состояние

BUG-1027 дал потоку `lumen-v8` 128 МиБ стека, поэтому **живого падения на
разумной глубине больше нет**: потолок уехал с ~190 уровней (штатные 2 МиБ)
за 10 000. Запись заведена как латентная — в той же роли, в какой заводился
[BUG-1026](BUG-1026-FIXED.md) (закрыт 2026-09-07): дефект существует, но
упереться в него на реальной странице сейчас нельзя.

## Что надо сделать

Перевести `import_node` на явный стек (pre-order обход с собственным
`Vec<(src_id, dst_parent)>`), как [LAYOUT-1](../ROADMAP.md) сделал с
обходами layout. Форма здесь простая — чистый pre-order без пост-обработки,
то есть тот самый «безопасный механический класс» из LAYOUT-1, а не сложный
случай LAYOUT-2. В объём LAYOUT-1/LAYOUT-2 эта функция не входила: те задачи
описаны как layout/paint/style-проходы и сериализаторы, `crates/js` в них не
упомянут.

Рядом стоит посмотреть на `serialize_node`/`serialize_children` в том же
файле — сериализация `innerHTML` рекурсивна ровно так же.

## Исправление 2026-09-07 (P3)

Обе функции переведены на явный heap-стек — тот же safe mechanical класс,
что LAYOUT-1 уже применил в `crates/engine/layout`.

**`import_node`** — LIFO-стек пар `(src_id, dst_parent)`. Вынесенный
`clone_one` создаёт узел без attach и возвращает `(new_id, has_children)` —
второе поле нужно, чтобы точно повторить старое поведение: узел типа
Doctype/Document/ShadowRoot/DocumentFragment (недостижим в реальном
`innerHTML`-фрагменте, но раньше обрабатывался ранним `return` без обхода
детей) не должен потом попасть в цикл со своими потомками — простое условие
`if has_children { stack.extend(...) }` вместо раннего `return`. Форма
чистая pre-order: ничего не читается из результата ребёнка, кроме его
нового id, который сразу уходит в `append_child`.

**`serialize_node`** — двухфазный `Frame::Open`/`Frame::Close`, потому что
закрывающий тег элемента пишется в `out` только после всех его потомков
(в отличие от `import_node`, это не «голый» pre-order). Каждый `Open`
элемента при обходе пишет открывающий тег, кладёт свой `Close(tag)` на
стек, затем детей в обратном порядке (LIFO сохраняет document order),
`Close` при снятии со стека дописывает закрывающий тег.
`serialize_children` не менялся — рекурсии в нём не было, только цикл по
топ-level детям, каждый из которых теперь вызывает уже нерекурсивный
`serialize_node`.

**Тесты:** два regression-теста на цепочке из 20 000 вложенных `<div>`
(`v8_runtime::dom_helpers::tests::import_node_deep_chain_does_not_overflow_the_stack`,
`serialize_node_deep_chain_does_not_overflow_the_stack`). Глубина сознательно
меньше конвенции LAYOUT-1 (200 000): построение цепочки идёт через
`Document::append_child`, чей debug_assert цикл-чек (`is_self_or_ancestor`)
проходит всех предков на каждый вызов — O(глубина) — что делает построение
цепочки O(N²) под `cargo test` (`debug_assertions` включены даже в
оптимизированном `test`-профиле). 200 000 замерено — ~15 минут на пару
тестов; 20 000 даёт ~3 с и всё ещё вдвое больше живого потолка ~10 000,
которым этот же баг измерил `lumen-v8` поток после BUG-1027 — с большим
запасом для голой Rust-рекурсии без V8/JS-кадров на стеке.

`cargo test -p lumen-js --lib --features v8-backend`: 3540/3540 (+2),
включая уже существующие `innerHTML`/`outerHTML`/`Document.importNode`
тесты — не сломаны. `cargo clippy -p lumen-js --all-targets --features
v8-backend --no-deps -- -D warnings`: чист (полный `--all-targets` без
`--no-deps` красит несвязанные `lumen-image`/`lumen-font` — расхождение
системного rustc 1.98.0 с пином 1.97.0 на этой Linux-машине, не наш диф).
