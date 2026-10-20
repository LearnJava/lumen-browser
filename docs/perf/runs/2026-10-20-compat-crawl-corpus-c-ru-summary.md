# Краулер паритета Lumen ↔ Chrome (COMPAT-CORPUS-1)

- коммит `fd25da24c`, lumen `D:\RustProjects\lumen-browser\.claude\worktrees\p2-work\target\dev-release\lumen.exe`, Chrome `153.0.8010.12`
- пороги: пиксели ≤ 35.0% блоков (16px, допуск 40), a11y ≥ 95%

## Итог

- **CPR: 15.2%** (66 из 434 сайтов с посчитанными проверками)
- проверки посчитаны у 434 из 500 сайтов (86.8%); эталон не открыл сайт: 66
- повторный прогон: 0 сайтов; флапы (разный исход проверки между прогонами): 0 (0.0%)

| проверка | прошло | из |
|---|---|---|
| status | 261 | 434 |
| walls | 230 | 434 |
| exceptions | 379 | 434 |
| pixels | 186 | 434 |
| a11y | 87 | 434 |

## Новые исключения по числу сайтов (автогруппировка)

- 8× `link hint fetch failed: url`
- 6× `script load failed: url typeerror: fetch: network error for url`
- 5× `typeerror`
- 3× `typeerror: cannot read properties of null (reading 'innerhtml')`
- 2× `syntaxerror: [object object] is not a valid selector`
- 2× `track load failed: empty src`
- 2× `typeerror: cannot read properties of undefined (reading 'indexof')`
- 2× `error: battery status api is not supported`
- 2× `stylesheet load failed: url error: http N`
- 2× `typeerror: cannot read properties of undefined (reading 'set')`
- 2× `referenceerror: runprogram is not defined`
- 2× `unhandled error: , with debug info:`
- 2× `script load failed: url error: http N`
- 2× `typeerror: cannot read properties of undefined (reading 'enumerable')`
- 1× `typeerror: cannot destructure property 'url' of '$' as it is undefined.`

## Лишние стены

- 169× `blank:near-empty`
- 31× `net-error`
- 2× `hard:cloudflare-chl`
- 1× `hard:akamai-reference`
- 1× `hard:datadome-host`

## Статусы Lumen (таксономия perf_audit)

TIMEOUT: 129, OK: 123, DEGRADED: 94, NET_FAIL: 54, SITE_REFUSED: 42, BROKEN_RENDER: 28, HUNG: 28, DEAD: 2

## Сайты не в паритете

