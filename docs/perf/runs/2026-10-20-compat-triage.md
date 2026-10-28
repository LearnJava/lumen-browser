# Кластеризация прогона корпуса C (SITES-TRIAGE-1)

Источники: `.tmp/compat-crawl-c-2026-10-28/results.jsonl`, `.tmp/compat-crawl-user-2026-10-28/results.jsonl`. Сайтов в прогоне: 503, не прошли: 466.

**Происхождение данных.** Сырьё прогона 2026-10-20 (`results.jsonl`) на диске не сохранилось, поэтому корпус C (500 главных страниц) и 30 сайтов пользователя перепрогнаны 2026-10-28: `scripts/compat_crawl.py --env LUMEN_NO_ADBLOCK=1 --no-rerun --chrome-arg=--no-proxy-server`, `dev-release` от 2026-10-28 (HEAD main), таймаут 40 с (сайты пользователя) и 30 с + dwell 1,5 с (корпус C), маршрут машины как есть (без прокси). Прогон корпуса дважды продолжен `--resume` (лимит фоновой задачи, сбой запуска Chrome). «Не прошёл» = статус Lumen не OK **или** упала хотя бы одна проверка паритета, поэтому в классы pixels/a11y попадают и сайты со статусом OK. Сайты, где эталон (Chrome) не открылся, проверок паритета не имеют и считаются только по статусу Lumen. Один результат на сайт: точки abroad/ru не разделены, глобальные сайты на этом маршруте могут быть завышены гео-блокировкой.
Первичный класс — по приоритету: не наш → TIMEOUT/HUNG → NET_FAIL → blank:near-empty → BROKEN_RENDER → pixels (differing) → a11y → прочее. Вес сайта 1–3 из `corpus-c.tsv`, сайт пользователя — 5.

## Сайты пользователя (без прокси)

