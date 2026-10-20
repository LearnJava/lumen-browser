# Краулер паритета Lumen ↔ Chrome (COMPAT-CORPUS-1)

- коммит `8778cac44`, lumen `D:\RustProjects\lumen-browser\.claude\worktrees\p2-work\target\dev-release\lumen.exe`, Chrome `153.0.8010.12`
- пороги: пиксели ≤ 35.0% блоков (16px, допуск 40), a11y ≥ 95%

## Итог

- **CPR: 5.9%** (29 из 494 сайтов с посчитанными проверками)
- проверки посчитаны у 494 из 500 сайтов (98.8%); эталон не открыл сайт: 6
- повторный прогон: 7 сайтов; флапы (разный исход проверки между прогонами): 3 (0.6%)

| проверка | прошло | из |
|---|---|---|
| status | 255 | 494 |
| walls | 206 | 494 |
| exceptions | 432 | 494 |
| pixels | 164 | 494 |
| a11y | 44 | 494 |

## Новые исключения по числу сайтов (автогруппировка)

- 6× `typeerror`
- 5× `script load failed: url error: http N`
- 4× `typeerror: cannot read properties of null (reading 'innerhtml')`
- 4× `script load failed: url typeerror: fetch: network error for url`
- 4× `link hint fetch failed: url`
- 3× `syntaxerror: [object object] is not a valid selector`
- 3× `error: battery status api is not supported`
- 3× `referenceerror: runprogram is not defined`
- 3× `unhandled error: , with debug info:`
- 2× `a client-side exception has occurred, see here for more info: url`
- 2× `typeerror: n.open is not a function`
- 2× `typeerror: cannot read properties of undefined (reading 'indexof')`
- 2× `typeerror: fn is not a function`
- 2× `stylesheet load failed: url error: http N`
- 2× `typeerror: cannot read properties of undefined (reading 'set')`

## Лишние стены

- 239× `blank:near-empty`
- 37× `net-error`
- 4× `hard:cloudflare-chl`
- 4× `hard:datadome-host`
- 4× `hard:akamai-reference`
- 1× `soft:blocked-generic-ru`
- 1× `soft:unsupported-browser-ru`
- 1× `soft:akamai-reference`
- 1× `soft:blocked-generic-en`

## Статусы Lumen (таксономия perf_audit)

TIMEOUT: 128, OK: 122, DEGRADED: 96, NET_FAIL: 48, HUNG: 40, SITE_REFUSED: 39, BROKEN_RENDER: 20, DEAD: 7

## Сайты не в паритете

