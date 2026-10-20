# Краулер паритета Lumen ↔ Chrome (COMPAT-CORPUS-1)

- коммит `f3c79fa42`, lumen `target\dev-release\lumen.exe`, Chrome `153.0.8010.12`
- пороги: пиксели ≤ 35.0% блоков (16px, допуск 40), a11y ≥ 95%

## Итог

- **CPR: 4.5%** (5 из 111 сайтов с посчитанными проверками)
- проверки посчитаны у 111 из 112 сайтов (99.1%); эталон не открыл сайт: 1
- повторный прогон: 106 сайтов; флапы (разный исход проверки между прогонами): 48 (43.2%)

| проверка | прошло | из |
|---|---|---|
| status | 76 | 111 |
| walls | 74 | 111 |
| exceptions | 100 | 111 |
| pixels | 52 | 111 |
| a11y | 11 | 111 |

## Новые исключения по числу сайтов (автогруппировка)

- 1× `typeerror: cannot read properties of undefined (reading 'document')`
- 1× `typeerror: e.children is not iterable`
- 1× `immediately task failed: typeerror: cannot read properties of undefined (reading 'tostring')`
- 1× `legacydomready task failed: typeerror: cannot convert undefined or null to object`
- 1× `legacydomready task failed: typeerror: cannot read properties of null (reading 'navigator')`
- 1× `script load failed: url error: http N`
- 1× `typeerror: cannot read properties of undefined (reading 'startswith')`
- 1× `script load failed: url error: module 'url not found`
- 1× `cannot read properties of undefined (reading 'postmessage')`
- 1× `typeerror: cannot read properties of undefined (reading 'postmessage')`
- 1× `typeerror: cannot read properties of null (reading 'document')`
- 1× `typeerror: this.videoinfo.el.load is not a function`
- 1× `typeerror: cannot read properties of null (reading 'state')`
- 1× `typeerror: cannot read properties of null (reading 'N')`
- 1× `error: battery status api is not supported`

## Лишние стены

- 35× `blank:near-empty`
- 1× `hard:datadome-host`
- 1× `hard:akamai-reference`
- 1× `hard:cloudflare-chl`

## Статусы Lumen (таксономия perf_audit)

OK: 25, NET_FAIL: 22, TIMEOUT: 19, SITE_REFUSED: 15, DEGRADED: 12, BROKEN_RENDER: 11, HUNG: 8

## Сайты не в паритете

