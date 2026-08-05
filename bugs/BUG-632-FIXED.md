# BUG-632 — `DiskHttpCache`-тесты валят гейт из-за протухших temp-БД (переиспользование PID)

**Статус:** FIXED 2026-08-05
**Компонент:** network (`crates/network/src/http_cache.rs`, тестовый хелпер `tmp_db_path`)
**Найден:** 2026-08-05, P3, на гейте `scripts/scoped-test.sh` в ходе BUG-277 среза 6

## Симптом

`scripts/scoped-test.sh` падает на `-p lumen-network --lib` с двумя провалами,
не связанными с изменением, которое гейт проверяет:

```
http_cache::tests::disk_cache_store_and_get_fresh   left: 2, right: 1
http_cache::tests::disk_cache_no_store_not_persisted left: 1, right: 0
```

`cache.len()` больше ожидаемого ровно на число записей, оставшихся от чужого
прогона: БД открывается не пустой.

## Корень

```rust
fn tmp_db_path() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join(format!("lumen_test_http_cache_{}_{}.db", std::process::id(), n))
}
```

Имя уникально **внутри одного процесса**, но не между прогонами: Windows
переиспользует PID, а `std::fs::remove_file` в конце каждого теста выполняется
только на успешном пути — упавший или прерванный прогон оставляет файл навсегда.
`DiskHttpCache::new` открывает существующий файл и наследует его строки.

Счётчик `n` вдобавок раздаётся в порядке реального старта тестовых потоков,
поэтому одному и тому же тесту от прогона к прогону достаётся разный `n` — то
есть протухший файл цепляет то один тест, то другой, то никакой. Отсюда
«флейк, воспроизводящийся раз в N прогонов».

На машине разработчика на момент находки лежало **875** файлов
`%TEMP%/lumen_test_http_cache_*.db` от 21.07 и 02.08.

## Проверка

```bash
rm -f "$TEMP"/lumen_test_http_cache_*.db
cargo test -p lumen-network --lib     # 2119 passed; 0 failed
```

## Как чинить

Любое из (первое предпочтительно):

1. `tempfile::TempDir` (крейт уже в dev-dependencies workspace-а?) либо явный
   `let _ = std::fs::remove_file(&path);` **перед** `DiskHttpCache::new`, чтобы
   тест не зависел от чужого мусора.
2. Гарантированная уборка через RAII-guard вместо `remove_file` в хвосте (сейчас
   при панике файл течёт — а тест паникует именно тогда, когда файл мешает,
   то есть дефект самоподдерживающийся).
3. Добавить в имя источник энтропии, не совпадающий между прогонами
   (время старта процесса), — самая слабая мера: мусор всё равно копится.

## Влияние

Ложно-красный обязательный гейт: следующий разработчик увидит два провала в
`lumen-network` поверх своего изменения в другом крейте и потратит время на
разбор. Продакшен-код не затронут — дефект целиком в тестовом хелпере.

## Как исправлено

Реализован вариант 2 из «Как чинить» (RAII-guard), скомбинированный с
вариантом 1 (уборка чужого мусора перед выдачей пути) — `tmp_db_path()`
заменён на `TmpDbGuard` в `mod tests`:

```rust
struct TmpDbGuard(PathBuf);

impl TmpDbGuard {
    fn new() -> Self {
        static CTR: AtomicU64 = AtomicU64::new(0);
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("lumen_test_http_cache_{}_{}.db", std::process::id(), n));
        let _ = std::fs::remove_file(&path); // подчистить чужой мусор по этому пути
        Self(path)
    }
}

impl Drop for TmpDbGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
```

`Drop` выполняется и при размотке стека на панике (тестовый харнесс не
использует `panic=abort`), поэтому файл больше не может пережить упавший
тест — самоподдерживающийся цикл разорван. Все 7 тестов `disk_cache_*`
переведены на `TmpDbGuard`, ручные `std::fs::remove_file` в хвостах убраны.

**Проверка:** `rm -f "$TEMP"/lumen_test_http_cache_*.db` (разово подчистить
875 старых файлов) → `cargo test -p lumen-network --lib` — 2119/2119 зелёных,
`cargo clippy -p lumen-network --all-targets -- -D warnings` чист.