| сайт | статус | класс | причина | ready_s |
|---|---|---|---|---|
| dzen-ru | OK | OK |  | 1.2 |
| pikabu-ru | OK | a11y | stderr: не декодируется #maskN_N_N: формат изображения не распознан по сигнатуре | 30.6 |
| mail-ru | BROKEN_RENDER | BROKEN_RENDER | нет API: cannot read properties of undefined (reading 'appendchild') | 5.2 |
| ozon-ru | SITE_REFUSED | не наш | без улик | 0.5 |
| avito-ru | DEGRADED | прочее | исключение: link hint fetch failed: URL | 37.8 |
| habr-com | HUNG | TIMEOUT/HUNG | stderr: ⤷ preload js [medium] URL | 55.5 |
| yandex-ru | OK | OK |  | 1.7 |
| vk-com | BROKEN_RENDER | BROKEN_RENDER | нет API: topsearch is not defined | 13.5 |
| rbc-ru | OK | a11y | stderr: пропуск картинки URL network error: http N | 4.9 |
| ria-ru | DEGRADED | a11y | исключение: @firebase/analytics: transactioninactiveerror: transaction is not active | 34.9 |
| lenta-ru | DEGRADED | blank:near-empty | stderr: [js warn] [api-cache] idle gc failed transactioninactiveerror: transaction is not  | 20.7 |
| gosuslugi-ru | BROKEN_RENDER | BROKEN_RENDER | stderr: [unhandled-rejection] axioserror: network error | 1.2 |
| wildberries-ru | BROKEN_RENDER | BROKEN_RENDER | stderr: [js error] ошибка загрузки видео [object event] | 13.3 |
| kinopoisk-ru | DEGRADED | прочее | stderr: [js warn] [api-cache] idle gc failed transactioninactiveerror: transaction is not  | 1.6 |
| ok-ru | OK | a11y | stderr: [js warn] performanceobserver: unsupported entrytype first-input | 40.5 |
| rutube-ru | TIMEOUT | TIMEOUT/HUNG | stderr: [js error] uncaught typeerror: navigator.serviceworker.startmessages is not a func | 41.6 |
| 2gis-ru | BROKEN_RENDER | BROKEN_RENDER | исключение: error: [clickstream agent] application is offline | 46.5 |
| hh-ru | OK | pixels | stderr: [js error] error: [clickstream agent] application is offline | 19.0 |
| gazeta-ru | BROKEN_RENDER | blank:near-empty | stderr: [unhandled-rejection] error: [ federation runtime ]: remoteentryexports is undefin | 20.3 |
| kp-ru | DEGRADED | a11y | исключение: script load failed: url typeerror: fetch: network error for url | 30.3 |
| sports-ru | DEGRADED | a11y | исключение: referenceerror: cannot access 'asyncwidget' before initialization | 21.5 |
| vc-ru | OK | a11y | stderr: [js websocket] connect error: network error: ws: read response: failed to fill who | 23.9 |
| auto-ru | OK | a11y | stderr: [unhandled-rejection] typeerror: fetch: network error for URL | 14.7 |
| drom-ru | DEGRADED | a11y | исключение: error: element to connect input does not exist. | 6.4 |
| cian-ru | OK | OK |  | 3.8 |
| mos-ru | DEGRADED | blank:near-empty | без улик | 14.2 |
| tbank-ru | HUNG | TIMEOUT/HUNG | сеть: ✗ URL (tls: TLS handshake: untrusted certificate chain: Unkn | 72.8 |
| sberbank-ru | NET_FAIL | NET_FAIL | сеть: ✗ URL (tls: TLS handshake: untrusted certificate chain: Unkn | 1.7 |
| rambler-ru | OK | a11y | stderr: [js error] [usemailcounter] error: [object object] | 4.5 |
| market-yandex-ru | DEGRADED | прочее | нет API: interface is not defined | 38.5 |

## Классы

| класс | сайтов | вес |
|---|---|---|
| не наш | 28 | 56 |
| TIMEOUT/HUNG | 107 | 218 |
| NET_FAIL | 13 | 30 |
| blank:near-empty | 77 | 156 |
| BROKEN_RENDER | 22 | 62 |
| pixels | 59 | 103 |
| a11y | 141 | 313 |
| прочее | 19 | 49 |
| **итого** | **466** | **987** |

## Кластеры по убыванию веса

### 1. [a11y] без улик

- сайтов: 22, вес: 48
- примеры: dns-shop-ru (https://dns-shop.ru/, ready_s=1.1, в3); t-me (https://t.me/, ready_s=7.1, в3); t2-ru (https://t2.ru/, ready_s=2.3, в3); wikipedia-org (https://wikipedia.org/, ready_s=7.2, в3); x-com (https://x.com/, ready_s=15.2, в3)

### 2. [TIMEOUT/HUNG] без улик

- сайтов: 21, вес: 48
- примеры: cloudflare-com (https://cloudflare.com/, ready_s=30.1, в3); dropbox-com (https://dropbox.com/, ready_s=32.1, в3); intuit-com (https://intuit.com/, ready_s=32.1, в3); magnit-ru (https://magnit.ru/, ready_s=30.1, в3); microsoft-com (https://microsoft.com/, ready_s=35.7, в3)

### 3. [blank:near-empty] без улик

- сайтов: 19, вес: 39
- примеры: mos-ru (https://mos.ru/, ready_s=14.2, в5); cnn-com (https://cnn.com/, ready_s=—, в3); google-com (https://google.com/, ready_s=16.0, в3); afisha-ru (https://afisha.ru/, ready_s=0.8, в2); britannica-com (https://britannica.com/, ready_s=30.3, в2)

### 4. [не наш] без улик

- сайтов: 14, вес: 32
- примеры: ozon-ru (https://ozon.ru/, ready_s=0.5, в5); forbes-com (https://forbes.com/, ready_s=8.5, в3); reddit-com (https://reddit.com/, ready_s=2.8, в3); allegro-pl (https://allegro.pl/, ready_s=2.7, в2); dellin-ru (https://dellin.ru/, ready_s=20.5, в2)

### 5. [a11y] stderr: [unhandled-rejection] error: battery status api is not supported

- сайтов: 10, вес: 22
- примеры: linkedin-com (https://linkedin.com/, ready_s=10.8, в3); reg-ru (https://reg.ru/, ready_s=1.9, в3); att-com (https://att.com/, ready_s=14.0, в2); avidreaders-ru (https://avidreaders.ru/, ready_s=5.3, в2); evotor-ru (https://evotor.ru/, ready_s=7.9, в2)

### 6. [NET_FAIL] сеть: ✗ URL (tls: TLS handshake: untrusted certificate chain: Unkn

- сайтов: 6, вес: 17
- примеры: sberbank-ru (https://sberbank.ru/, ready_s=1.7, в5); alfabank-ru (https://alfabank.ru/, ready_s=1.4, в3); vtb-ru (https://www.vtb.ru/, ready_s=2.8, в3); nspk-ru (https://nspk.ru/, ready_s=1.5, в2); rzd-ru (https://rzd.ru/, ready_s=14.9, в2)

### 7. [blank:near-empty] stderr: не декодируется URL

- сайтов: 8, вес: 16
- примеры: tumblr-com (https://tumblr.com/, ready_s=25.3, в3); tutu-ru (https://tutu.ru/, ready_s=25.7, в3); gazprombank-ru (https://gazprombank.ru/, ready_s=28.3, в2); mamba-ru (https://mamba.ru/, ready_s=8.9, в2); mk-ru (https://mk.ru/, ready_s=28.4, в2)

### 8. [pixels] без улик: расхождение кадра

- сайтов: 7, вес: 14
- примеры: netflix-com (https://netflix.com/, ready_s=11.1, в3); ring-com (https://ring.com/, ready_s=16.1, в2); skyeng-ru (https://skyeng.ru/, ready_s=8.0, в2); supercell-com (https://supercell.com/, ready_s=6.5, в2); threads-com (https://threads.com/, ready_s=10.3, в2)

### 9. [a11y] stderr: [js websocket] connect error: network error: ws: read response: failed to fill who

- сайтов: 5, вес: 14
- примеры: vc-ru (https://vc.ru/, ready_s=23.9, в5); mangabuff-ru (https://mangabuff.ru/, ready_s=23.6, в3); rustore-ru (https://rustore.ru/, ready_s=14.8, в3); lemonde-fr (https://lemonde.fr/, ready_s=27.7, в2); interia-pl (https://interia.pl/, ready_s=20.5, в1)

### 10. [a11y] stderr: [js warn] performanceobserver: unsupported entrytype first-input

- сайтов: 5, вес: 13
- примеры: ok-ru (https://ok.ru/, ready_s=40.5, в5); hse-ru (https://hse.ru/, ready_s=11.5, в2); pixabay-com (https://pixabay.com/, ready_s=22.9, в2); progoroduhta-ru (https://progoroduhta.ru/, ready_s=4.4, в2); yoomoney-ru (https://yoomoney.ru/, ready_s=11.4, в2)

### 11. [blank:near-empty] stderr: [unhandled-rejection] error: battery status api is not supported

- сайтов: 5, вес: 11
- примеры: rt-ru (https://www.rt.ru/, ready_s=8.9, в3); elpais-com (https://elpais.com/, ready_s=25.6, в2); interfax-ru (https://interfax.ru/, ready_s=15.2, в2); playground-ru (https://playground.ru/, ready_s=24.9, в2); sima-land-ru (https://sima-land.ru/, ready_s=31.6, в2)

### 12. [a11y] stderr: пропуск картинки URL ne

- сайтов: 5, вес: 9
- примеры: bsky-app (https://bsky.app/, ready_s=16.5, в2); namecheap-com (https://namecheap.com/, ready_s=23.8, в2); playwekings-ru (https://playwekings.ru/, ready_s=26.8, в2); usps-com (https://usps.com/, ready_s=15.8, в2); magnific-com (https://magnific.com/, ready_s=11.8, в1)

### 13. [TIMEOUT/HUNG] stderr: [js websocket] connect error: network error: ws: read response: failed to fill who

- сайтов: 4, вес: 9
- примеры: imgsrc-ru (https://www.imgsrc.ru/, ready_s=30.3, в3); detmir-ru (https://detmir.ru/, ready_s=31.3, в2); dtf-ru (https://dtf.ru/, ready_s=32.1, в2); radiopotok-ru (https://radiopotok.ru/, ready_s=11.8, в2)

### 14. [a11y] stderr: ⤷ preload js [medium] URL

- сайтов: 4, вес: 9
- примеры: shopify-com (https://shopify.com/, ready_s=10.2, в3); tabor-ru (https://tabor.ru/, ready_s=3.3, в3); aol-com (https://aol.com/, ready_s=29.8, в2); giphy-com (https://giphy.com/, ready_s=5.3, в1)

### 15. [TIMEOUT/HUNG] stderr: ⤷ preload js [medium] URL

- сайтов: 3, вес: 9
- примеры: habr-com (https://habr.com/, ready_s=55.5, в5); nytimes-com (https://nytimes.com/, ready_s=33.7, в3); otto-de (https://otto.de/, ready_s=30.1, в1)

### 16. [a11y] stderr: не декодируется URL

- сайтов: 5, вес: 8
- примеры: duckduckgo-com (https://duckduckgo.com/, ready_s=2.6, в2); pravda-ru (https://pravda.ru/, ready_s=16.7, в2); walmart-com (https://walmart.com/, ready_s=12.5, в2); fidelity-com (https://fidelity.com/, ready_s=7.9, в1); figma-com (https://figma.com/, ready_s=12.7, в1)

### 17. [NET_FAIL] stderr: ошибка загрузки URL network error: too many redirects

- сайтов: 4, вес: 8
- примеры: epicgames-com (https://epicgames.com/, ready_s=6.5, в3); seznam-cz (https://seznam.cz/, ready_s=2.1, в2); springer-com (https://springer.com/, ready_s=5.3, в2); ameba-jp (https://www.ameba.jp/, ready_s=5.2, в1)

### 18. [a11y] stderr: [unhandled-rejection] typeerror: fetch: network error for URL

- сайтов: 2, вес: 8
- примеры: auto-ru (https://auto.ru/, ready_s=14.7, в5); openai-com (https://openai.com/, ready_s=14.9, в3)

### 19. [TIMEOUT/HUNG] stderr: не декодируется URL

- сайтов: 5, вес: 7
- примеры: fontanka-ru (https://fontanka.ru/, ready_s=32.1, в2); woman-ru (https://woman.ru/, ready_s=15.5, в2); corriere-it (https://corriere.it/, ready_s=35.4, в1); docomo-ne-jp (https://www.docomo.ne.jp/, ready_s=32.3, в1); lg-com (https://lg.com/, ready_s=46.0, в1)

### 20. [не наш] нет API: cannot read properties of null (reading 'eval')

- сайтов: 4, вес: 7
- примеры: nih-gov (https://nih.gov/, ready_s=6.3, в3); sourceforge-net (https://sourceforge.net/, ready_s=7.8, в2); ancestry-com (https://ancestry.com/, ready_s=10.8, в1); genius-com (https://genius.com/, ready_s=5.0, в1)

### 21. [BROKEN_RENDER] исключение: link hint fetch failed: url

- сайтов: 3, вес: 7
- примеры: icloud-com (https://icloud.com/, ready_s=10.7, в3); snapchat-com (https://snapchat.com/, ready_s=6.9, в3); starlink-com (https://starlink.com/, ready_s=2.9, в1)

### 22. [a11y] stderr: пропуск картинки URL network error: http N

- сайтов: 2, вес: 7
- примеры: rbc-ru (https://www.rbc.ru/, ready_s=4.9, в5); sport-express-ru (https://www.sport-express.ru/, ready_s=30.0, в2)

### 23. [a11y] исключение: @firebase/analytics: transactioninactiveerror: transaction is not active

- сайтов: 2, вес: 7
- примеры: ria-ru (https://ria.ru/, ready_s=34.9, в5); wps-com (https://wps.com/, ready_s=35.0, в2)

### 24. [blank:near-empty] нет API: cannot read properties of null (reading 'document')

- сайтов: 3, вес: 6
- примеры: samsung-com (https://samsung.com/, ready_s=10.2, в3); ups-com (https://ups.com/, ready_s=28.0, в2); onet-pl (https://onet.pl/, ready_s=15.4, в1)

### 25. [a11y] исключение: script load failed: url typeerror: fetch: network error for url

- сайтов: 2, вес: 6
- примеры: kp-ru (https://kp.ru/, ready_s=30.3, в5); chess-com (https://chess.com/, ready_s=11.6, в1)

### 26. [pixels] stderr: [js warn] performanceobserver: unsupported entrytype first-input

- сайтов: 2, вес: 5
- примеры: nic-ru (https://nic.ru/, ready_s=1.9, в3); thepoizon-ru (https://thepoizon.ru/, ready_s=5.2, в2)

### 27. [TIMEOUT/HUNG] stderr: [unhandled-rejection] error: battery status api is not supported

- сайтов: 2, вес: 4
- примеры: disneyplus-com (https://disneyplus.com/, ready_s=37.1, в2); vesti-ru (https://vesti.ru/, ready_s=31.4, в2)

### 28. [a11y] исключение: error: battery status api is not supported

- сайтов: 2, вес: 4
- примеры: airbnb-com (https://airbnb.com/, ready_s=12.3, в2); kion-ru (https://kion.ru/, ready_s=24.7, в2)

### 29. [pixels] нет API: cannot read properties of undefined (reading 'enumerable')

- сайтов: 2, вес: 4
- примеры: scribd-com (https://scribd.com/, ready_s=14.5, в2); slideshare-net (https://slideshare.net/, ready_s=8.3, в2)

### 30. [прочее] исключение: script load failed: url typeerror: fetch: network error for url

- сайтов: 2, вес: 4
- примеры: aif-ru (https://aif.ru/, ready_s=26.2, в2); rg-ru (https://rg.ru/, ready_s=27.1, в2)

### 31. [TIMEOUT/HUNG] stderr: [js error] track load failed: empty src

- сайтов: 2, вес: 3
- примеры: mdpi-com (https://mdpi.com/, ready_s=34.2, в2); wise-com (https://wise.com/, ready_s=32.1, в1)

### 32. [TIMEOUT/HUNG] stderr: [js error] uncaught typeerror: cannot read properties of undefined (reading 'get')

- сайтов: 2, вес: 3
- примеры: grammarly-com (https://grammarly.com/, ready_s=31.5, в2); kickstarter-com (https://kickstarter.com/, ready_s=30.8, в1)

### 33. [TIMEOUT/HUNG] stderr: повтор после сорванного tls-рукопожатия: URL

- сайтов: 2, вес: 3
- примеры: ntv-ru (https://ntv.ru/, ready_s=46.4, в2); t-mobile-com (https://t-mobile.com/, ready_s=30.2, в1)

### 34. [a11y] исключение: typeerror: e.children is not iterable

- сайтов: 2, вес: 3
- примеры: litmarket-ru (https://litmarket.ru/, ready_s=12.8, в2); clever-com (https://clever.com/, ready_s=22.0, в1)

### 35. [a11y] нет API: cannot read properties of undefined (reading 'get')

- сайтов: 2, вес: 3
- примеры: eventbrite-com (https://eventbrite.com/, ready_s=7.2, в2); gofundme-com (https://gofundme.com/, ready_s=27.7, в1)

### 36. [pixels] stderr: не декодируется URL

- сайтов: 2, вес: 3
- примеры: fandom-com (https://fandom.com/, ready_s=17.9, в2); curseforge-com (https://curseforge.com/, ready_s=7.9, в1)

### 37. [не наш] stderr: [worker] fetch: URL network error

- сайтов: 2, вес: 3
- примеры: mobile-de (https://mobile.de/, ready_s=3.2, в2); hilton-com (https://hilton.com/, ready_s=3.0, в1)

### 38. [a11y] нет API: cannot read properties of null (reading 'insertadjacenthtml')

- сайтов: 2, вес: 2
- примеры: mercadolibre-com-ar (https://mercadolibre.com.ar/, ready_s=9.6, в1); mercadolivre-com-br (https://mercadolivre.com.br/, ready_s=11.6, в1)

### 39. [pixels] исключение: script load failed: url error: http N

- сайтов: 2, вес: 2
- примеры: biblegateway-com (https://biblegateway.com/, ready_s=7.3, в1); hltv-org (https://hltv.org/, ready_s=31.0, в1)

### Единичные причины (по одному сайту; в кластеры не сведены)

- **не наш**: 8 сайтов, вес 14 — godaddy-com, bloomberg-com, domclick-ru, marriott-com, sportmaster-ru, coupang-com, hm-com, kohls-com
- **TIMEOUT/HUNG**: 66 сайтов, вес 132 — rutube-ru, tbank-ru, adobe-com, bbc-com, beeline-ru, chatgpt-com, discord-com, ebay-com, github-com, medium-com, qq-com, raiffeisen-ru, t-online-de, vkvideo-ru, zoom-us, alibaba-com, apnews-com, asus-com, ati-su, cnbc-com, discogs-com, foxnews-com, independent-co-uk, iz-ru, jotform-com …
- **NET_FAIL**: 3 сайтов, вес 5 — aniwaves-ru, tgstat-ru, webmd-com
- **blank:near-empty**: 42 сайтов, вес 84 — gazeta-ru, lenta-ru, mirtesen-ru, roblox-com, theguardian-com, aviasales-ru, b2b-center-ru, bild-de, cbr-ru, claude-ai, dailymail-com, dailymotion-com, ea-com, eapteka-ru, film-ru, fiverr-com, free-fr, ikea-com, kaspersky-ru, khl-ru, liveinternet-ru, mlb-com, mosreg-ru, nike-com, nintendo-com …
- **BROKEN_RENDER**: 19 сайтов, вес 55 — 2gis-ru, gosuslugi-ru, mail-ru, vk-com, wildberries-ru, amazon-com, pinterest-com, spotify-com, tiktok-com, imgur-com, kuper-ru, letu-ru, mvideo-ru, roku-com, twitch-tv, viber-com, vkplay-ru, flipkart-com, ryanair-com
- **pixels**: 44 сайтов, вес 75 — hh-ru, 1tv-ru, accuweather-com, aliexpress-ru, bilibili-com, box-com, businessinsider-com, capcut-com, cbsnews-com, cdc-gov, cnews-ru, drive2-ru, duolingo-com, ft-com, gdebenz-ru, gitlab-com, huawei-com, lenovo-com, lesta-ru, line-me, saby-ru, surveymonkey-com, tanki-su, tp-link-com, weather-com …
- **a11y**: 69 сайтов, вес 150 — drom-ru, pikabu-ru, rambler-ru, sports-ru, apple-com, askona-ru, cdek-ru, flickr-com, instagram-com, megafon-ru, sciencedirect-com, smi2-ru, soundcloud-com, x5-ru, ameblo-jp, atlassian-com, autodesk-com, bestbuy-com, brave-com, cambridge-org, citilink-ru, deepl-com, dnevnik-ru, fotostrana-ru, gid-ru …
- **прочее**: 17 сайтов, вес 45 — avito-ru, kinopoisk-ru, market-yandex-ru, baidu-com, ivi-ru, mts-ru, academia-edu, agoda-com, aliexpress-com, cikrf-ru, cyberleninka-ru, e1-ru, garant-ru, iamcook-ru, temu-com, vz-ru, kwai-com

## Топ-30 сайтов веса 3 и их класс

| сайт | класс | причина | ready_s |
|---|---|---|---|
| amazon-com | BROKEN_RENDER | stderr: iframe: пропуск bg-картинки data:image/png;baseN,ivborwNkggbxNeqzqbonoddNucbhwNssi | 9.7 |
| icloud-com | BROKEN_RENDER | исключение: link hint fetch failed: url | 10.7 |
| pinterest-com | BROKEN_RENDER | stderr: [js warn] performanceobserver: unsupported entrytype element | 9.6 |
| snapchat-com | BROKEN_RENDER | исключение: link hint fetch failed: url | 6.9 |
| spotify-com | BROKEN_RENDER | без улик | 10.0 |
| tiktok-com | BROKEN_RENDER | нет API: cannot read properties of undefined (reading 'tostring') | 22.4 |
| alfabank-ru | NET_FAIL | сеть: ✗ URL (tls: TLS handshake: untrusted certificate chain: Unkn | 1.4 |
| epicgames-com | NET_FAIL | stderr: ошибка загрузки URL network error: too many redirects | 6.5 |
| vtb-ru | NET_FAIL | сеть: ✗ URL (tls: TLS handshake: untrusted certificate chain: Unkn | 2.8 |
| adobe-com | TIMEOUT/HUNG | stderr: пропуск картинки ./media_NdNcNdNcNabNbdNdNafNdNcNfNbbc.webp?width=N&format=webply& | 36.5 |
| bbc-com | TIMEOUT/HUNG | stderr: @font-face «freight disp light»: не загружен /URL | 30.5 |
| beeline-ru | TIMEOUT/HUNG | stderr: [js error] uncaught referenceerror: $ is not defined | 34.9 |
| chatgpt-com | TIMEOUT/HUNG | stderr: не декодируется #a: формат изображения не распознан по сигнатуре | 30.1 |
| cloudflare-com | TIMEOUT/HUNG | без улик | 30.1 |
| discord-com | TIMEOUT/HUNG | stderr: ✗ URL (read: hN co | 30.8 |
| dropbox-com | TIMEOUT/HUNG | без улик | 32.1 |
| ebay-com | TIMEOUT/HUNG | stderr: [js] rive wasm load error details: [object object] | 32.1 |
| github-com | TIMEOUT/HUNG | stderr: не декодируется //fls-na.amazon.com/N/remote-weblab-triggers/N/oe/atvpdkikxNder:N- | 33.7 |
| imgsrc-ru | TIMEOUT/HUNG | stderr: [js websocket] connect error: network error: ws: read response: failed to fill who | 30.3 |
| intuit-com | TIMEOUT/HUNG | без улик | 32.1 |
| magnit-ru | TIMEOUT/HUNG | без улик | 30.1 |
| medium-com | TIMEOUT/HUNG | stderr: thread 'lumen-browser' (N) panicked at crates\engine\layout\src\box_tree\image_req | 29.2 |
| microsoft-com | TIMEOUT/HUNG | без улик | 35.7 |
| nytimes-com | TIMEOUT/HUNG | stderr: ⤷ preload js [medium] URL | 33.7 |
| paypal-com | TIMEOUT/HUNG | без улик | 35.4 |
| qq-com | TIMEOUT/HUNG | stderr: [unhandled-rejection] indexsizeerror: the source width and height must be non-zero | 32.1 |
| raiffeisen-ru | TIMEOUT/HUNG | stderr: [js error] uncaught error: adfox#checkcontainer: element not found or is not insta | 38.2 |
| t-online-de | TIMEOUT/HUNG | stderr: [js error] uncaught typeerror: cannot read properties of undefined (reading 'confi | 37.2 |
| vimeo-com | TIMEOUT/HUNG | без улик | 30.1 |
| vkvideo-ru | TIMEOUT/HUNG | stderr: [js warn] [api-cache] idle gc failed transactioninactiveerror: transaction is not  | 31.3 |
