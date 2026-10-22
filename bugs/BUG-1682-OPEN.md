# BUG-1682 — рантайм V8 дочернего фрейма не освобождается никогда: ~20 МБ на каждый iframe, ни удаление `<iframe>`, ни замена страницы его не уносят

**Статус:** OPEN
**Заведён:** 2026-10-22 (P2, блокер WPT-RUN-16)
**Область:** shell/js (`crates/shell/src/frames.rs` — `FrameHandle`, `spawn_frame`, `drop_frame_subtree`; `crates/shell/src/frame_dynamic_load.rs::scan_dynamic_frames`; `crates/js/src/frame_bridge.rs::upsert_binding`; `crates/shell/src/frame_ancestry.rs` — `register_parent_document(…, parent_peer)`; `impl Drop for V8JsRuntime` в `crates/js/src/v8_runtime/runtime.rs:2175`)

## Симптом

Каждый `<iframe>` получает свой `V8JsRuntime`: отдельный поток и изолят, около 20 МБ приватной памяти даже для пустого документа. Этот рантайм не освобождается ни при удалении `<iframe>` из DOM, ни при замене страницы. Память браузера растёт линейно с числом фреймов, которые страница когда-либо создавала, и падает только при перезапуске процесса.

Под раннером WPT браузер переиспользуется от теста к тесту. В шардах `referrer-policy` и `encoding` (каждый тест создаёт несколько iframe) один браузер доходит до 2.5 ГБ за ~10 тестов. Полный прогон WPT-RUN-16 (3 полосы × 14 браузеров) упёрся в своп: свободная память ушла в 0, система вставала на 3–5 мин, тесты получали ложные TIMEOUT и CRASH. Без починки полная цифра испорчена, поэтому баг стоит в начале очереди P3.

## Проба

Приватная память процесса, замер раз в 3 с (`psutil`, `memory_info().private`), `lumen.exe --no-paint <url>`, бинарь `dev-release` на `1e87683d6`.

`ifr-empty.html` — каждые 200 мс создаёт iframe и удаляет его через 150 мс:

```html
<!doctype html><script>
var n = 0;
setInterval(function () {
  var f = document.createElement('iframe');
  f.src = 'empty-child.html?' + (n++);      // empty-child.html: <!doctype html><p>hi
  document.body.appendChild(f);
  setTimeout(function () { f.remove(); }, 150);
}, 200);
</script>
```

`navframes.html` — три статических iframe, после `load` вызывает `location.replace` на самого себя с новым `?n#n`:

```html
<!doctype html>
<iframe src="empty-child.html"></iframe><iframe src="empty-child.html"></iframe><iframe src="empty-child.html"></iframe>
<script>
var n = +(location.hash.slice(1) || 0);
onload = function () { setTimeout(function () { location.replace(location.pathname + '?' + (n + 1) + '#' + (n + 1)); }, 100); };
</script>
```

| страница | МБ через 3 / 15 / 30 / 45 с |
|---|---|
| `ifr-empty.html` (удаление iframe) | 339 / 1 693 / 3 413 / — |
| `ifr.html` (то же, ребёнок с 2 000 `<p>` и массивом на 200 000 элементов) | 445 / 2 130 / 4 321 / 6 055 |
| `navframes.html` (замена страницы с 3 фреймами) | 280 / 1 651 / 3 322 / 4 943 |
| `nav.html` (замена страницы **без** фреймов) | 367 / 491 / 504 / 505, стабильно |

Без фреймов навигация не течёт, а с пустыми фреймами течёт. Значит, утекает именно рантайм ребёнка, а не его DOM.

Утечка давняя, это не регрессия: так же ведут себя `dev-release` на `ea2ba3351` (2026-10-20) и `release` из слота p4 (2026-10-20 14:40). Цикл parent ↔ child появился в BUG-480 срез 3 (`d0bf953b6`), ещё до WPT-RUN-9. Почему WPT-RUN-9 (`d913a39b9`, тот же рецепт 3 × 14) пережил память на грани (свободно 0.1–0.8 ГБ, `docs/wpt/pass-rate.md` §«Третья полная цифра»), а WPT-RUN-16 — нет, не выяснялось.

## Механизм (по коду)

1. **Пути удаления нет.** `scan_dynamic_frames` (`frame_dynamic_load.rs:59-77`) сообщает только о новых хостах и о сменённом `src`, исчезнувший хост он не замечает. Хэндлы покидают список `frames` только в `apply_frame_navigation`/`drop_frame_subtree` (`frames.rs:3058-3070`) и при замене страницы. Документация `FrameHandle` (`frames.rs:3091-3097`, `:2778-2780`) прямо говорит, что удаление отдельного фрейма — «будущий срез».
2. **Реестр родителя только растёт.** `register_frame_document` → `upsert_binding` (`frame_bridge.rs:307-321`) заменяет или дописывает запись, но никогда не удаляет. Новый iframe — новый `host_nid`, поэтому `FrameDocSlots.frames` растёт без предела. Каждая запись держит `Document` ребёнка и `peer` — клон его `Arc<V8JsRuntime>` (`persistent_js.rs:1232-1233`, `frames.rs:2804-2812`).
3. **Цикл `Arc`.** Same-origin ребёнок хранит `parent_peer` — `Arc<V8JsRuntime>` родителя (`frame_ancestry.rs:68-75`, `frames.rs:2491`), а родитель хранит `peer` ребёнка. Ни один `Drop for V8JsRuntime` (а только он шлёт `V8Command::Shutdown` и join-ит поток) не срабатывает, даже когда страница заменена: так объясняется строка `navframes.html`.

## Что делать

- Разорвать цикл: `parent_peer`/`top`-пиры ребёнка — `Weak<V8JsRuntime>` (или явная очистка привязок при уходе страницы), чтобы замена страницы уносила все рантаймы фреймов.
- Удаление отдельного фрейма: `scan_dynamic_frames` замечает отсоединённый хост → хэндл уходит из `frames`, привязка уходит из `FrameDocSlots.frames` родителя (нужен парный к `upsert_binding` `remove_binding`), у ребёнка чистятся `parent`/`top`. `contentWindow` удалённого iframe по спеке становится `null`, это поведение проверить.

## Как проверить

Таблица выше: все три страницы с фреймами держатся на уровне строки `nav.html`, то есть не растут после первых секунд. Тест в `crates/shell`: после `drop` страницы с фреймом и после удаления хоста число живых `V8JsRuntime` (счётчик в `Drop` или `Weak::strong_count`) возвращается к исходному. Затем WPT-RUN-16: шард `referrer-policy` под `run_smoke.py` не поднимает браузер выше нескольких сотен МБ.
