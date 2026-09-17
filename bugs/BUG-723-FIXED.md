# BUG-723: шимовый `fetch()` не умеет `file://` и теряет двоеточие диска — динамические `<script>`/`<link>` на локальной странице отдают `error`

**Статус:** FIXED 2026-09-18 (P3)
**Компонент:** js (`crates/js/src/dom.rs` — `_url_resolve` + `fetch`),
shell (`crates/shell/src/resource_base.rs`, `frames.rs`, `page_load.rs`,
`lumen/hibernation.rs`), network (`crates/network/src/lib.rs::fetch_with_redirect`,
`crates/core/src/url.rs::Url`)
**Найден:** P3, проверка фикса [BUG-722](BUG-722-FIXED.md), 2026-08-09

## Симптом

На странице по схеме `file://` любой ресурс, который шим грузит через свой
`fetch` (внешний динамический `<script src>` — путь
[BUG-571](BUG-571-FIXED.md); `<link rel=stylesheet>` — путь
[BUG-722](BUG-722-FIXED.md)), падает, и элемент получает `error` вместо
`load`. Ровно те же файлы шелл при этом грузит и применяет штатно.

Две пробы рядом (`.tmp/b723.html`, `.tmp/b703_link.html`), обе из живого
окна:

```
[JS error] script load failed: file://D/RustProjects/.../b723_x.js:
    TypeError: fetch: network error for file://D/RustProjects/.../b723_x.js
[JS error] stylesheet load failed: file://D/RustProjects/.../b703_link.css:
    TypeError: fetch: network error for file://D/RustProjects/.../b703_link.css
```

Для скрипта: `events: ['error']`, `window.__b723_ran === false` — тело не
исполнилось. Для листа: `events: ['error']`, но
`getComputedStyle(target).color === 'rgb(1, 2, 3)'` — то есть шелл этот же
файл прочитал и влил в каскад, а JS-путь по нему отчитался ошибкой.

## Что видно из текста ошибки

В URL пропало двоеточие диска: `file://D/RustProjects/...` вместо
`file:///D:/RustProjects/...`. Исходная страница открыта как
`file:///D:/RustProjects/...`, значит двоеточие теряет либо
`_lumen_document_base_url()`, либо `_url_resolve` при разрешении
относительной ссылки. Родственно (но не тождественно) разбору
драйв-леттера в [BUG-651](BUG-651-FIXED.md), где ту же букву диска неверно
обрабатывает `PageSource::from_arg`.

Двух дефектов здесь, судя по всему, два независимых: (1) искажение пути и
(2) отсутствие поддержки схемы `file://` в провайдере `fetch` как таковой.
Второе нужно проверить отдельно — на корректном `file:///D:/…` URL, — прежде
чем чинить: не исключено, что после починки (1) схема заработает сама.

## Почему заведено отдельно

Пробел не привнесён BUG-722 и не относится к `<link>`: он общий для всего,
что шим грузит своим `fetch`, и на `<script>` воспроизводится ровно так же
(проверено, не выведено по аналогии). До BUG-722 на `<link>` он был не виден
просто потому, что событий не было вовсе.

## Чем это мешает

Локальные HTML-страницы — основной формат отладочных проб и графических
тестов репозитория, так что любая проба «динамический скрипт/лист» на
`file://` даёт ложный `error` и легко читается как движковый баг в
проверяемой фиче. На сетевых страницах (`http(s)://`) оба пути работают —
`https://www.tbank.ru/` в разборе BUG-722 грузит и исполняет и то и другое.

## Исправлено

Оба независимых дефекта из раздела выше подтвердились.

**(1) искажение пути.** Корень — не JS, а Rust: шесть точек в
`crates/shell/src` (`resource_base.rs::base_url_string`, `frames.rs` ×3,
`lumen/hibernation.rs`, `page_load.rs`) строили `location.href`/дочерний URL
как `format!("file://{}", path.display())` — ровно 2 буквальных слэша.
`file_url_to_path` (URL → путь) снимает ведущий `/` перед буквой диска, так
что путь, полученный из URL, хранится как `D:/RustProjects/...` без него; в
обратную сторону тот `/` никто не возвращал, и итоговая строка получалась
`file://D:/RustProjects/...` — **2** слэша, а не канонические 3
(`file:///D:/...`, как везде в остальной кодовой базе). При таком вводе
JS-шимовый `_lumen_parse_url` (`crates/js/src/shim/url_parse_shim.js`) читает
`rest` после `://` как `D:/RustProjects/...`, находит первый `/` только на
третьей позиции, значит "authority" = `D:`, парсит его как `hostname:port`,
получает `port = ""` (после двоеточия ничего до `/`) — и поскольку пустая
строка falsy, отбрасывает и порт, и двоеточие: `host = "D"`. Отсюда
`file://D/RustProjects/...` из симптома.

Добавлена каноническая `path_to_file_url()` (`resource_base.rs`) — обратная
функция к уже существовавшей `file_url_to_path()`: вставляет недостающий `/`
перед буквой диска (проверка «второй байт — `:`»). Все шесть сайтов
переведены на неё.

**(2) отсутствие поддержки `file:` в `JsFetchProvider`.** Реальная
реализация — `impl JsFetchProvider for HttpClient`
(`crates/network/src/lib.rs`) — никогда не имела ветки для `file:`:
`fetch_with_redirect` вызывает `require_http_scheme`, который отвергает
любую схему кроме `http`/`https` с `Error::Network("unsupported scheme: file")`
ещё до открытия сокета — то есть даже на корректном `file:///D:/...` URL
после фикса (1) запрос по-прежнему падал бы. Добавлена отдельная ветка
`file:` в `fetch_with_redirect`, по образцу уже существующей ветки `data:`
(DATAURL-1): байты читаются синхронно с диска через новый
`Url::to_file_path()` (`crates/core/src/url.rs`, делегирует `to_file_path()`
крейта `url` — тот же ОС-корректный алгоритм, что и у браузеров), content-type
угадывается по расширению (`guess_file_content_type()` — покрывает типичные
подресурсы: скрипты, стили, изображения, шрифты; для остального —
`application/octet-stream`, как у обычного статического файлового сервера).

Новые тесты: `to_file_path_windows_drive_letter`/
`to_file_path_non_file_scheme_is_none` (`lumen-core`),
`guess_file_content_type_covers_script_and_style`/
`fetch_request_file_url_reads_local_disk_without_network` (`lumen-network` —
порт `0` в URL доказал бы, что запрос ушёл бы в сеть, если бы файл не
читался локально).

`cargo clippy -p lumen-core -p lumen-network -p lumen-shell --all-targets --
-D warnings` чист. `cargo test -p lumen-core -p lumen-network --lib`
2236/2236. `cargo test -p lumen-shell --bins` — все затронутые тесты
(`resource_base`/`frames`/`page_source`/`tab_lifecycle::hibernate`) зелёные.
`scripts/scoped-test.sh main` — единственный красный тест
`cases::snapshot_cpu::cpu_snapshots_match_references`, тот же набор из 7
файлов, что и чужой дрейф эталонов [BUG-1008](BUG-1008-OPEN.md); проверено
прогоном того же теста на `main` без правки — идентичный дифф байт-в-байт.
Только JS/сеть/шелл, пиксели не затронуты.
