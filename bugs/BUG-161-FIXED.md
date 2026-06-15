# BUG-161

**Статус:** FIXED 2026-06-15
**Компонент:** network
**Файл:** `crates/network/src/h2/hpack.rs`

## Фикс (2026-06-15)

Корень: `H2Conn::connect_with_profile` создавал `Decoder::new()`, у которого
`proto_max` оставался дефолтным 4096, тогда как клиент анонсировал в своих SETTINGS
`SETTINGS_HEADER_TABLE_SIZE = 65536` (профиль Chrome). Сервер (ya.ru) присылал
легальный Dynamic Table Size Update до 65536 — декодер отвергал его как
`TableSizeTooLarge`. Симметрия HPACK: SETTINGS пира управляет нашим encoder
(`apply_remote_settings` → `encoder.set_max_size`), а *наш* анонсированный
SETTINGS должен управлять `proto_max` декодера. Эта связка отсутствовала.

Исправление (`crates/network/src/h2/conn.rs`): после создания декодера —
`decoder.set_proto_max(settings.header_table_size as usize)`. Регрессионные тесты
в `hpack.rs`: `decode_size_update_within_proto_max`,
`decode_size_update_exceeds_proto_max_rejected`,
`decode_size_update_then_header_within_raised_max`.

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
