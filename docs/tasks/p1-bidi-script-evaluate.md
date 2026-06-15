# Задача: BiDi script.evaluate + script.callFunction (8H.3 subset)

**Developer:** P1  
**Ветка:** `p1-bidi-script-evaluate`  
**Размер:** S (~80 строк + 4 теста)  
**Крейты:** `lumen-shell` (crates/shell/src/bidi/)

---

## Контекст

Текущий BiDi-сервер (`crates/shell/src/bidi/protocol.rs`) обрабатывает
`session.*` и `browsingContext.*`, но возвращает `unknown command` на любую
`script.*` команду. Playwright/Selenium 5 использует `script.evaluate` для
выполнения JS в странице.

Задача: добавить Protocol-level обработчики для `script.evaluate` и
`script.callFunction`. На этом этапе возвращаем детерминированный stub-ответ
(реального JS-выполнения нет — это требует 8A.7 shell-as-driver-client).

---

## Пред-запуск

- [ ] Прочесть `crates/shell/src/bidi/protocol.rs:1–166` — понять формат dispatch
- [ ] Убедиться в наличии `fn dispatch(message: &str, state: &mut BidiState) -> DispatchResult`
- [ ] `git status` — main чист

---

## Шаги

### 1. Создать ветку

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/bidi-script -b p1-bidi-script-evaluate
cd .claude/worktrees/bidi-script
```

### 2. Добавить новые arms в dispatch

Файл: `crates/shell/src/bidi/protocol.rs`

Найти блок `match method {` (~строка 145) и найти arm:

```rust
        other => DispatchResult::single(make_error(
```

**Перед** этим arm вставить:

```rust
        "script.evaluate" => script_evaluate(id, &params, state),
        "script.callFunction" => script_call_function(id, &params, state),
        "script.addPreloadScript" => script_add_preload(id, &params, state),
        "script.removePreloadScript" => script_remove_preload(id, &params, state),
```

### 3. Добавить функции-обработчики

В конец файла, после всех существующих функций (`bc_activate`, `browsing_context_tree`,
`make_success`, `make_error`, `empty_obj`), добавить:

```rust
// ──────────────────────────────────────────
// script.* handlers (BiDi §10)
// ──────────────────────────────────────────

/// `script.evaluate` — выполнить JS expression в browsing context (BiDi §10.2.4).
///
/// Phase 1 stub: проверяет что context существует, возвращает `{type:"undefined"}`.
/// Реальное выполнение требует 8A.7 (shell-as-driver-client).
fn script_evaluate(id: i64, params: &JsonValue, state: &BidiState) -> DispatchResult {
    let ctx_id = params
        .get("target")
        .and_then(|t| t.get("context"))
        .and_then(|v| v.as_str());

    if let Some(ctx_id) = ctx_id {
        if state.find(ctx_id).is_none() {
            return DispatchResult::single(make_error(
                Some(id),
                "no such frame",
                &format!("unknown browsing context: {ctx_id}"),
            ));
        }
    }

    // Phase 1: return undefined stub.
    let mut result = BTreeMap::new();
    result.insert("type".into(), JsonValue::String("undefined".into()));

    let mut outer = BTreeMap::new();
    outer.insert("result".into(), JsonValue::Object(result));
    outer.insert("realm".into(), JsonValue::String("stub-realm".into()));

    DispatchResult::single(make_success(id, JsonValue::Object(outer)))
}

/// `script.callFunction` — вызвать функцию в browsing context (BiDi §10.2.5).
///
/// Phase 1 stub: те же проверки что script.evaluate, возвращает `{type:"undefined"}`.
fn script_call_function(id: i64, params: &JsonValue, state: &BidiState) -> DispatchResult {
    // Same validation + stub response as evaluate.
    script_evaluate(id, params, state)
}

/// `script.addPreloadScript` — зарегистрировать preload script (BiDi §10.2.1).
///
/// Phase 1 stub: возвращает детерминированный script-id без реального хранения.
fn script_add_preload(id: i64, _params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let script_id = state.next_id(0xaaaa);
    let mut result = BTreeMap::new();
    result.insert("script".into(), JsonValue::String(script_id));
    DispatchResult::single(make_success(id, JsonValue::Object(result)))
}

/// `script.removePreloadScript` — удалить preload script (BiDi §10.2.2).
///
/// Phase 1 stub: ACK без реального удаления.
fn script_remove_preload(id: i64, _params: &JsonValue, _state: &mut BidiState) -> DispatchResult {
    DispatchResult::single(make_success(id, empty_obj()))
}
```

> **Важно:** `script_add_preload` берёт `state: &mut BidiState` (не `&BidiState`)
> потому что вызывает `state.next_id()`. Проверь что `BidiState::next_id` публичен
> или поменяй на `pub(super)`.

Если `BidiState::next_id` не `pub` — найди `fn next_id` в `BidiState impl` и
добавь `pub(super)`:

```rust
    pub(super) fn next_id(&mut self, tag: u16) -> String {
```

### 4. Исправить сигнатуры в dispatch-arms

`script_add_preload` и `script_remove_preload` принимают `&mut BidiState`, значит
arms должны быть такими (замени в dispatch):

```rust
        "script.addPreloadScript" => script_add_preload(id, &params, state),
        "script.removePreloadScript" => script_remove_preload(id, &params, state),
```

(Уже правильно — `state` передаётся как `&mut BidiState` из `dispatch`.)

### 5. Добавить тесты

В блоке `#[cfg(test)]` в конце `protocol.rs`:

```rust
#[test]
fn script_evaluate_unknown_context_returns_error() {
    let mut state = BidiState::new();
    let result = dispatch(
        r#"{"id":1,"method":"script.evaluate","params":{"expression":"1+1","target":{"context":"bad-ctx"},"awaitPromise":false}}"#,
        &mut state,
    );
    assert!(!result.close);
    assert!(result.frames[0].contains("no such frame"), "got: {}", result.frames[0]);
}

#[test]
fn script_evaluate_no_context_returns_stub() {
    let mut state = BidiState::new();
    // No context given — should return stub without error.
    let result = dispatch(
        r#"{"id":2,"method":"script.evaluate","params":{"expression":"1+1","awaitPromise":false}}"#,
        &mut state,
    );
    assert!(result.frames[0].contains("undefined"), "got: {}", result.frames[0]);
}

#[test]
fn script_add_preload_returns_script_id() {
    let mut state = BidiState::new();
    let result = dispatch(
        r#"{"id":3,"method":"script.addPreloadScript","params":{"functionDeclaration":"()=>{}"}}"#,
        &mut state,
    );
    assert!(result.frames[0].contains("script"), "got: {}", result.frames[0]);
}

#[test]
fn script_remove_preload_acks() {
    let mut state = BidiState::new();
    let result = dispatch(
        r#"{"id":4,"method":"script.removePreloadScript","params":{"script":"stub-id"}}"#,
        &mut state,
    );
    assert!(result.frames[0].contains("success"), "got: {}", result.frames[0]);
}
```

### 6. Проверить

```bash
cargo clippy -p lumen-shell --all-targets -- -D warnings
cargo test -p lumen-shell 2>&1 | tail -5
```

### 7. Закоммитить и влить

```bash
git add crates/shell/src/bidi/protocol.rs
git commit -m "P1: BiDi script.evaluate/callFunction/addPreload/removePreload (8H.3)

Phase 1 stub: context-валидация + детерминированный ответ {type:undefined}.
script.addPreloadScript возвращает script-id через next_id. 4 unit-теста.
Реальное выполнение JS требует 8A.7 (shell-as-driver-client).

Co-Authored-By: Claude Haiku 4.5 <noreply@anthropic.com>"

cd ../..
git merge --no-ff p1-bidi-script-evaluate -m "Merge p1-bidi-script-evaluate: BiDi script module stub"
git branch -d p1-bidi-script-evaluate
git add STATUS-P1.md && git commit -m "P1: отметить p1-bidi-script-evaluate завершённой"
git push origin main
git worktree remove .claude/worktrees/bidi-script
```

---

## Критерии готовности

- [ ] `script.evaluate` / `script.callFunction` не возвращают `unknown command`
- [ ] Неизвестный context → `no such frame`
- [ ] `script.addPreloadScript` возвращает `{script: "<id>"}`
- [ ] `script.removePreloadScript` возвращает success ACK
- [ ] 4 unit-теста проходят
- [ ] Clippy чист
