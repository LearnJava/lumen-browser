# BUG-304: DNS-резолвер — os error 11004 (WSANO_DATA) на живых доменах

**Статус:** FIXED 2026-07-29
**Дата:** 2026-07-17
**Компонент:** network (резолверы: `dns.rs` / `doh.rs` / `dot.rs`)
**Найден:** впервые замечен ручным аудитом 2026-07-02 (баг тогда не завели);
подтверждён перф-аудитом `/lumen-perf-audit` 2026-07-17 на 3+ сайтах

## Симптом

Подресурсы с живых, резолвящихся доменов стабильно падают с
«Запрошенное имя верно, но данные запрошенного типа не найдены»
(WSANO_DATA / os error 11004 — имя существует, но записи запрошенного типа нет):

```
✗ https://mc.yandex.ru/watch/16443139        (rbc.ru)
✗ https://top-fwz1.mail.ru/counter?...       (rbc.ru)
✗ https://ssp.rambler.ru/capirs_async.js     (lenta.ru)
✗ https://static-mon.yandex.net/static/main.js (ya.ru)
✗ https://cdn.skcrtxr.com/...                (habr.com)
```

`nslookup mc.yandex.ru` при этом отвечает A-записью — домены резолвятся
системно. Гипотеза июльского аудита: резолвер просит не тот тип записи
(например, только AAAA без фолбэка на A, или SRV/HTTPS-запись).

## Влияние

Каждый затронутый сайт теряет часть подресурсов (в основном счётчики/реклама —
но механизм общий и может бить по CDN со скриптами/шрифтами). Плюс лишние
таймауты в сетевой фазе загрузки.

## Диагностика 2026-07-29 (P3) — гипотеза «не тот тип записи» ОПРОВЕРГНУТА

Симптом воспроизводится **вне Lumen**, тем же системным getaddrinfo, и сразу
для всех трёх семейств адресов:

```
python -c "import socket; socket.getaddrinfo('mc.yandex.ru', 443, fam, socket.SOCK_STREAM)"
  AF_UNSPEC → [Errno 11004]   AF_INET (A) → [Errno 11004]   AF_INET6 (AAAA) → [Errno 11004]
  example.com теми же тремя вызовами → OK
```

То есть Lumen ничего не спрашивает «не то»: `SystemDnsResolver` вызывает
`(host, port).to_socket_addrs()` = getaddrinfo с `AF_UNSPEC`, и ОС возвращает
ошибку до того, как код видит хоть один адрес.

Причина — DNS-сервер, настроенный на машине, блокирует эти домены и отвечает
**sinkhole-адресом**: `nslookup mc.yandex.ru` → сервер `fdfe:dcba:9876::2`,
ответ `0.0.0.0` и `::` (а не реальная A-запись, как показалось при первом
аудите). Windows-getaddrinfo такой ответ отбрасывает и поднимает WSANO_DATA.
Коды различают случаи однозначно:

| хост | `raw_os_error()` |
|---|---|
| `mc.yandex.ru` (sinkhole-ответ) | `Some(11004)` — WSANO_DATA |
| `no-such-host-xyz.invalid` (NXDOMAIN) | `Some(11001)` — WSAHOST_NOT_FOUND |
| `example.com` | Ok, 4 адреса |

**Вывод:** исходная заявка — не дефект резолвера Lumen, а блокировка на уровне
DNS (AdGuard-подобный сервер либо hosts-файл формата `0.0.0.0 tracker`). Пять
доменов из списка — счётчики и рекламные сети, ровно то, что такие списки режут.

## Реальный дефект, найденный по ходу (он и исправлен)

Ни один из трёх резолверов не отбрасывал sinkhole-адрес:

* `SystemDnsResolver` (`dns.rs`) — на Windows его прикрывает сама ОС, но на
  Linux/macOS getaddrinfo отдаёт `0.0.0.0` из `/etc/hosts` как обычный ответ
  (самый распространённый формат hosts-блоклистов);
* `DohResolver` (`doh.rs`) и `DotResolver` (`dot.rs`) разбирают A/AAAA-записи
  сами и системного фильтра не видят вообще — ни на одной ОС.

Дальше `connect()` (`lib.rs:1341`) честно пытался соединиться по полученному
адресу, а `TcpStream::connect("0.0.0.0:443")` уходит **на локальную машину**:
запрос к заблокированному домену молча попал бы на сервис пользователя на том
же порту (в лучшем случае — connection refused и лишний таймаут вместо внятной
ошибки).

**Фикс:** общий фильтр `dns::reject_sinkhole_addrs(hostname, addrs)` —
выбрасывает unspecified-адреса (`0.0.0.0` / `::`) из ответа резолвера; если
пригодных адресов не осталось, а ответ был непуст, поднимается явная ошибка
«DNS answered with a sinkhole address only (0.0.0.0/::) — host is blocked by
the DNS server or hosts file». Подключён во всех трёх резолверах.

Границы фильтра (сознательные):

* **loopback (`127.0.0.1`) НЕ фильтруется** — валидный ответ для локальной
  разработки (`*.localtest.me` и подобные);
* **IP-литералы не фильтруются** — `http://0.0.0.0:8080/` это явное намерение
  пользователя (обычный адрес dev-сервера); DoH/DoT литералы и так обрабатывают
  до запроса, `SystemDnsResolver` проверяет литерал явно (`is_ip_literal`);
* частичный sinkhole (AAAA заблокирован, A настоящий) — рабочий адрес остаётся,
  ошибки нет.

Плюс к сырому тексту ошибки getaddrinfo на Windows дописывается подсказка при
`raw_os_error() == Some(11004)`: «name exists but has no address records —
usually DNS-level blocking» — чтобы следующий, кто увидит 11004 в логах, не
искал дефект в резолвере (как это произошло здесь).

## Проверка

`cargo test -p lumen-network --lib dns::` (10 passed), `--lib doh::` (44 passed).
Новые тесты: `sinkhole_only_answer_is_rejected`,
`sinkhole_mixed_answer_keeps_usable_addrs`, `sinkhole_filter_keeps_loopback`,
`sinkhole_filter_passes_empty_input_through`,
`system_resolver_unspecified_literal_passes`, `ip_literal_detection`,
`resolve_sinkhole_only_returns_err`, `resolve_sinkhole_plus_real_keeps_real`,
`resolve_unspecified_literal_is_not_filtered`.
`cargo clippy -p lumen-network --all-targets -- -D warnings` — зелёный.

## Что осталось за скобками

Сами пять доменов на этой машине по-прежнему не резолвятся — это ожидаемо и
правильно: их режет DNS пользователя. Отдельного «фикса» тут нет и быть не
должно; чтобы видеть их загруженными, меняют DNS-сервер машины, а не браузер.
