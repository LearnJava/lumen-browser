## Бисект от lumen-2026-09-23 к chrome-153-plain (plain_tls)

Дата: 2026-10-13. Повторов на ячейку: 2.

| Сайт | как есть | без Cache-Control | User-Agent → B | Accept-Encoding → B | без DNT | + sec-ch-ua | + sec-ch-ua-mobile | + sec-ch-ua-platform | + Upgrade-Insecure-Requests | + Sec-Fetch-User | весь набор B |
|---|---|---|---|---|---|---|---|---|---|---|---|
| zillow | 403 perimeterx / 403 perimeterx | 403 perimeterx / 403 perimeterx | 200 / 200 | 403 perimeterx / 403 perimeterx | 403 perimeterx / 403 perimeterx | 403 perimeterx / 403 perimeterx | 403 perimeterx / 403 perimeterx | 403 perimeterx / 403 perimeterx | 403 perimeterx / 403 perimeterx | 403 perimeterx / 403 perimeterx | 200 / 200 |
| reuters | 401 datadome / 401 datadome | 401 datadome / 401 datadome | 401 datadome / 401 datadome | 401 datadome / 401 datadome | 401 datadome / 401 datadome | 401 datadome / 401 datadome | 401 datadome / 401 datadome | 401 datadome / 401 datadome | 401 datadome / 401 datadome | 401 datadome / 401 datadome | 401 datadome / 401 datadome |
| accuweather | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 |
| adobe | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 |
| washingtonpost | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / 200 | 200 / TIMEOUT | 200 / 200 | 200 / 200 | 200 / 200 |
| khanacademy | 200 js-challenge / 200 js-challenge | 200 js-challenge / 200 js-challenge | 200 js-challenge / 200 js-challenge | 200 js-challenge / 200 js-challenge | 200 js-challenge / 200 js-challenge | 200 js-challenge / 200 js-challenge | 200 js-challenge / 200 js-challenge | 200 js-challenge / 200 js-challenge | 200 js-challenge / 200 js-challenge | 200 js-challenge / 200 js-challenge | 200 js-challenge / 200 js-challenge |