| сайт | не прошло | детали |
|---|---|---|
| mail-ru | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| yandex-ru | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nic-ru | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 84/91 (Lumen 364) |
| reg-ru | a11y | a11y: 99/129 (Lumen 411) |
| vk-com | pixels, a11y | pixels: diff 88.47% (порог 35.0%), маска 2.0%; a11y: 1/15 (Lumen 277) |
| mts-ru | exceptions, a11y | exceptions: typeerror: cannot read properties of null (reading 'innerhtml'); a11y: 1/3 (Lumen 313) |
| ozon-ru | status, walls, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; a11y: 16/143 (Lumen 277) |
| ok-ru | exceptions, a11y | exceptions: error: failed to construct 'textdecoder': the encoding label provided ('ascii'; a11y: 4/31 (Lumen 333) |
| drom-ru | exceptions, a11y | exceptions: error: element to connect input does not exist.; a11y: 0/154 (Lumen 232) |
| ivi-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sberbank-ru | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| avito-ru | walls, a11y | walls: soft:blocked-generic-ru; a11y: 18/176 (Lumen 253) |
| wb-ru | status, pixels, a11y | status: chrome=200 lumen=498; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/1 (Lumen 233) |
| rt-ru | exceptions, pixels, a11y | exceptions: @import failed: url typeerror: fetch: network error for url; pixels: diff 49.29% (порог 35.0%), маска 88.1%; a11y: 80/157 (Lumen 314) |
| pikabu-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tbank-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| rustore-ru | a11y | a11y: 164/375 (Lumen 610) |
| t2-ru | status, a11y | status: chrome=200 lumen=503; a11y: 17/70 (Lumen 238) |
| megafon-ru | exceptions, a11y | exceptions: rangeerror: maximum call stack size exceeded; script load failed: url typeerro; a11y: 8/124 (Lumen 240) |
| kontur-ru | pixels, a11y | pixels: diff 67.8% (порог 35.0%), маска 36.3%; a11y: 81/89 (Lumen 516) |
| gismeteo-ru | exceptions, a11y | exceptions: adfox transport frame was moved; a11y: 123/142 (Lumen 382) |
| rutube-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tabor-ru | status, a11y | status: chrome=200 lumen=404; a11y: 10/82 (Lumen 266) |
| max-ru | walls | walls: blank:near-empty |
| rbc-ru | a11y | a11y: 222/281 (Lumen 598) |
| mangabuff-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| dns-shop-ru | status, a11y | status: chrome=200 lumen=308; a11y: 1/139 (Lumen 236) |
| rambler-ru | a11y | a11y: 346/451 (Lumen 661) |
| beeline-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vkvideo-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vtb-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mirtesen-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| hh-ru | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| askona-ru | exceptions, a11y | exceptions: syntaxerror: invalid or unexpected token; a11y: 116/175 (Lumen 441) |
| kinopoisk-ru | walls, a11y | walls: blank:near-empty; a11y: 3/147 (Lumen 232) |
| mos-ru | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 68/166 (Lumen 1149) |
| ria-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| magnit-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| 2gis-ru | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 41/48 (Lumen 297) |
| x5-ru | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| cdek-ru | walls, a11y | walls: blank:near-empty; a11y: 12/165 (Lumen 232) |
| tutu-ru | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 149/190 (Lumen 615) |
| smi2-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| alfabank-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| profi-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| lenta-ru | exceptions | exceptions: initsso error typeerror: fetch: network error for url; typeerror: e.children i |
| vkusvill-ru | exceptions, a11y | exceptions: typeerror: prevel.addeventlistener is not a function; a11y: 156/178 (Lumen 581) |
| hse-ru | a11y | a11y: 18/151 (Lumen 488) |
| saby-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| cbr-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| cian-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| garant-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| goldapple-ru | a11y | a11y: 2/24 (Lumen 230) |
| tass-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: diff 50.07% (порог 35.0%), маска 57.3%; a11y: 1/124 (Lumen 232) |
| kp-ru | status, walls, pixels, a11y | status: chrome=200 lumen=303; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| finam-ru | walls, a11y | walls: blank:near-empty; a11y: 12/471 (Lumen 228) |
| banki-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| rzd-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vkplay-ru | walls, a11y | walls: blank:near-empty; a11y: 28/30 (Lumen 303) |
| fotostrana-ru | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mk-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sports-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| aif-ru | exceptions, a11y | exceptions: feed_container_is_narrow feed container with id yandex_rtb_r-a-N-N is narrow; a11y: 377/399 (Lumen 721) |
| thepoizon-ru | pixels, a11y | pixels: diff 99.84% (порог 35.0%), маска 81.9%; a11y: 48/81 (Lumen 440) |
| pochta-ru | exceptions, a11y | exceptions: a client-side exception has occurred, see here for more info: url; syntaxerror; a11y: 5/128 (Lumen 236) |
| aliexpress-ru | status, pixels, a11y | status: chrome=429 lumen=200; pixels: нет кадра; a11y: нет a11y-дерева |
| litres-ru | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| vedomosti-ru | walls, exceptions, pixels, a11y | walls: blank:near-empty; exceptions: %cmain.ts:error%c vite ssr/client; typeerror: cannot read properties of undefi; pixels: diff 98.39% (порог 35.0%), маска 39.4%; a11y: 4/425 (Lumen 236) |
| liveinternet-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| rg-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| kommersant-ru | pixels, a11y | pixels: diff 76.29% (порог 35.0%), маска 55.6%; a11y: 235/286 (Lumen 731) |
| gazprombank-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| drive2-ru | a11y | a11y: 92/98 (Lumen 492) |
| iz-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| bspb-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| interfax-ru | exceptions, pixels, a11y | exceptions: typeerror: n.open is not a function; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 151/167 (Lumen 422) |
| aviasales-ru | walls, a11y | walls: blank:near-empty; a11y: 16/275 (Lumen 252) |
| sovcombank-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vc-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| auto-ru | a11y | a11y: 92/140 (Lumen 380) |
| azbyka-ru | a11y | a11y: 145/164 (Lumen 434) |
| uchi-ru | walls, a11y | walls: blank:near-empty; a11y: 2/60 (Lumen 240) |
| nspk-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| kion-ru | exceptions, a11y | exceptions: error: battery status api is not supported; a11y: 72/148 (Lumen 409) |
| playground-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sportbox-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| smotrim-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| dellin-ru | status, pixels, a11y | status: chrome=200 lumen=401; pixels: diff 36.93% (порог 35.0%), маска 6.4%; a11y: 1/19 (Lumen 232) |
| stihi-ru | a11y | a11y: 129/140 (Lumen 372) |
| s7-ru | walls, a11y | walls: blank:near-empty; a11y: 1/102 (Lumen 236) |
| gazeta-ru | walls, a11y | walls: blank:near-empty; a11y: 1/109 (Lumen 240) |
| irecommend-ru | a11y | a11y: 196/218 (Lumen 668) |
| eapteka-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| gdebenz-ru | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 68/170 (Lumen 864) |
| lamoda-ru | walls, a11y | walls: blank:near-empty; a11y: 4/66 (Lumen 240) |
| prodoctorov-ru | walls, a11y | walls: blank:near-empty; a11y: 1/77 (Lumen 244) |
| aniwaves-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| matchtv-ru | exceptions, pixels, a11y | exceptions: link hint fetch failed: url; pixels: diff 38.07% (порог 35.0%), маска 77.4%; a11y: 504/571 (Lumen 930) |
| domclick-ru | status, a11y | status: chrome=200 lumen=401; a11y: 11/186 (Lumen 260) |
| 1tv-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ntv-ru | exceptions | exceptions: hydration completed but contains mismatches.; typeerror: cannot read propertie |
| pravda-ru | a11y | a11y: 151/188 (Lumen 441) |
| iamcook-ru | exceptions | exceptions: typeerror: cannot read properties of undefined (reading 'indexof') |
| mvideo-ru | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| soccer365-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sport-express-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| popcornnews-ru | a11y | a11y: 180/200 (Lumen 439) |
| forbes-ru | status, walls, pixels, a11y | status: chrome=200 lumen=308; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| yoomoney-ru | a11y | a11y: 8/37 (Lumen 303) |
| progorod43-ru | a11y | a11y: 78/83 (Lumen 394) |
| hibiny-ru | walls, a11y | walls: blank:near-empty; a11y: 1/378 (Lumen 262) |
| avidreaders-ru | a11y | a11y: 147/205 (Lumen 526) |
| litboom-ru | a11y | a11y: 183/229 (Lumen 497) |
| litgorod-ru | a11y | a11y: 751/1047 (Lumen 1385) |
| progoroduhta-ru | a11y | a11y: 91/108 (Lumen 412) |
| playwekings-ru | status, pixels, a11y | status: chrome=200 lumen=302; pixels: нет кадра; a11y: нет a11y-дерева |
| lipetsknews-ru | a11y | a11y: 60/73 (Lumen 359) |
| ngs-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mosreg-ru | walls, a11y | walls: blank:near-empty; a11y: 3/173 (Lumen 228) |
| evotor-ru | a11y | a11y: 76/130 (Lumen 470) |
| klerk-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| gid-ru | a11y | a11y: 43/51 (Lumen 297) |
| nso-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| lesta-ru | walls, exceptions, pixels, a11y | walls: soft:unsupported-browser-ru; exceptions: syntaxerror: failed to execute 'insertrule' on 'cssstylesheet': the supplied t; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 18/27 (Lumen 419) |
| ostrovok-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| fontanka-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| happ-su | exceptions, a11y | exceptions: backup wasm url failed: section N out of order; buffered fallback failed: sect; a11y: 2/11 (Lumen 283) |
| e1-ru | exceptions | exceptions: typeerror: n.open is not a function |
| cloudpayments-ru | exceptions | exceptions: script load failed: url typeerror: fetch: network error for url |
| loveplanet-ru | a11y | a11y: 39/124 (Lumen 537) |
| khl-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| dnevnik-ru | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| detmir-ru | exceptions, a11y | exceptions: script load failed: url error: http N; a11y: 58/91 (Lumen 547) |
| mamba-ru | exceptions, a11y | exceptions: typeerror: cannot read properties of null (reading 'N'); a11y: 0/26 (Lumen 248) |
| rp5-ru | a11y | a11y: 247/771 (Lumen 1045) |
| profinance-ru | a11y | a11y: 114/138 (Lumen 371) |
| vesti-ru | a11y | a11y: 52/65 (Lumen 340) |
| sutochno-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| letu-ru | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| urfu-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tanki-su | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| t-j-ru | a11y | a11y: 1032/1142 (Lumen 1380) |
| topwar-ru | a11y | a11y: 66/73 (Lumen 397) |
| cnews-ru | exceptions, pixels, a11y | exceptions: ajax error:; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 363/428 (Lumen 820) |
| news-ru | a11y | a11y: 163/227 (Lumen 499) |
| skyeng-ru | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of null (reading 'innerhtml'); pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 28/150 (Lumen 330) |
| b2b-center-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: diff 57.88% (порог 35.0%), маска 3.4%; a11y: 2/80 (Lumen 244) |
| vz-ru | exceptions | exceptions: typeerror: fn is not a function |
| dtf-ru | exceptions, a11y | exceptions: %chawk (N.N)%c transport sending error %o line-height: Nem;; error: failed to ; a11y: 250/294 (Lumen 512) |
| spbu-ru | a11y | a11y: 233/279 (Lumen 649) |
| film-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| livelib-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sima-land-ru | a11y | a11y: 173/238 (Lumen 499) |
| ati-su | exceptions, a11y | exceptions: script load failed: url typeerror: fetch: network error for url; syntaxerror: ; a11y: 141/172 (Lumen 516) |
| perekrestok-ru | status, a11y | status: chrome=403 lumen=200; a11y: 0/3 (Lumen 274) |
| mts-link-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| afisha-ru | walls, a11y | walls: blank:near-empty; a11y: 17/1438 (Lumen 238) |
| tatar-ru | a11y | a11y: 72/98 (Lumen 424) |
| tgstat-ru | exceptions, a11y | exceptions: referenceerror: jquery is not defined; typeerror: cannot convert undefined or ; a11y: 171/208 (Lumen 574) |
| vgtimes-ru | pixels | pixels: diff 35.84% (порог 35.0%), маска 21.4% |
| labirint-ru | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ingos-ru | exceptions, a11y | exceptions: error typeerror: cannot read properties of null (reading 'innerhtml'); error: ; a11y: 308/533 (Lumen 626) |
| lemanapro-ru | status, a11y | status: chrome=200 lumen=401; a11y: 0/908 (Lumen 265) |
| citilink-ru | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| sportmaster-ru | status, pixels, a11y | status: chrome=200 lumen=401; pixels: нет кадра; a11y: нет a11y-дерева |
| kuper-ru | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| kaspersky-ru | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| megamarket-ru | status, a11y | status: chrome=403 lumen=200; a11y: 0/1 (Lumen 228) |
| eldorado-ru | walls | walls: blank:near-empty |
| nalog-gov-ru | exceptions, a11y | exceptions: stylesheet load failed: url error: http N; a11y: 86/103 (Lumen 405) |
| pfr-gov-ru | a11y | a11y: 93/109 (Lumen 529) |
| sfr-gov-ru | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| google-com | pixels, a11y | pixels: diff 73.39% (порог 35.0%), маска 0.6%; a11y: 1/8 (Lumen 273) |
| cloudflare-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| microsoft-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| apple-com | a11y | a11y: 117/131 (Lumen 373) |
| youtube-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| instagram-com | status, walls, a11y | status: chrome=200 lumen=None; walls: net-error; a11y: 0/76 (Lumen 253) |
| linkedin-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| amazon-com | a11y | a11y: 0/3 (Lumen 256) |
| github-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| wikipedia-org | a11y | a11y: 92/109 (Lumen 706) |
| netflix-com | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/52 (Lumen 232) |
| x-com | status, pixels, a11y | status: chrome=200 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| icloud-com | walls, exceptions, pixels, a11y | walls: blank:near-empty; exceptions: link hint fetch failed: url; stylesheet load failed: url error: http N; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/7 (Lumen 242) |
| pinterest-com | pixels, a11y | pixels: diff 92.24% (порог 35.0%), маска 26.8%; a11y: 3/59 (Lumen 301) |
| tiktok-com | exceptions, a11y | exceptions: immediately task failed: typeerror: cannot read properties of undefined (readi; a11y: 0/4 (Lumen 277) |
| yahoo-com | pixels, a11y | pixels: diff 99.14% (порог 35.0%), маска 80.4%; a11y: 57/121 (Lumen 328) |
| adobe-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| zoom-us | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| roblox-com | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of undefined (reading 'set'); pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 149/177 (Lumen 392) |
| spotify-com | pixels, a11y | pixels: diff 88.1% (порог 35.0%), маска 62.0%; a11y: 8/232 (Lumen 271) |
| baidu-com | a11y | a11y: 24/36 (Lumen 308) |
| qq-com | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vimeo-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| chatgpt-com | status, pixels, a11y | status: chrome=200 lumen=403; pixels: нет кадра; a11y: нет a11y-дерева |
| t-me | status, walls, a11y | status: chrome=200 lumen=None; walls: net-error; a11y: 1/56 (Lumen 233) |
| reddit-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| epicgames-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 99.63% (порог 35.0%), маска 62.1%; a11y: 0/195 (Lumen 241) |
| openai-com | exceptions, a11y | exceptions: <html>; error [statsig] a networking error occurred during post request to url; a11y: 11/110 (Lumen 351) |
| samsung-com | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of null (reading 'document'); typeerror: thi; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/142 (Lumen 244) |
| dropbox-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| intuit-com | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nih-gov | walls, exceptions, a11y | walls: hard:cloudflare-chl; exceptions: referenceerror: runprogram is not defined; typeerror; unhandled error: , with ; a11y: 0/2 (Lumen 230) |
| snapchat-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| archive-org | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| godaddy-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tumblr-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nytimes-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| shopify-com | a11y | a11y: 18/104 (Lumen 469) |
| paypal-com | a11y | a11y: 0/42 (Lumen 290) |
| discord-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| t-online-de | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| flickr-com | exceptions, pixels, a11y | exceptions: referenceerror: webfont is not defined; script load failed: url error: http N;; pixels: diff 98.18% (порог 35.0%), маска 58.8%; a11y: 60/66 (Lumen 363) |
| soundcloud-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| theguardian-com | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 171/273 (Lumen 628) |
| medium-com | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| cnn-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ebay-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| forbes-com | exceptions, pixels, a11y | exceptions: typeerror: fn is not a function; pixels: diff 98.54% (порог 35.0%), маска 55.5%; a11y: 399/461 (Lumen 1269) |
| bbc-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sciencedirect-com | exceptions, pixels, a11y | exceptions: syntaxerror: identifier 'pendodata' has already been declared; typeerror: b.ge; pixels: diff 53.04% (порог 35.0%), маска 45.9%; a11y: 110/131 (Lumen 361) |
| mit-edu | a11y | a11y: 49/56 (Lumen 309) |
| twitch-tv | a11y | a11y: 12/150 (Lumen 388) |
| researchgate-net | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| stripe-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| sourceforge-net | pixels, a11y | pixels: diff 35.95% (порог 35.0%), маска 47.2%; a11y: 137/150 (Lumen 386) |
| namecheap-com | status, walls, a11y | status: chrome=200 lumen=301; walls: net-error; a11y: 0/179 (Lumen 237) |
| hubspot-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| roku-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| linktr-ee | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| reuters-com | status, walls, pixels, a11y | status: chrome=200 lumen=401; walls: hard:datadome-host; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/199 (Lumen 248) |
| weather-com | status, walls, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; a11y: 0/87 (Lumen 252) |
| hp-com | pixels, a11y | pixels: diff 99.33% (порог 35.0%), маска 74.6%; a11y: 0/49 (Lumen 261) |
| springer-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 92.92% (порог 35.0%), маска 71.1%; a11y: 0/5 (Lumen 264) |
| www-gov-uk | pixels | pixels: diff 37.19% (порог 35.0%), маска 35.6% |
| autodesk-com | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: blank:near-empty, hard:akamai-reference; pixels: diff 99.64% (порог 35.0%), маска 88.1%; a11y: 4/21 (Lumen 265) |
| nature-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 95.58% (порог 35.0%), маска 68.0%; a11y: 0/5 (Lumen 273) |
| steampowered-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| salesforce-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| alibaba-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mi-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| wiley-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| etsy-com | walls, pixels, a11y | walls: hard:datadome-host; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/62 (Lumen 232) |
| bilibili-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| dell-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| naver-com | exceptions, a11y | exceptions: cannot read properties of undefined (reading 'postmessage'); typeerror: cannot; a11y: 154/204 (Lumen 456) |
| slack-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ea-com | walls, exceptions, pixels, a11y | walls: blank:near-empty; exceptions: error [statsig] a networking error occurred during post request to url network; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 2/83 (Lumen 237) |
| mobile-de | status, walls, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; a11y: 0/7 (Lumen 240) |
| nasa-gov | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| brave-com | a11y | a11y: 126/137 (Lumen 466) |
| tp-link-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| cdc-gov | exceptions, pixels, a11y | exceptions: syntaxerror: [data-bs-toggle="collapse" is not a valid selector; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 25/47 (Lumen 533) |
| washingtonpost-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| gitlab-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tradingview-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nvidia-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| bloomberg-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| wsj-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| globo-com | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/448 (Lumen 244) |
| bsky-app | status, pixels, a11y | status: chrome=200 lumen=None; pixels: нет кадра; a11y: нет a11y-дерева |
| calendly-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| unsplash-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| go-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 45.96% (порог 35.0%), маска 0.3%; a11y: 0/89 (Lumen 233) |
| indeed-com | status, a11y | status: chrome=200 lumen=403; a11y: 0/31 (Lumen 232) |
| duckduckgo-com | a11y | a11y: 21/133 (Lumen 369) |
| line-me | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| claude-ai | status, walls, a11y | status: chrome=200 lumen=None; walls: net-error; a11y: 1/107 (Lumen 249) |
| steamcommunity-com | exceptions, a11y | exceptions: typeerror: cannot read properties of undefined (reading 'apply'); a11y: 49/158 (Lumen 521) |
| temu-com | status, walls, pixels, a11y | status: chrome=200 lumen=400; walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/6 (Lumen 252) |
| dailymotion-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| supercell-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| huawei-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| substack-com | walls, a11y | walls: net-error; a11y: 0/73 (Lumen 245) |
| taobao-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| duolingo-com | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 43/89 (Lumen 348) |
| fandom-com | status, walls, pixels, a11y | status: chrome=200 lumen=404; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| aol-com | status, walls, pixels, a11y | status: chrome=200 lumen=404; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| businessinsider-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| pixabay-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| grammarly-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| speedtest-net | status, walls, pixels, a11y | status: chrome=200 lumen=404; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| aliexpress-com | walls, pixels, a11y | walls: blank:near-empty; pixels: diff 68.7% (порог 35.0%), маска 38.5%; a11y: 0/206 (Lumen 261) |
| cnbc-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| walmart-com | walls, a11y | walls: blank:near-empty; a11y: 21/39 (Lumen 284) |
| goodreads-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| quora-com | status, walls, pixels, a11y | status: chrome=200 lumen=308; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| shein-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 37.37% (порог 35.0%), маска 74.6%; a11y: 1/97 (Lumen 245) |
| npr-org | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 80.17% (порог 35.0%), маска 27.2%; a11y: 4/289 (Lumen 249) |
| noaa-gov | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| scribd-com | status, walls, exceptions, a11y | status: chrome=200 lumen=451; walls: blank:near-empty; exceptions: link hint fetch failed: url; a11y: 1/86 (Lumen 255) |
| indiatimes-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| behance-net | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| foxnews-com | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| eventbrite-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| slideshare-net | status, walls, a11y | status: chrome=200 lumen=451; walls: blank:near-empty; a11y: 1/111 (Lumen 234) |
| uol-com-br | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| capcut-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ft-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/257 (Lumen 249) |
| fast-com | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| free-fr | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tandfonline-com | status, exceptions, a11y | status: chrome=200 lumen=403; exceptions: referenceerror: runprogram is not defined; typeerror; unhandled error: , with ; a11y: 4/119 (Lumen 233) |
| rakuten-co-jp | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| deviantart-com | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tripadvisor-com | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: hard:datadome-host; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/127 (Lumen 244) |
| wps-com | exceptions, a11y | exceptions: @firebase/analytics: transactioninactiveerror: transaction is not active; a11y: 5/48 (Lumen 304) |
| threads-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| playstation-com | exceptions, pixels, a11y | exceptions: syntaxerror: [*/href="#a"] is not a valid selector; pixels: diff 98.97% (порог 35.0%), маска 56.0%; a11y: 0/12 (Lumen 389) |
| usatoday-com | status, walls, a11y | status: chrome=200 lumen=403; walls: blank:near-empty; a11y: 1/168 (Lumen 262) |
| target-com | status, walls, pixels | status: chrome=200 lumen=429; walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0% |
| ovh-net | a11y | a11y: 11/18 (Lumen 320) |
| atlassian-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mdpi-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| wix-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| accuweather-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ikea-com | status, pixels, a11y | status: chrome=200 lumen=403; pixels: diff 38.69% (порог 35.0%), маска 42.9%; a11y: 0/178 (Lumen 234) |
| nintendo-com | exceptions | exceptions: link hint fetch failed: url |
| ups-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| merriam-webster-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| jotform-com | a11y | a11y: 94/103 (Lumen 562) |
| dailymail-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nike-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| elpais-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| britannica-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| perplexity-ai | status, walls, exceptions, a11y | status: chrome=200 lumen=403; walls: hard:cloudflare-chl; exceptions: typeerror; typeerror: d[ai(...)] is not a function; a11y: 0/29 (Lumen 240) |
| surveymonkey-com | status, pixels, a11y | status: chrome=200 lumen=403; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/96 (Lumen 244) |
| cambridge-org | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| lemonde-fr | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| kick-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| cbsnews-com | status, walls, pixels, a11y | status: chrome=200 lumen=406; walls: blank:near-empty; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/106 (Lumen 232) |
| yelp-com | status, pixels, a11y | status: chrome=403 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| viber-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| airbnb-com | exceptions, pixels, a11y | exceptions: error: battery status api is not supported; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/13 (Lumen 265) |
| shopee-com-br | status, pixels, a11y | status: chrome=200 lumen=403; pixels: diff 69.51% (порог 35.0%), маска 58.0%; a11y: 0/3 (Lumen 248) |
| ring-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| marriott-com | status, exceptions | status: chrome=403 lumen=200; exceptions: a client-side exception has occurred, see here for more info: url; error rende |
| uber-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 88.7% (порог 35.0%), маска 15.9%; a11y: 2/70 (Lumen 264) |
| itch-io | status, walls, a11y | status: chrome=200 lumen=None; walls: net-error; a11y: 1/302 (Lumen 267) |
| box-com | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of null (reading 'readystate'); typeerror: c; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 117/147 (Lumen 421) |
| independent-co-uk | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| asus-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| allegro-pl | status, walls, pixels, a11y | status: chrome=200 lumen=403; walls: hard:datadome-host; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 1/82 (Lumen 274) |
| mediafire-com | pixels, a11y | pixels: diff 72.43% (порог 35.0%), маска 4.7%; a11y: 31/38 (Lumen 277) |
| imgur-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| bild-de | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/648 (Lumen 245) |
| mlb-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nikkei-com | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| seznam-cz | status, walls, a11y | status: chrome=200 lumen=301; walls: net-error; a11y: 15/543 (Lumen 233) |
| zillow-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| welt-de | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| wattpad-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| apnews-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: net-error; pixels: diff 80.16% (порог 35.0%), маска 43.3%; a11y: 1/369 (Lumen 257) |
| lenovo-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ameblo-jp | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| fiverr-com | status, walls, a11y | status: chrome=200 lumen=301; walls: net-error; a11y: 3/140 (Lumen 233) |
| wp-pl | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| disneyplus-com | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 5/57 (Lumen 252) |
| att-com | status, walls, a11y | status: chrome=200 lumen=403; walls: hard:akamai-reference; a11y: 0/28 (Lumen 240) |
| zoho-com | pixels, a11y | pixels: diff 43.76% (порог 35.0%), маска 64.0%; a11y: 75/81 (Lumen 680) |
| usps-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 59.61% (порог 35.0%), маска 16.1%; a11y: 0/102 (Lumen 253) |
| deepl-com | status, walls, a11y | status: chrome=200 lumen=451; walls: blank:near-empty; a11y: 1/182 (Lumen 252) |
| discogs-com | status, walls, exceptions, pixels, a11y | status: chrome=200 lumen=403; walls: hard:cloudflare-chl; exceptions: typeerror; typeerror: k[aj(...)] is not a function; pixels: diff 86.33% (порог 35.0%), маска 44.8%; a11y: 1/219 (Lumen 256) |
| lefigaro-fr | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 3/579 (Lumen 264) |
| academia-edu | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| agoda-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ryanair-com | exceptions, pixels, a11y | exceptions: typeerror: cannot read properties of null (reading 'innerhtml'); pixels: diff 38.99% (порог 35.0%), маска 38.3%; a11y: 81/99 (Lumen 388) |
| pexels-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| curseforge-com | status, walls, exceptions, pixels, a11y | status: chrome=200 lumen=403; walls: hard:cloudflare-chl; exceptions: typeerror; typeerror: i[ai(...)] is not a function; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/122 (Lumen 274) |
| chess-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| biblegateway-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 38.64% (порог 35.0%), маска 0.0%; a11y: 0/33 (Lumen 233) |
| figma-com | a11y | a11y: 90/95 (Lumen 385) |
| nbcnews-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 99.86% (порог 35.0%), маска 90.1%; a11y: 1/444 (Lumen 241) |
| focus-de | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| daum-net | status, walls, pixels, a11y | status: chrome=200 lumen=307; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| coupang-com | status, pixels, a11y | status: chrome=403 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| mayoclinic-org | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| latimes-com | status, walls, a11y | status: chrome=200 lumen=301; walls: net-error; a11y: 11/489 (Lumen 261) |
| lg-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| bankofamerica-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| kleinanzeigen-de | status, pixels, a11y | status: chrome=200 lumen=403; pixels: diff 59.42% (порог 35.0%), маска 60.4%; a11y: 0/14 (Lumen 265) |
| classlink-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| mercadolivre-com-br | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 47.38% (порог 35.0%), маска 72.2%; a11y: 0/158 (Lumen 276) |
| mercadolibre-com-ar | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 47.78% (порог 35.0%), маска 72.0%; a11y: 0/166 (Lumen 279) |
| patreon-com | status, pixels, a11y | status: chrome=200 lumen=403; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 2/71 (Lumen 282) |
| genius-com | exceptions, pixels, a11y | exceptions: referenceerror: runprogram is not defined; typeerror; unhandled error: , with ; pixels: diff 98.51% (порог 35.0%), маска 0.3%; a11y: 0/1 (Lumen 229) |
| state-gov | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| note-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| poki-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| character-ai | pixels, a11y | pixels: нет кадра; a11y: нет a11y-дерева |
| flipkart-com | walls, pixels, a11y | walls: net-error; pixels: diff 100.0% (порог 35.0%), маска 61.4%; a11y: 2/124 (Lumen 241) |
| repubblica-it | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| otto-de | status, a11y | status: chrome=200 lumen=403; a11y: 1/250 (Lumen 246) |
| fidelity-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| hm-com | status, pixels | status: chrome=403 lumen=503; pixels: diff 75.98% (порог 35.0%), маска 0.1% |
| trendyol-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 96.36% (порог 35.0%), маска 66.7%; a11y: 0/120 (Lumen 261) |
| corriere-it | pixels, a11y | pixels: diff 74.92% (порог 35.0%), маска 15.3%; a11y: 494/924 (Lumen 1486) |
| xiaohongshu-com | status, pixels, a11y | status: chrome=200 lumen=307; pixels: нет кадра; a11y: нет a11y-дерева |
| lowes-com | walls, a11y | walls: soft:akamai-reference, soft:blocked-generic-en; a11y: 2/104 (Lumen 228) |
| onet-pl | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/282 (Lumen 237) |
| change-org | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| caixa-gov-br | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| nexusmods-com | exceptions, a11y | exceptions: script load failed: url error: http N; a11y: 97/121 (Lumen 382) |
| ancestry-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: net-error; pixels: diff 98.14% (порог 35.0%), маска 75.6%; a11y: 0/36 (Lumen 245) |
| sofascore-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| kwai-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| costco-com | status, walls, pixels, a11y | status: chrome=200 lumen=404; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| flightradar24-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| flashscore-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| webmd-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| clever-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| hilton-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| bol-com | status, walls, a11y | status: chrome=403 lumen=200; walls: blank:near-empty; a11y: 0/3 (Lumen 234) |
| nexon-com | exceptions, a11y | exceptions: script load failed: url error: http N; a11y: 3/22 (Lumen 288) |
| elmundo-es | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ecosia-org | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| healthline-com | status, pixels, a11y | status: chrome=200 lumen=403; pixels: diff 92.34% (порог 35.0%), маска 24.0%; a11y: 0/95 (Lumen 236) |
| docomo-ne-jp | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: net-error; pixels: diff 65.77% (порог 35.0%), маска 93.1%; a11y: 2/87 (Lumen 245) |
| kohls-com | status, walls, a11y | status: chrome=200 lumen=403; walls: blank:near-empty, hard:akamai-reference; a11y: 2/254 (Lumen 244) |
| letterboxd-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| theatlantic-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/184 (Lumen 233) |
| olx-pl | status, pixels, a11y | status: chrome=200 lumen=403; pixels: diff 87.07% (порог 35.0%), маска 13.9%; a11y: 0/173 (Lumen 232) |
| huffpost-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| garmin-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| wise-com | status, pixels, a11y | status: chrome=200 lumen=403; pixels: diff 94.81% (порог 35.0%), маска 33.9%; a11y: 0/210 (Lumen 246) |
| infobae-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| n-tv-de | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| moneycontrol-com | status, walls, a11y | status: chrome=200 lumen=301; walls: net-error; a11y: 2/992 (Lumen 261) |
| ameba-jp | status, walls, a11y | status: chrome=200 lumen=302; walls: net-error; a11y: 0/127 (Lumen 264) |
| starlink-com | pixels, a11y | pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 17/35 (Lumen 258) |
| pbs-org | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| ouest-france-fr | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 75.16% (порог 35.0%), маска 15.9%; a11y: 1/532 (Lumen 241) |
| skroutz-gr | status, pixels, a11y | status: chrome=200 lumen=403; pixels: diff 46.69% (порог 35.0%), маска 14.4%; a11y: 0/581 (Lumen 242) |
| instructure-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| goal-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 15/270 (Lumen 253) |
| terra-com-br | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| as-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| magnific-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| bfmtv-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: net-error; pixels: diff 91.37% (порог 35.0%), маска 47.8%; a11y: 0/551 (Lumen 237) |
| crazygames-com | status, pixels, a11y | status: chrome=200 lumen=301; pixels: нет кадра; a11y: нет a11y-дерева |
| ilovepdf-com | exceptions, a11y | exceptions: syntaxerror: [object object] is not a valid selector; a11y: 71/80 (Lumen 426) |
| douyin-com | exceptions, pixels, a11y | exceptions: typeerror: right-hand side of 'instanceof' is not an object; pixels: diff 100.0% (порог 35.0%), маска 70.1%; a11y: 0/52 (Lumen 244) |
| marca-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| tabelog-com | status, walls, pixels, a11y | status: chrome=200 lumen=None; walls: net-error; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 0/542 (Lumen 257) |
| huggingface-co | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| interia-pl | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| aparat-com | a11y | a11y: 8/172 (Lumen 262) |
| wordwall-net | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| gofundme-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| finn-no | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| gsmarena-com | walls, a11y | walls: blank:near-empty; a11y: 0/196 (Lumen 233) |
| giphy-com | a11y | a11y: 24/41 (Lumen 274) |
| cookpad-com | status, pixels, a11y | status: chrome=200 lumen=404; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 4/131 (Lumen 242) |
| t-mobile-com | status, walls, a11y | status: chrome=200 lumen=403; walls: blank:near-empty, hard:akamai-reference; a11y: 0/174 (Lumen 244) |
| nypost-com | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| aftonbladet-se | walls, pixels, a11y | walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| hltv-org | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| hotpepper-jp | status, walls, pixels, a11y | status: chrome=200 lumen=302; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| vivo-com | exceptions, pixels, a11y | exceptions: script load failed: url typeerror: fetch: network error for url; pixels: diff None% (порог 35.0%), маска 100.0%; a11y: 10/56 (Lumen 354) |
| kickstarter-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| skysports-com | pixels | pixels: diff None% (порог 35.0%), маска 100.0% |
| riotgames-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| lanacion-com-ar | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| one-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |
| clarin-com | status, walls, pixels, a11y | status: chrome=200 lumen=301; walls: blank:near-empty; pixels: нет кадра; a11y: нет a11y-дерева |

## Эталон не открыл сайт (вне CPR)

- imgsrc-ru: net::ERR_CONNECTION_RESET
- litmarket-ru: net::ERR_CONNECTION_RESET
- lenreg-ru: net::ERR_CONNECTION_RESET
- cikrf-ru: net::ERR_CONNECTION_RESET
- leroymerlin-ru: net::ERR_CONNECTION_RESET
- konami-net: net::ERR_CONNECTION_CLOSED

## Флапы

- mail-ru: walls, exceptions
- nic-ru: status
- reg-ru: status, exceptions, pixels
