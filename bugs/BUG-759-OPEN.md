# BUG-759 — `credentials` unit-тесты гоняются на процесс-глобальном провайдере без синхронизации и красят гейт `scoped-test.sh`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/credentials.rs:516-540` — `create_without_provider_rejects_not_allowed`, `create_and_get_through_installed_provider`; `provider()` / установка провайдера)
**Найден:** P3, 2026-08-10, гейтом `scripts/scoped-test.sh` при закрытии [BUG-391](BUG-391-FIXED.md)

## Симптом

```
---- credentials::tests::create_without_provider_rejects_not_allowed stdout ----
thread 'credentials::tests::create_without_provider_rejects_not_allowed' panicked
  at crates\js\src\credentials.rs:538:17:
assertion `left == right` failed
  left: ""
 right: "example.com"

test result: FAILED. 2817 passed; 1 failed
error: 1 target failed: `-p lumen-js --lib`
```

Гейт падает с `exit 101`, при этом падение не связано с правкой, которую он
проверяет.

## Причина

Провайдер учётных данных — **процесс-глобальный слот**, а два теста в одном
бинарнике работают с ним без синхронизации:

* `create_without_provider_rejects_not_allowed` проверяет `provider().is_none()`
  и только тогда зовёт `create("a|b|c|d|e|f|g|-7|0|")`, ожидая
  `NotAllowedError`;
* `create_and_get_through_installed_provider` устанавливает двойник `Echo`,
  чей `create` содержит `assert_eq!(req.rp_id, "example.com")`.

Между проверкой `is_none()` и самим вызовом `create` второй тест успевает
установить `Echo`, вызов уходит в него, и падает **чужой** assert — отсюда
и странная атрибуция: строка `538` принадлежит `Echo` внутри
`create_and_get_through_installed_provider`, а имя в отчёте — у соседнего
теста. Классический TOCTOU на разделяемом слоте; комментарий самого теста
(«Provider is process-global; other tests may install one») фиксирует
осведомлённость о проблеме, но защищает от неё только проверкой, которая
и есть гонка.

## Воспроизведение

Гонка зависит от планировщика и проявляется только под нагрузкой. Замеры
2026-08-10 на ветке `p3-bug-391`:

| Способ | Падений |
|---|---|
| `cargo test -p lumen-js --lib credentials::` (12 прогонов) | 0 из 12 |
| `cargo test -p lumen-js --features v8-backend --lib` (4 прогона) | 0 из 4 |
| `bash scripts/scoped-test.sh` (15 пакетов параллельно) | 1 из 2 |

То есть ловится именно многопакетным прогоном, где несколько тест-бинарей
конкурируют за CPU и меняют расстановку потоков внутри бинарника
`lumen-js`.

## Как чинить

Убрать разделяемое состояние из тестов, а не подкручивать тайминги:
прогонять оба теста под общим `Mutex` (сериализовать доступ к слоту
провайдера) либо — лучше — сделать установку провайдера областной
(RAII-guard, снимающий провайдер на `Drop`), чтобы «нет провайдера» было
проверяемым инвариантом, а не наблюдением. Вариант «пометить тесты
`#[ignore]`» не годится: они покрывают реальную ветку WebAuthn.

## Ловушка при диагностике

`bash scripts/scoped-test.sh > log 2>&1; echo "exit=$?"` в фоновой задаче:
итоговый код возврата, который показывает харнесс, — это код замыкающего
`echo`, то есть всегда 0. Настоящий `exit 101` виден только в выводе самого
`echo` внутри файла задачи. Не считать такой прогон зелёным по индикатору
харнесса — читать строку с кодом.

## Связанные

* Той же природы, что уже описанный флейк гейта в
  [BUG-632](BUG-632-FIXED.md) (устаревшая временная БД HTTP-кэша) — общий
  класс: гейт красят не правки, а разделяемое состояние тестов.
