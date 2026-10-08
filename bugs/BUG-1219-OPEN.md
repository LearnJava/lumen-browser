# BUG-1219 — `/creditors` и `/counteragents`: «Application error: a client-side exception» (`Cannot read properties of undefined (reading 'set')`)

**Статус:** OPEN
**Компонент:** js (не локализовано — какой веб-API отдаёт `undefined` под `.set`)
**Найден:** 2026-09-29, стенд bankruptcy-platform (`test_au` / `password123`), сборка `main` 22a782d55

## Симптом

Обе страницы после гидратации показывают белый экран с «Application error: a client-side exception has occurred».
DOM — 41 элемент, `document.title` пуст (у рабочих страниц 30 символов, 100–216 элементов). Воспроизводится и при
полной загрузке, и при клиентской навигации.

Перехват через `console.error` при `window.next.router.push('/creditors')`:

```
TypeError: Cannot read properties of undefined (reading 'set')
    at eval (eval at _lumen_script_execute_classic …, <anonymous>:1:18981)
    at aW (<anonymous>:1:73244)  at oe (<anonymous>:1:84685)  at ol (<anonymous>:1:85323) …
```

Стек внутри React-рендера (`aW`/`oe`/`ol`), значит исключение из компонента страницы в чанке маршрута (лениво грузится
`app/creditors/page-*.js`). Страницы `/cases`, `/correspondence`, `/profile`, `/about` работают.

## Что делать

Сохранить чанки `app/creditors/page-*.js` и `app/counteragents/page-*.js` (`fetch` из страницы), найти по смещению
`:1:18981` выражение `X.set(...)`, определить, какой API возвращает `undefined` (кандидаты: `Map`/`WeakMap`-подобные
хранилища, `URLSearchParams`, `CSS.*`, `Intl.*`), и свести к минимальному repro.
