# BUG-792 — https-ответ без TLS `close_notify` теряет тело целиком: весь HTTPS-корпус WPT по-прежнему недостижим

**Статус:** OPEN
**Заведён:** 2026-08-20 (WPT-RUN-5, полный прогон корпуса на Linux)
**Область:** `lumen-network` (`crates/network/src/lib.rs` — body-секция
`read_response`, `read_framed_buffered`, streaming-вариант; ветка
`BodyFraming::Eof` / «ни chunked, ни Content-Length»)
**Владелец:** P1/P3 (движок). Найден P2.

## Симптом

```
network error: read body: peer closed connection without sending TLS close_notify:
https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof
```

**2380 попаданий** в логах 37 шардов Linux-прогона WPT-RUN-5 — самая частая
сетевая ошибка прогона с отрывом (следующая, `HTTP 404`, даёт 171). В двух
https-категориях: `WebCryptoAPI` — 710, `ai` — 721.

Итог по этим же 37 шардам: из **635 прогнанных `.https.`-тестов harness OK — 0,
сабтестов отчитано 0** (505 TIMEOUT, 115 ERROR, 15 FAIL). Для сравнения,
не-https на том же прогоне: 653 OK из 1362 и тысячи сабтестов. То есть https
как был, так и остался нулём — но уже по другой причине, чем
[BUG-785](BUG-785-FIXED.md) (тот про `UnknownIssuer`, TLS-рукопожатие; оно
теперь проходит, `UnknownIssuer` в логах прогона — 0 попаданий).

Вторичные симптомы того же корня, если смотреть по статусам тестов, а не по
логу: 302 `Timed out waiting for testharnessreport.js results` и 115
`browsingContext.navigate reported success but the document was never replaced`
(последнее — [BUG-438](BUG-438-OPEN.md) в роли усилителя: навигация на
незагрузившийся документ рапортует успех).

## Механизм

`wptserve` отдаёт значительную часть ответов **close-delimited**: без
`Content-Length` и без `chunked`, конец тела = конец соединения (RFC 7230
§3.3.3 п.7). Движок это поддерживает — ветка `read_to_end` в body-секции
`read_response` заведена ровно под wptserve (комментарий там же).

Но поверх TLS «конец соединения» бывает двух видов, и `rustls` их различает:
корректное завершение (`close_notify`) и обрыв. На обрыве `rustls` возвращает
`io::ErrorKind::UnexpectedEof` — защита от truncation-атаки. Наш код зовёт
`read_to_end` и на любой `Err` **выбрасывает всё прочитанное** и возвращает
`Error::Network`. Тело, дошедшее полностью, теряется целиком из-за того, как
соединение было закрыто.

`wptserve` `close_notify` не шлёт. Отсюда: каждый close-delimited https-ответ —
документ, `testharness.js`, сабресурс — приходит в движок как сетевая ошибка.

Затрагивает три места с одинаковой логикой:

- `crates/network/src/lib.rs:1249` — «ни chunked, ни Content-Length» в `read_response`
- `crates/network/src/lib.rs:1258` — `BodyFraming::Eof` в `read_framed_buffered`
- streaming-путь там же (`err = Some(Error::Network(format!("read body: {e}")))`)

`Content-Length`-путь не затронут: `read_exact` набирает объявленную длину до
того, как дойдёт до обрыва.

## Репро

`tests/wpt/verify_bug792_tls_eof_body.py` — три TLS-ответа с одной и той же
страницей, wptserve в репро не участвует:

```
$ python tests/wpt/verify_bug792_tls_eof_body.py
eof-clean      LAID OUT      # без Content-Length, но с close_notify
length-abrupt  LAID OUT      # Content-Length + обрыв без close_notify
eof-abrupt     LOST          # без Content-Length + обрыв  <- дефект
  Ошибка dump ...: network error: read body: peer closed connection without sending TLS close_notify
BUG-792 REPRODUCED: only EOF-framed + no close_notify loses the body
```

Две зелёные строки важны не меньше красной: они показывают, что ни TLS сам по
себе, ни резкий обрыв соединения по отдельности движок не ломают — ломает
именно сочетание.

## Что делать

Прочитанные до обрыва байты — это и есть тело; браузеры такой ответ принимают
(иначе половина интернета не открывалась бы). Нужно на EOF-фреймировании
трактовать `UnexpectedEof` как обычный EOF: не `read_to_end` с потерей буфера,
а цикл `read` с накоплением, где `UnexpectedEof` завершает тело успешно.

Оговорка про безопасность, которую нельзя терять при исправлении: `rustls`
поднимает эту ошибку не из педантизма, а против усечения ответа. Поэтому
послабление уместно **только** на EOF-фреймировании, где длина неизвестна и
усечение неотличимо от нормы в принципе. На `Content-Length` и `chunked`
обрыв обязан остаться ошибкой: там недобор байт — доказуемое усечение.

Цена бездействия измерена: 7190 автоматизируемых id манифеста (**10.6 %
корпуса**) гарантированно стоят ноль в любой цифре pass-rate, сколько бы часов
прогон ни занял.

## Независимое подтверждение с Windows-половины (WPT-RUN-6, 2026-08-20/21)

Разбор массового TIMEOUT завершённого Windows-прогона WPT-RUN-5 (479/479
шардов) той же классификацией по суффиксу файла теста даёт тот же паттерн,
что на Linux: `https.window` 179/189 (94.7%), `https.any` 169/171 (98.8%),
`https.any.worker` 149/149 (100.0%), `any.serviceworker` 205/210 (97.6%) —
итого 702 id, все на `https://127.0.0.1:18443` (подтверждено прямым чтением
`Reload:`-строк в `.tmp/wpt-corpus/*.log`, отличимо от воркерной причины
[BUG-778](BUG-778-OPEN.md), у которой те же суффиксы без `https.`-префикса
идут по обычному `http://127.0.0.1:18300`). Отдельное подтверждение
воспроизводится на другой машине/сборке — не артефакт одного прогона.