| сайт | не прошло | детали |
|---|---|---|
| mail-ru | exceptions, a11y | exceptions: typeerror: cannot destructure property 'url' of '$' as it is undefined.; typee; a11y: 39/74 (Lumen 268) |
| yandex-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: diff 93.74% (порог 35.0%), маска 38.8%; a11y: 6/190 (Lumen 232) |
| nic-ru | status, pixels, a11y | status: chrome=200 lumen=301; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 7/91 (Lumen 236) |
| reg-ru | status, pixels, a11y | status: chrome=200 lumen=301; pixels: diff 90.27% (порог 35.0%), маска 20.3%; a11y: 15/131 (Lumen 240) |
| vk-com | pixels, a11y | pixels: diff 88.47% (порог 35.0%), маска 2.0%; a11y: 1/15 (Lumen 259) |
| mts-ru | exceptions, a11y | exceptions: typeerror: cannot read properties of null (reading 'innerhtml'); a11y: 1/3 (Lumen 335) |
| ozon-ru | status, walls, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; a11y: 16/144 (Lumen 254) |
| ok-ru | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| drom-ru | exceptions, a11y | exceptions: error: element to connect input does not exist.; a11y: 0/154 (Lumen 228) |
| ivi-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| gosuslugi-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: diff 65.9% (порог 35.0%), маска 38.5%; a11y: 10/53 (Lumen 237) |
| sberbank-ru | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| avito-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| wb-ru | status, pixels, a11y | status: chrome=200 lumen=498; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/8 (Lumen 233) |
| rt-ru | exceptions, pixels, a11y | exceptions: @import failed: url typeerror: fetch: network error for url; pixels: diff 49.29% (порог 35.0%), маска 88.1%; a11y: 146/157 (Lumen 388) |
| pikabu-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tbank-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| rustore-ru | a11y | a11y: 166/375 (Lumen 632) |
| t2-ru | status, pixels, a11y | status: chrome=200 lumen=503; pixels: diff 40.38% (порог 35.0%), маска 47.1%; a11y: 17/83 (Lumen 254) |
| megafon-ru | exceptions, a11y | exceptions: rangeerror: maximum call stack size exceeded; a11y: 8/124 (Lumen 256) |
| kontur-ru | exceptions, pixels, a11y | exceptions: an error occurred: invalidcharactererror: atob: invalid baseN string; pixels: diff 67.37% (порог 35.0%), маска 37.0%; a11y: 81/89 (Lumen 531) |
| gismeteo-ru | exceptions, a11y | exceptions: adfox transport frame was moved; a11y: 124/142 (Lumen 396) |
| rutube-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tabor-ru | status, a11y | status: chrome=200 lumen=404; a11y: 10/82 (Lumen 278) |
| max-ru | walls | walls: blank:near-empty |
| rbc-ru | a11y | a11y: 228/271 (Lumen 631) |
| mangabuff-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| dns-shop-ru | status, walls, a11y | status: chrome=200 lumen=308; walls: blank:near-empty; a11y: 1/154 (Lumen 236) |
| rambler-ru | a11y | a11y: 353/391 (Lumen 671) |
| beeline-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vkvideo-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vtb-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mirtesen-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| hh-ru | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| askona-ru | exceptions, a11y | exceptions: script load failed: url typeerror: fetch: network error for url; syntaxerror: ; a11y: 116/188 (Lumen 475) |
| kinopoisk-ru | a11y | a11y: 66/156 (Lumen 397) |
| mos-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ria-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| magnit-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| 2gis-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| x5-ru | pixels, a11y | pixels: diff 53.84% (порог 35.0%), маска 1.0%; a11y: 7/57 (Lumen 425) |
| cdek-ru | walls, a11y | walls: blank:near-empty; a11y: 12/166 (Lumen 232) |
| tutu-ru | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| smi2-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| alfabank-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| profi-ru | a11y | a11y: 123/140 (Lumen 393) |
| lenta-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vkusvill-ru | a11y | a11y: 156/178 (Lumen 565) |
| hse-ru | a11y | a11y: 18/149 (Lumen 472) |
| saby-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| cbr-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| cian-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| garant-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| goldapple-ru | a11y | a11y: 12/41 (Lumen 230) |
| tass-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: diff 36.88% (порог 35.0%), маска 64.1%; a11y: 1/132 (Lumen 232) |
| kp-ru | status, walls, pixels, a11y | status: chrome=200 lumen=303; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| finam-ru | walls, a11y | walls: blank:near-empty; a11y: 12/468 (Lumen 228) |
| banki-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| rzd-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vkplay-ru | walls, a11y | walls: blank:near-empty; a11y: 36/118 (Lumen 266) |
| fotostrana-ru | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mk-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sports-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| aif-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| thepoizon-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| pochta-ru | exceptions, a11y | exceptions: a client-side exception has occurred, see here for more info: url; syntaxerror; a11y: 5/128 (Lumen 228) |
| aliexpress-ru | status, a11y | status: chrome=429 lumen=200; a11y: 0/2 (Lumen 276) |
| litres-ru | a11y | a11y: 68/500 (Lumen 402) |
| vedomosti-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| liveinternet-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| rg-ru | exceptions, a11y | exceptions: script load failed: url typeerror: fetch: network error for url; a11y: 282/304 (Lumen 543) |
| kommersant-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| gazprombank-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| drive2-ru | a11y | a11y: 91/98 (Lumen 506) |
| iz-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| bspb-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| interfax-ru | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 158/178 (Lumen 448) |
| aviasales-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sovcombank-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vc-ru | a11y | a11y: 242/270 (Lumen 489) |
| auto-ru | a11y | a11y: 93/147 (Lumen 380) |
| azbyka-ru | a11y | a11y: 145/164 (Lumen 434) |
| uchi-ru | walls, a11y | walls: blank:near-empty; a11y: 2/60 (Lumen 240) |
| nspk-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| kion-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| playground-ru | exceptions, a11y | exceptions: typeerror: a.hasownproperty is not a function; a11y: 321/378 (Lumen 844) |
| smotrim-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: diff 93.98% (порог 35.0%), маска 96.5%; a11y: 14/198 (Lumen 260) |
| dellin-ru | status, pixels, a11y | status: chrome=200 lumen=401; pixels: diff 37.21% (порог 35.0%), маска 10.7%; a11y: 3/83 (Lumen 266) |
| stihi-ru | a11y | a11y: 129/140 (Lumen 405) |
| s7-ru | walls, a11y | walls: blank:near-empty; a11y: 1/102 (Lumen 268) |
| gazeta-ru | walls, a11y | walls: blank:near-empty; a11y: 1/107 (Lumen 271) |
| irecommend-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| eapteka-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 17/260 (Lumen 281) |
| gdebenz-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| lamoda-ru | walls, a11y | walls: blank:near-empty; a11y: 4/66 (Lumen 240) |
| prodoctorov-ru | walls, a11y | walls: blank:near-empty; a11y: 1/77 (Lumen 244) |
| matchtv-ru | a11y | a11y: 493/551 (Lumen 930) |
| domclick-ru | status, a11y | status: chrome=200 lumen=401; a11y: 9/164 (Lumen 260) |
| 1tv-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ntv-ru | exceptions | exceptions: hydration completed but contains mismatches.; track load failed: empty src; ty |
| pravda-ru | a11y | a11y: 140/188 (Lumen 474) |
| iamcook-ru | exceptions | exceptions: typeerror: cannot read properties of undefined (reading 'indexof') |
| mvideo-ru | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 221/257 (Lumen 566) |
| sport-express-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| popcornnews-ru | a11y | a11y: 181/201 (Lumen 428) |
| forbes-ru | pixels, a11y | pixels: diff 39.85% (порог 35.0%), маска 37.7%; a11y: 206/315 (Lumen 489) |
| yoomoney-ru | a11y | a11y: 8/37 (Lumen 291) |
| progorod43-ru | a11y | a11y: 74/83 (Lumen 382) |
| hibiny-ru | a11y | a11y: 264/379 (Lumen 700) |
| avidreaders-ru | a11y | a11y: 147/205 (Lumen 517) |
| litboom-ru | a11y | a11y: 183/229 (Lumen 488) |
| litgorod-ru | a11y | a11y: 740/1047 (Lumen 1376) |
| progoroduhta-ru | a11y | a11y: 94/110 (Lumen 406) |
| playwekings-ru | status, pixels, a11y | status: chrome=200 lumen=302; pixels: нет кадра; a11y: нет a11y-дерева |
| lipetsknews-ru | a11y | a11y: 48/55 (Lumen 361) |
| ngs-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mosreg-ru | walls, a11y | walls: blank:near-empty; a11y: 3/173 (Lumen 228) |
| evotor-ru | a11y | a11y: 76/130 (Lumen 470) |
| klerk-ru | walls, a11y | walls: blank:near-empty; a11y: 4/309 (Lumen 241) |
| gid-ru | a11y | a11y: 43/51 (Lumen 297) |
| nso-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| lesta-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ostrovok-ru | pixels, a11y | pixels: diff 66.1% (порог 35.0%), маска 27.5%; a11y: 11/290 (Lumen 420) |
| fontanka-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| happ-su | a11y | a11y: 2/11 (Lumen 283) |
| cloudpayments-ru | exceptions | exceptions: script load failed: url typeerror: fetch: network error for url |
| loveplanet-ru | a11y | a11y: 35/129 (Lumen 575) |
| khl-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| dnevnik-ru | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| detmir-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mamba-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| rp5-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| profinance-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vesti-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sutochno-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| letu-ru | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| urfu-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tanki-su | status, pixels, a11y | status: chrome=200 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| t-j-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| komus-ru | status, pixels, a11y | status: chrome=503 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| topwar-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| cnews-ru | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 364/518 (Lumen 820) |
| news-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| skyeng-ru | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of null (reading 'innerhtml'); pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 28/150 (Lumen 322) |
| b2b-center-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: diff 58.15% (порог 35.0%), маска 3.4%; a11y: 2/81 (Lumen 244) |
| vz-ru | exceptions | exceptions: typeerror: fn is not a function |
| dtf-ru | exceptions, a11y | exceptions: %chawk (N.N)%c transport sending error %o line-height: Nem;; a11y: 238/297 (Lumen 502) |
| spbu-ru | a11y | a11y: 226/279 (Lumen 649) |
| film-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| livelib-ru | a11y | a11y: 421/663 (Lumen 961) |
| sima-land-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ati-su | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| perekrestok-ru | status, a11y | status: chrome=403 lumen=200; a11y: 0/3 (Lumen 244) |
| woman-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mts-link-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| afisha-ru | walls, a11y | walls: blank:near-empty; a11y: 17/1020 (Lumen 234) |
| tatar-ru | a11y | a11y: 73/99 (Lumen 421) |
| tgstat-ru | exceptions, a11y | exceptions: referenceerror: jquery is not defined; typeerror: cannot convert undefined or ; a11y: 171/208 (Lumen 573) |
| vgtimes-ru | pixels | pixels: diff 35.81% (порог 35.0%), маска 21.4% |
| labirint-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ingos-ru | exceptions, a11y | exceptions: error typeerror: cannot read properties of null (reading 'innerhtml'); error: ; a11y: 287/359 (Lumen 623) |
| lemanapro-ru | status, a11y | status: chrome=200 lumen=401; a11y: 0/914 (Lumen 262) |
| citilink-ru | a11y | a11y: 158/441 (Lumen 427) |
| sportmaster-ru | status, walls, pixels, a11y | status: chrome=200 lumen=401; walls: blank:near-empty; pixels: diff 51.7% (порог 35.0%), маска 71.8%; a11y: 13/151 (Lumen 276) |
| kaspersky-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| megamarket-ru | status, a11y | status: chrome=403 lumen=200; a11y: 0/1 (Lumen 228) |
| eldorado-ru | walls | walls: blank:near-empty |
| nalog-gov-ru | exceptions, a11y | exceptions: stylesheet load failed: url error: http N; a11y: 81/101 (Lumen 405) |
| pfr-gov-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sfr-gov-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| microsoft-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| apple-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| amazon-com | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| github-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| wikipedia-org | a11y | a11y: 92/109 (Lumen 706) |
| icloud-com | walls, exceptions, pixels, a11y | walls: blank:near-empty; exceptions: link hint fetch failed: url; stylesheet load failed: url error: http N; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/7 (Lumen 242) |
| pinterest-com | a11y | a11y: 3/55 (Lumen 288) |
| tiktok-com | exceptions, pixels, a11y | exceptions: link hint fetch failed: data:;baseN,=; pixels: diff 100.0% (порог 35.0%), маска 28.3%; a11y: 15/55 (Lumen 248) |
| yahoo-com | exceptions, a11y | exceptions: link hint fetch failed: url; a11y: 57/113 (Lumen 327) |
| adobe-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| zoom-us | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| roblox-com | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of undefined (reading 'set'); pixels: diff 99.54% (порог 35.0%), маска 7.4%; a11y: 149/172 (Lumen 392) |
| baidu-com | a11y | a11y: 24/36 (Lumen 308) |
| qq-com | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vimeo-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| reddit-com | exceptions, a11y | exceptions: error: module 'data:text/javascript,%Na%N%N%N%N%N%N%N%Nwindow.sticky_canary%N%; a11y: 13/356 (Lumen 329) |
| epicgames-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| openai-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| samsung-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| dropbox-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nih-gov | status, exceptions, a11y | status: chrome=403 lumen=301; exceptions: referenceerror: runprogram is not defined; typeerror; unhandled error: , with ; a11y: 0/2 (Lumen 245) |
| tumblr-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| shopify-com | a11y | a11y: 19/108 (Lumen 501) |
| flickr-com | exceptions, pixels, a11y | exceptions: referenceerror: webfont is not defined; script load failed: url error: http N;; pixels: diff 98.18% (порог 35.0%), маска 58.8%; a11y: 60/66 (Lumen 363) |
| theguardian-com | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 240/258 (Lumen 626) |
| cnn-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ebay-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| forbes-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sciencedirect-com | exceptions, a11y | exceptions: syntaxerror: identifier 'pendodata' has already been declared; typeerror: b.ge; a11y: 0/3 (Lumen 377) |
| mit-edu | a11y | a11y: 49/56 (Lumen 325) |
| twitch-tv | a11y | a11y: 2/142 (Lumen 281) |
| researchgate-net | pixels | pixels: diff None% (порог 35.0%), маска 100.0% |
| stripe-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sourceforge-net | exceptions | exceptions: script load failed: url typeerror: fetch: network error for url |
| namecheap-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| roku-com | walls | walls: blank:near-empty |
| reuters-com | pixels | pixels: diff None% (порог 35.0%), маска 100.0% |
| springer-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 93.15% (порог 35.0%), маска 69.5%; a11y: 0/4 (Lumen 264) |
| www-gov-uk | pixels | pixels: diff 37.19% (порог 35.0%), маска 35.6% |
| nature-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 95.77% (порог 35.0%), маска 66.0%; a11y: 0/4 (Lumen 273) |
| steampowered-com | a11y | a11y: 3/183 (Lumen 508) |
| salesforce-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| alibaba-com | exceptions, pixels, a11y | exceptions: script load failed: ////buyercentral.alibaba.com/buyer/privacy/need_show_cooki; pixels: diff 85.44% (порог 35.0%), маска 84.0%; a11y: 4/24 (Lumen 404) |
| mi-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| wiley-com | exceptions | exceptions: typeerror: cannot read properties of undefined (reading 'enumerable') |
| etsy-com | pixels | pixels: diff None% (порог 35.0%), маска 100.0% |
| bilibili-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| naver-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| slack-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ea-com | walls, exceptions, pixels, a11y | walls: blank:near-empty; exceptions: error [statsig] a networking error occurred during post request to url network; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 2/81 (Lumen 229) |
| nasa-gov | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| brave-com | a11y | a11y: 126/137 (Lumen 458) |
| tp-link-com | status, pixels, a11y | status: chrome=404 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| cdc-gov | exceptions, pixels, a11y | exceptions: syntaxerror: [data-bs-toggle="collapse" is not a valid selector; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 25/47 (Lumen 525) |
| washingtonpost-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| gitlab-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tradingview-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nvidia-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| bloomberg-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| wsj-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| calendly-com | exceptions | exceptions: script load failed: url typeerror: fetch: network error for url |
| go-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| duckduckgo-com | a11y | a11y: 21/131 (Lumen 398) |
| steamcommunity-com | exceptions, a11y | exceptions: typeerror: cannot read properties of undefined (reading 'apply'); a11y: 10/39 (Lumen 541) |
| supercell-com | a11y | a11y: 70/80 (Lumen 313) |
| huawei-com | exceptions, pixels, a11y | exceptions: track load failed: empty src; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 49/73 (Lumen 491) |
| taobao-com | exceptions, a11y | exceptions: typeerror: cannot read properties of undefined (reading 'bind'); a11y: 116/188 (Lumen 2137) |
| duolingo-com | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 43/86 (Lumen 368) |
| businessinsider-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| grammarly-com | status, pixels, a11y | status: chrome=451 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| aliexpress-com | status, a11y | status: chrome=429 lumen=200; a11y: 0/2 (Lumen 321) |
| cnbc-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| walmart-com | exceptions | exceptions: link hint fetch failed: url |
| shein-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| npr-org | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| noaa-gov | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| scribd-com | exceptions, a11y | exceptions: link hint fetch failed: url; a11y: 2/3 (Lumen 255) |
| indiatimes-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| behance-net | a11y | a11y: 1/246 (Lumen 259) |
| foxnews-com | walls, a11y | walls: hard:akamai-reference; a11y: 0/1 (Lumen 262) |
| uol-com-br | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| capcut-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ft-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| fast-com | exceptions, a11y | exceptions: link hint fetch failed: url; a11y: 1/3 (Lumen 274) |
| free-fr | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tandfonline-com | walls, exceptions | walls: hard:cloudflare-chl; exceptions: typeerror; typeerror: i[ak(...)] is not a function |
| rakuten-co-jp | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| deviantart-com | status, walls, a11y | status: chrome=200 lumen=302; walls: net-error; a11y: 0/123 (Lumen 249) |
| tripadvisor-com | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: hard:datadome-host; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/136 (Lumen 248) |
| wps-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| threads-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| playstation-com | exceptions, pixels, a11y | exceptions: syntaxerror: [*/href="#a"] is not a valid selector; pixels: diff 38.36% (порог 35.0%), маска 49.2%; a11y: 59/75 (Lumen 392) |
| target-com | a11y | a11y: 0/1 (Lumen 269) |
| ovh-net | a11y | a11y: 11/17 (Lumen 323) |
| atlassian-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mdpi-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| accuweather-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| nintendo-com | exceptions | exceptions: link hint fetch failed: url |
| merriam-webster-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| jotform-com | a11y | a11y: 95/106 (Lumen 580) |
| nike-com | status, pixels, a11y | status: chrome=200 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| perplexity-ai | status, exceptions, a11y | status: chrome=403 lumen=301; exceptions: referenceerror: runprogram is not defined; typeerror; unhandled error: , with ; a11y: 0/2 (Lumen 269) |
| cambridge-org | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| kick-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| cbsnews-com | status, walls, pixels, a11y | status: chrome=200 lumen=406; walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/100 (Lumen 232) |
| yelp-com | status, pixels, a11y | status: chrome=403 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| airbnb-com | walls, exceptions, pixels, a11y | walls: blank:near-empty; exceptions: error: battery status api is not supported; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/115 (Lumen 265) |
| ring-com | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| marriott-com | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of undefined (reading 'call'); pixels: diff 73.29% (порог 35.0%), маска 53.6%; a11y: 0/84 (Lumen 258) |
| uber-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| box-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: net-error; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/141 (Lumen 270) |
| independent-co-uk | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| asus-com | a11y | a11y: 121/131 (Lumen 427) |
| allegro-pl | pixels | pixels: diff None% (порог 35.0%), маска 100.0% |
| mediafire-com | pixels | pixels: diff 72.43% (порог 35.0%), маска 4.7% |
| imgur-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mlb-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nikkei-com | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| seznam-cz | status, walls, a11y | status: chrome=200 lumen=301; walls: net-error; a11y: 15/522 (Lumen 233) |
| zillow-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| apnews-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| lenovo-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| ameblo-jp | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| fiverr-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| wp-pl | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| disneyplus-com | exceptions, pixels, a11y | exceptions: link hint fetch failed: url; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 15/16 (Lumen 252) |
| usps-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| discogs-com | status, walls, exceptions, pixels, a11y | status: chrome=200 lumen=403; walls: hard:cloudflare-chl; exceptions: typeerror; pixels: diff 51.24% (порог 35.0%), маска 0.0%; a11y: 1/214 (Lumen 256) |
| lefigaro-fr | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| academia-edu | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| agoda-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ryanair-com | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of null (reading 'innerhtml'); pixels: diff 39.66% (порог 35.0%), маска 47.2%; a11y: 111/122 (Lumen 388) |
| figma-com | a11y | a11y: 90/95 (Lumen 385) |
| nbcnews-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 52.26% (порог 35.0%), маска 0.0%; a11y: 0/14 (Lumen 241) |
| focus-de | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| daum-net | status, walls, pixels, a11y | status: chrome=200 lumen=307; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| coupang-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mayoclinic-org | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of null (reading 'document'); typeerror: can; pixels: diff 88.47% (порог 35.0%), маска 0.5%; a11y: 0/121 (Lumen 232) |
| latimes-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| lg-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| bankofamerica-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| kleinanzeigen-de | a11y | a11y: 0/1 (Lumen 248) |
| genius-com | status, walls, exceptions, pixels, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; exceptions: script load failed: url typeerror: fetch: network error for url; typeerror; pixels: diff 89.97% (порог 35.0%), маска 0.3%; a11y: 0/110 (Lumen 229) |
| state-gov | a11y | a11y: 66/73 (Lumen 684) |
| note-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| poki-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| character-ai | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| fidelity-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| hm-com | pixels | pixels: diff 75.98% (порог 35.0%), маска 0.1% |
| trendyol-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| corriere-it | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| xiaohongshu-com | status, walls, pixels, a11y | status: chrome=200 lumen=307; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| onet-pl | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| change-org | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nexusmods-com | exceptions, a11y | exceptions: script load failed: url error: http N; a11y: 97/121 (Lumen 378) |
| ancestry-com | status, walls, pixels, a11y | status: chrome=403 lumen=200; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| sofascore-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| kwai-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| costco-com | exceptions, a11y | exceptions: link hint fetch failed: url; a11y: 3/5 (Lumen 233) |
| flightradar24-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| clever-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| hilton-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| bol-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| nexon-com | status, pixels, a11y | status: chrome=200 lumen=302; pixels: нет кадра; a11y: нет a11y-дерева |
| docomo-ne-jp | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| letterboxd-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| theatlantic-com | status, walls, a11y | status: chrome=200 lumen=301; walls: net-error; a11y: 0/7 (Lumen 233) |
| huffpost-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| garmin-com | a11y | a11y: 77/84 (Lumen 539) |
| moneycontrol-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| ameba-jp | status, walls, a11y | status: chrome=200 lumen=302; walls: net-error; a11y: 0/128 (Lumen 264) |
| starlink-com | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 18/30 (Lumen 261) |
| ouest-france-fr | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| instructure-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| goal-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| terra-com-br | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| as-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| magnific-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| bfmtv-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| crazygames-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ilovepdf-com | exceptions, a11y | exceptions: syntaxerror: [object object] is not a valid selector; a11y: 71/76 (Lumen 422) |
| douyin-com | exceptions, pixels, a11y | exceptions: typeerror: right-hand side of 'instanceof' is not an object; pixels: diff 100.0% (порог 35.0%), маска 68.1%; a11y: 0/46 (Lumen 240) |
| tabelog-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: net-error; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/147 (Lumen 253) |
| huggingface-co | a11y | a11y: 67/89 (Lumen 335) |
| interia-pl | status, walls, a11y | status: chrome=200 lumen=301; walls: net-error; a11y: 0/518 (Lumen 237) |
| aparat-com | a11y | a11y: 105/173 (Lumen 450) |
| wordwall-net | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| finn-no | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| gsmarena-com | status, pixels, a11y | status: chrome=200 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| giphy-com | a11y | a11y: 23/40 (Lumen 294) |
| nypost-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| hltv-org | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| hotpepper-jp | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vivo-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| riotgames-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| lanacion-com-ar | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: нет кадра; a11y: нет a11y-дерева |
| one-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| clarin-com | status, walls, a11y | status: chrome=200 lumen=301; walls: net-error; a11y: 0/241 (Lumen 233) |

## Эталон не открыл сайт (вне CPR)

- imgsrc-ru: TimeoutError: timed out
- aniwaves-ru: TimeoutError: timed out
- lenreg-ru: net::ERR_NAME_NOT_RESOLVED
- cikrf-ru: net::ERR_CONNECTION_REFUSED
- leroymerlin-ru: net::ERR_NAME_NOT_RESOLVED
- youtube-com: TimeoutError: timed out
- instagram-com: net::ERR_CONNECTION_TIMED_OUT
- linkedin-com: TimeoutError: timed out
- x-com: TimeoutError: timed out
- t-me: net::ERR_CONNECTION_TIMED_OUT
- snapchat-com: TimeoutError: timed out
- godaddy-com: TimeoutError: timed out
- nytimes-com: TimeoutError: timed out
- discord-com: TimeoutError: timed out
- t-online-de: TimeoutError: timed out
- soundcloud-com: TimeoutError: timed out
- medium-com: TimeoutError: timed out
- bbc-com: TimeoutError: timed out
- hubspot-com: TimeoutError: timed out
- linktr-ee: TimeoutError: timed out
- hp-com: net::ERR_NAME_NOT_RESOLVED
- dell-com: net::ERR_CONNECTION_TIMED_OUT
- bsky-app: TimeoutError: timed out
- unsplash-com: TimeoutError: timed out
- line-me: TimeoutError: timed out
- claude-ai: net::ERR_CONNECTION_TIMED_OUT
- dailymotion-com: TimeoutError: timed out
- substack-com: TimeoutError: timed out
- fandom-com: TimeoutError: timed out
- aol-com: TimeoutError: timed out
- pixabay-com: TimeoutError: timed out
- speedtest-net: TimeoutError: timed out
- goodreads-com: TimeoutError: timed out
- quora-com: TimeoutError: timed out
- eventbrite-com: TimeoutError: timed out
- wix-com: net::ERR_CONNECTION_RESET
- ups-com: TimeoutError: timed out
- elpais-com: TimeoutError: timed out
- britannica-com: TimeoutError: timed out
- lemonde-fr: TimeoutError: timed out
- viber-com: TimeoutError: timed out
- itch-io: TimeoutError: timed out
- bild-de: TimeoutError: timed out
- welt-de: TimeoutError: timed out
- wattpad-com: TimeoutError: timed out
- pexels-com: TimeoutError: timed out
- curseforge-com: TimeoutError: timed out
- chess-com: TimeoutError: timed out
- biblegateway-com: TimeoutError: timed out
- classlink-com: TimeoutError: timed out
- mercadolivre-com-br: TimeoutError: timed out
- mercadolibre-com-ar: TimeoutError: timed out
- flipkart-com: net::ERR_CONNECTION_TIMED_OUT
- repubblica-it: TimeoutError: timed out
- caixa-gov-br: TimeoutError: timed out
- flashscore-com: TimeoutError: timed out
- webmd-com: TimeoutError: timed out
- konami-net: net::ERR_NAME_NOT_RESOLVED
- elmundo-es: TimeoutError: timed out
- ecosia-org: TimeoutError: timed out
- infobae-com: TimeoutError: timed out
- n-tv-de: TimeoutError: timed out
- marca-com: TimeoutError: timed out
- gofundme-com: TimeoutError: timed out
- aftonbladet-se: TimeoutError: timed out
- kickstarter-com: TimeoutError: timed out
