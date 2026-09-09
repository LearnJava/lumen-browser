# E2E-4 — стенд гидрации React 18 / Next.js

Локальный стенд для дорожки **E2E** (`ROADMAP.md`, задача E2E-4). Отвечает на один
вопрос: доходит ли гидрация React 18 до конца в Lumen и на чём именно она встаёт.
Внешний стенд (Keycloak + Next.js 14) для этого поднимать не нужно — здесь
воспроизводится ровно та форма, на которой он падал.

Две ступени, отличаются **корнем гидрации**:

| Файл | Корень | Что проверяет |
|---|---|---|
| `index.html` + `client.js` | `hydrateRoot(<div id=root>, …)` | обычное React-приложение в контейнере |
| `doc.html` + `client2.js` | `hydrateRoot(document, …)` | форма Next.js 14 App Router |

Разметку SSR генерируют `gen.js` / `gen2.js` настоящим `react-dom/server`, поэтому в
ней есть маркеры Suspense `<!--$-->` / `<!--/$-->` — те самые, на которых стояла
живая проба. Клиентское дерево обязано совпадать с серверным байт в байт, включая
`<script>`-теги: у `doc.html` они часть дерева React, как их отдаёт Next.js.

`docapi.html` — отдельная минимальная страница: печатает, какие члены `Node` есть у
глобального `document`, а каких нет.

## Чего здесь нет

Бандлы `react.js` / `react-dom.js` (UMD, 18.3.1) **не закоммичены** — это чужой код,
и вендорить его ради стенда не нужно. Их кладёт рядом `gen.js`, копируя из
`node_modules` любого проекта с React 18:

```bash
cd <проект с react 18>/           # напр. фронтенд внешнего стенда
NODE_PATH="$PWD/node_modules" node <lumen>/samples/e2e4-hydration/gen.js <lumen>/samples/e2e4-hydration
NODE_PATH="$PWD/node_modules" node <lumen>/samples/e2e4-hydration/gen2.js <lumen>/samples/e2e4-hydration
```

`gen.js` пишет `ssr-body.html` (тело для `index.html`) и копирует оба бандла;
`gen2.js` пишет `doc.html` целиком.

## Как гонять

Гидрация React 18 планируется через Scheduler, а headless-дамп видит только
синхронный скрипт ([`docs/engine-gaps.md`](../../docs/engine-gaps.md)) — значит нужно
живое окно и http одного origin:

```bash
python -m http.server 8762 --bind 127.0.0.1 --directory samples/e2e4-hydration &
python samples/e2e4-hydration/drive.py http://127.0.0.1:8762/index.html 15
python samples/e2e4-hydration/drive.py http://127.0.0.1:8762/doc.html 15
```

`drive.py` поднимает `target/dev-release/lumen.exe --mcp-live-port`, ждёт
`document_ready`, вычитывает журнал страницы (`window.__PROBE`), затем кликает по
кнопке и печатает `resource://console` и отфильтрованный stderr.

`?lookahead=1` подставляет `document.removeChild`/`insertBefore`/`replaceChild`
заглушками, `?lookahead=2` добавляет геттеры `firstChild`/`lastChild`. Семантика
заглушек **заведомо неверная** — они нужны только чтобы увидеть, что сломается
следующим за [BUG-557](../../bugs/BUG-557-OPEN.md), и их вывод нельзя читать как
измерение самой гидрации.

## Замер 2026-09-09 (P6, E2E-4)

* Корень-контейнер — гидрация проходит целиком: фиберы на месте
  (`__reactFiber$…` на кнопке), маркеры Suspense съедены штатно.
* Корень `document` — страница **виснет**: 130 тыс. повторов
  `TypeError: t.removeChild is not a function`, `wait{document_ready}` истекает
  за 30 с (воспроизведено трижды). Причина — [BUG-557](../../bugs/BUG-557-OPEN.md).
* Интерактивность упирается в [BUG-926](../../bugs/BUG-926-OPEN.md) (нулевая ширина
  `<button>`) и [BUG-1044](../../bugs/BUG-1044-OPEN.md) (MCP-клик попадает в родителя).
