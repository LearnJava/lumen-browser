# BUG-161

**Статус:** OPEN
**Компонент:** network
**Файл:** `crates/network/src/h2/hpack.rs`

## Описание

HTTP/2 HPACK: `dynamic table size update exceeds negotiated max` → ya.ru вообще
не грузится. Клиент отвергает валидный dynamic table size update, считая, что он
превышает согласованный максимум.

Обнаружено при попытке открыть `https://ya.ru/`.

## Старт расследования

В `crates/network/src/h2/hpack.rs` проверить обработку HPACK dynamic table size
update (RFC 7541 §6.3) против согласованного `SETTINGS_HEADER_TABLE_SIZE`.
Вероятная причина: сравнение с неверным лимитом (наш max вместо max пира) или
несколько подряд идущих size-update в одном header block обрабатываются неверно.
