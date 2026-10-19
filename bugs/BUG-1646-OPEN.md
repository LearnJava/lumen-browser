# BUG-1646 — H2-соединение рвётся по «no response to PING for 5000 ms» (anubis.techaro.lol, leboncoin, zillow), первый кадр на 29–74 с

**Статус:** OPEN
**Заведён:** 2026-10-19 (P2, COMPAT-CORPUS-4-BASE, `scripts/captcha_stand.py --arm lumen`)
**Область:** network (`crates/network/src/h2/mux.rs`, живая проверка простоя PING, BUG-1205)

## Симптом

Живое окно `--maximized`, маршрут home-direct, `target/dev-release/lumen.exe` от 2026-10-19 15:35.
Разрывы `H2 connection lost … no response to PING for 5000 ms` в stderr (счёт по 20 страницам корпуса `corpus-captcha.tsv`):
`anubis-techaro` — 9, `human-zillow` — 11, `datadome-leboncoin` — 5, `akamai-nike` — 1; на остальных 16 — 0.

- `[bench] first non-empty frame`: anubis-techaro 28885 мс (повтор 31550), datadome-leboncoin 68045 мс, human-zillow 73697 мс
  (у страниц без разрывов — единицы секунд, gitlab.gnome.org 7344 мс);
- `https://anubis.techaro.lol/` стоит на «Loading…»: падают картинки `pensive.webp`/`happy.webp`; в 1 прогоне из 4 после PoW пришёл
  `302` на `/`, запрос `/` упал тем же разрывом, вкладка показала страницу Lumen «Сайт … слишком долго не отвечает»;
- `anubis-gnome-gitlab` (`gitlab.gnome.org/GNOME/gtk`) в эту причину **не** входит: разрывов нет, `rs=complete`, текст 0 символов — отдельный дефект.

`curl --noproxy '*' https://anubis.techaro.lol/` с той же машины отвечает сразу. Эталон Chrome 148 (Playwright) на том же
маршруте проходит проверку (`docs/perf/runs/2026-10-14-captcha-chrome.json`, `anubis-techaro` — пройдено).

## Что сделать

1. Воспроизвести без окна (`--ipc-server` или сетевой пример крейта `network`) и выяснить, почему PING не получает ACK за 5 с:
   сервер не отвечает на PING при открытых потоках, или цикл мультиплексора занят и не читает кадры вовремя.
2. Если сервер просто молчит — не объявлять соединение мёртвым, пока по открытым потокам идут DATA/HEADERS
   (любой входящий кадр = живое соединение), либо поднять таймаут и перекинуть запрос на новое соединение (как уже сделано
   для повторного запроса, `mux.rs:666`).
3. Проверка: `python scripts/captcha_stand.py --arm lumen --only anubis-techaro,datadome-leboncoin,human-zillow` → без
   `H2 connection lost` в stderr; `first non-empty frame` у leboncoin/zillow — единицы секунд.

Смежно: [BUG-1205](BUG-1205-FIXED.md) (PING-проверка простоя).
