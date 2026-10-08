# BUG-1224 — `preventDefault()` в обработчике `click` не отменяет переход по `<a href>`: каждый клик по ссылке SPA — полная перезагрузка

**Статус:** FIXED 2026-09-29 (P3)
**Исправление:** `click.rs` — результат диспетчеризации `click` читается через `route_query_js`/`eval_js_value` (`false` = `preventDefault`), при отмене нативная активация (ссылка, форма, флажок, details) пропускается; навигацию, поставленную самим обработчиком, по-прежнему забирает `take_navigate_request`.
**Компонент:** shell (`crates/shell/src/lumen/click.rs:578-611` — отправка JS-`click`; ветка `FormClickAction::Nothing` `:809+` — нативный переход по ссылке)
**Найден:** 2026-09-29, разбор видео пользователя и живой стенд bankruptcy-platform (Next.js `next/link`), сборка `main` 22a782d55

## Симптом

Любой SPA-роутер (Next.js `Link`, React Router, Vue Router) перехватывает клик по ссылке через `event.preventDefault()` и
делает переход без перезагрузки. В Lumen `defaultPrevented` игнорируется: после обработчика выполняется и нативный переход,
страница грузится заново — заново скачиваются и исполняются все скрипты, страница мигает без стилей ([BUG-1225](BUG-1225-FIXED.md)).

Доказательства на стенде (вход `test_au`):

- `sessionStorage`-журнал реального клика (MCP `click`, `isTrusted=true`) по `a[href="/cases"]`:
  `capture click tgt=SPAN isTrusted=true | bubble-after-handlers defaultPrevented=true | beforeunload` — обработчик Next
  сработал и отменил событие, но `beforeunload` всё равно пришёл; маркер `window.__m` после клика `undefined`.
- Тот же переход, сделанный `window.next.router.push('/cases')` или синтетическим `a.dispatchEvent(new MouseEvent('click', …))`
  (по `a`, `span`, `svg`), — мягкий: маркер жив (`marker=1`).
- Затронуты 100 % кликов по пунктам сайдбара: 10 из 10 переходов в замере и все переходы в записи пользователя.

## Минимальный repro (без Next)

```html
<!-- prevent.html -->
<a id="l" href="other.html" style="display:block;width:200px;height:40px">link</a>
<div id="out">no click yet</div>
<script>
document.getElementById('l').addEventListener('click', function (e) {
  e.preventDefault();
  document.getElementById('out').textContent = 'handler ran';
});
</script>
```

`other.html` — любая страница с текстом «OTHER PAGE». MCP `click` по `#l` → в окне «OTHER PAGE» (ожидание: остаётся
`prevent.html` с надписью «handler ran»).

## Причина в коде

`click.rs:587-603`: JS-событие уходит через `route_eval_js` — «выстрелил и забыл»; единственное, что читается назад, —
`take_navigate_request` (навигация, поставленная самим обработчиком через `location`). Результат диспетчеризации
(`defaultPrevented`) никак не возвращается, и `:809` (`FormClickAction::Nothing` → `links::find_link` → навигация)
исполняется безусловно. Для форм тот же класс вопроса ([BUG-837](BUG-837-FIXED.md)) закрывали отдельно.

## Что делать

Читать результат диспетчеризации в порядке «read-after-eval» (как `take_navigate_request` рядом) и пропускать нативную
активацию ссылки (и submit/checkbox/details) при `defaultPrevented`. Тест: repro выше + ссылка с `target=_blank`,
`javascript:` href и клик с модификатором (для них `preventDefault` тоже обязан отменять действие).