| сайт | не прошло | детали |
|---|---|---|
| ya | pixels | pixels: diff 99.08% (порог 35.0%), маска 37.2% |
| rust-lang | a11y | a11y: 52/57 (Lumen 309) |
| lenta | exceptions | exceptions: typeerror: cannot read properties of undefined (reading 'document'); typeerror |
| github | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| stackoverflow | a11y | a11y: нет a11y-дерева |
| crates | walls | walls: blank:near-empty |
| docs-rs | a11y | a11y: 21/24 (Lumen 287) |
| ria | a11y | a11y: 154/211 (Lumen 558) |
| habr | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mdn | pixels, a11y | pixels: diff 77.48% (порог 35.0%), маска 10.2%; a11y: 0/87 (Lumen 273) |
| rbc | a11y | a11y: 251/271 (Lumen 641) |
| google | pixels, a11y | pixels: diff 73.65% (порог 35.0%), маска 0.6%; a11y: 1/8 (Lumen 277) |
| youtube | status, pixels, a11y | status: chrome=200 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| chatgpt | status, pixels, a11y | status: chrome=200 lumen=403; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 2/24 (Lumen 249) |
| reddit | a11y | a11y: нет a11y-дерева |
| wikipedia | a11y | a11y: 92/109 (Lumen 710) |
| whatsapp | status, a11y | status: chrome=200 lumen=None; a11y: нет a11y-дерева |
| yahoo | pixels, a11y | pixels: diff 97.61% (порог 35.0%), маска 77.6%; a11y: 58/116 (Lumen 324) |
| amazon | walls, a11y | walls: blank:near-empty; a11y: нет a11y-дерева |
| tiktok | exceptions, a11y | exceptions: immediately task failed: typeerror: cannot read properties of undefined (readi; a11y: 0/4 (Lumen 262) |
| duckduckgo | a11y | a11y: 21/133 (Lumen 373) |
| bing | exceptions, a11y | exceptions: script load failed: url error: module 'url not found; a11y: 1/189 (Lumen 293) |
| yahoo-jp | status, a11y | status: chrome=403 lumen=200; a11y: 3/20 (Lumen 534) |
| netflix | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/52 (Lumen 262) |
| weather | status, walls, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; a11y: 0/64 (Lumen 268) |
| naver | exceptions, a11y | exceptions: cannot read properties of undefined (reading 'postmessage'); typeerror: cannot; a11y: 103/156 (Lumen 503) |
| temu | status, walls, pixels, a11y | status: chrome=200 lumen=400; walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/6 (Lumen 232) |
| pinterest | pixels, a11y | pixels: diff 91.32% (порог 35.0%), маска 25.6%; a11y: 3/59 (Lumen 284) |
| live | walls, a11y | walls: blank:near-empty; a11y: 0/166 (Lumen 248) |
| microsoft | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| baidu | a11y | a11y: 24/36 (Lumen 318) |
| canva | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| twitch | a11y | a11y: 19/151 (Lumen 411) |
| claude | a11y | a11y: нет a11y-дерева |
| fandom | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| samsung | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of null (reading 'document'); typeerror: thi; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/142 (Lumen 271) |
| globo | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/449 (Lumen 277) |
| nytimes | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ebay | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| zoom | pixels, a11y | pixels: diff 78.93% (порог 35.0%), маска 56.8%; a11y: 36/138 (Lumen 431) |
| spotify | pixels, a11y | pixels: diff 88.1% (порог 35.0%), маска 62.0%; a11y: 8/232 (Lumen 267) |
| walmart | exceptions, a11y | exceptions: typeerror: cannot read properties of null (reading 'state'); a11y: 38/63 (Lumen 314) |
| aliexpress | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of null (reading 'N'); pixels: diff 68.84% (порог 35.0%), маска 39.0%; a11y: 0/206 (Lumen 250) |
| imdb | a11y | a11y: 71/76 (Lumen 353) |
| quora | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| bbc | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| cnn | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| etsy | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: hard:datadome-host; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/11 (Lumen 283) |
| dropbox | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| indeed | status, a11y | status: chrome=200 lumen=403; a11y: 0/31 (Lumen 232) |
| adobe | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 8/37 (Lumen 293) |
| espn | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| accuweather | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| wikihow | status, a11y | status: chrome=200 lumen=None; a11y: нет a11y-дерева |
| rakuten | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tradingview | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| foxnews | status, walls, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; a11y: 0/650 (Lumen 240) |
| dailymail | walls, pixels, a11y | walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 16/1053 (Lumen 250) |
| wordpress | a11y | a11y: нет a11y-дерева |
| wsj | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| imgur | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| zillow | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| airbnb | walls, exceptions, pixels, a11y | walls: blank:near-empty; exceptions: error: battery status api is not supported; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/66 (Lumen 301) |
| flipkart | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: blank:near-empty, hard:akamai-reference; pixels: diff 99.71% (порог 35.0%), маска 61.4%; a11y: 2/129 (Lumen 286) |
| tumblr | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nih | walls, exceptions, a11y | walls: hard:cloudflare-chl; exceptions: typeerror; a11y: 0/56 (Lumen 232) |
| webmd | status, pixels, a11y | status: chrome=200 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| usatoday | status, walls, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; a11y: 1/175 (Lumen 248) |
| nbcnews | status, pixels, a11y | status: chrome=200 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| washingtonpost | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| cnbc | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| reuters | status, pixels, a11y | status: chrome=200 lumen=401; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/201 (Lumen 232) |
| uol | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mercadolivre | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| huggingface | a11y | a11y: 67/87 (Lumen 363) |
| perplexity | status, a11y | status: chrome=200 lumen=403; a11y: 0/29 (Lumen 263) |
| character-ai | exceptions, a11y | exceptions: error [statsig] a networking error occurred during post request to url network; a11y: 3/18 (Lumen 287) |
| cricbuzz | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| duolingo | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| coursera | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| udemy | status, a11y | status: chrome=200 lumen=None; a11y: нет a11y-дерева |
| khanacademy | pixels, a11y | pixels: diff 53.21% (порог 35.0%), маска 20.9%; a11y: 77/90 (Lumen 359) |
| uber | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| doordash | a11y | a11y: 0/2 (Lumen 263) |
| costco | status, exceptions, a11y | status: chrome=200 lumen=404; exceptions: link hint fetch failed: url; a11y: 0/202 (Lumen 237) |
| target | status, walls, pixels | status: chrome=200 lumen=429; walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0% |
| medium | status, a11y | status: chrome=200 lumen=None; a11y: нет a11y-дерева |
| soundcloud | status, pixels, a11y | status: chrome=200 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| mozilla | a11y | a11y: 109/153 (Lumen 727) |
| w3schools | pixels | pixels: diff None% (порог 35.0%), маска 100.0% |
| paypal | a11y | a11y: 0/42 (Lumen 331) |
| coinbase | a11y | a11y: нет a11y-дерева |
| snapchat | status, a11y | status: chrome=200 lumen=None; a11y: нет a11y-дерева |
| archive | a11y | a11y: 32/77 (Lumen 477) |
| theguardian | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| indiatimes | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ndtv | a11y | a11y: нет a11y-дерева |
| facebook | status, a11y | status: chrome=200 lumen=None; a11y: нет a11y-дерева |
| instagram | status, a11y | status: chrome=200 lumen=None; a11y: нет a11y-дерева |
| xcom | status, pixels, a11y | status: chrome=200 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| linkedin | status, a11y | status: chrome=200 lumen=None; a11y: нет a11y-дерева |
| discord | status, pixels, a11y | status: chrome=200 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| msft-login | a11y | a11y: 0/4 (Lumen 264) |
| gemini | walls, pixels, a11y | walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 2/9 (Lumen 286) |
| shopee | status, a11y | status: chrome=200 lumen=403; a11y: 0/34 (Lumen 274) |
| weibo | pixels | pixels: diff 37.61% (порог 35.0%), маска 18.9% |

## Эталон не открыл сайт (вне CPR)

- bilibili: ConnectionAbortedError: [WinError 10053] Программа на вашем хост-компьютере разорвала установленное подключение

## Флапы

- ya: status
- rust-lang: status
- lenta: pixels, a11y
- stackoverflow: status, exceptions, pixels
- ria: walls, pixels
- mdn: status, walls
- rbc: exceptions
- youtube: walls
- reddit: walls, exceptions, pixels
- whatsapp: walls, pixels
- amazon: pixels
- tiktok: pixels
- yahoo-jp: walls, exceptions, pixels
- canva: walls
- twitch: walls, pixels
- claude: walls, pixels
- nytimes: status
- zoom: status, walls
- imdb: walls, pixels
- quora: walls
- bbc: status, walls
- adobe: exceptions
- wikihow: walls, pixels
- wordpress: walls, exceptions, pixels
- webmd: walls
- nbcnews: walls
- reuters: walls
- mercadolivre: status, walls
- perplexity: exceptions
- cricbuzz: walls
- duolingo: walls
- udemy: walls, pixels
- uber: walls
- doordash: exceptions
- medium: pixels
- soundcloud: walls
- paypal: status, walls, pixels
- coinbase: walls, pixels
- snapchat: walls, pixels
- archive: pixels
- theguardian: status, walls
- ndtv: walls, pixels
- facebook: pixels
- instagram: pixels
- linkedin: walls, pixels
- discord: walls
- msft-login: pixels
- weibo: exceptions, a11y
