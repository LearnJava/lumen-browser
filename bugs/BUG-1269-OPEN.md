# BUG-1269 — `form.submit()` из скрипта игнорирует `target`: навигирует верхний документ вместо именованного `<iframe>`

**Статус:** OPEN
**Заведён:** 2026-10-04 (P2, найден при разборе стены WPT-прогона — `docs/tasks/p2-wpt-runner-throughput.md` §уход со страницы теста)
**Область:** shell (`crates/shell/src/lumen/form_submit.rs::run_form_submission` — путь `NavigateRequest::SubmitForm` от `form.submit()`/`requestSubmit()` страницы)

## Симптом

Страница создаёт `<iframe name="frame-0">` и `<form target="frame-0" method=GET
action="/common/blank.html">`, кладёт значение в поле и зовёт `form.submit()`.
По HTML LS §4.10.21.3 (шаг «target navigable» — §7.3.1.7 «rules for choosing a
navigable» по имени `frame-0`) навигироваться должен фрейм. Lumen навигирует
**верхний документ**: в stdout браузера сразу после `form.submit()` —

```
⊢ form get /common/blank.html body=input-1=%E3%82%AE%2C…
Reload: http://localhost:18300/common/blank.html?input-1=%E3%82%AE%2C…
```

Страница теста исчезает вместе с testharness, `iframe.onload` не наступает
никогда, тест висит до таймаута.

`run_form_submission` не читает `target` вовсе (`grep target
crates/shell/src/lumen/form_submit.rs` — одно совпадение, в комментарии про
CSP). Путь отправки формы *изнутри фрейма*
(`crates/shell/src/lumen/frame_form_submit.rs:53`) `target` читает — у страницы
этого нет.

## Как воспроизвести

```
<venv>/python tests/wpt/run_smoke.py --binary target/dev-release/lumen.exe \
    '/encoding/legacy-mb-japanese/euc-jp/eucjp-encode-form-x-euc-jp.html?4001-5000'
```

В логе — `Reload: …/common/blank.html?input-1=…` через ~0.5 с после старта
теста; итог TIMEOUT без единого сабтеста (с 2026-10-04 исполнитель кончает такой
тест через 15 с, а не через 65 — см. ниже; до этого — `timeout: long`, 60 + 5 с).

## Почему это важно

- **Тесты.** Все 45 файлов `encoding/**/*-encode-form*.html` (общий
  `encoding/resources/encode-form-common.js`) — **583 id** с вариантами — так
  написаны, и все кончаются TIMEOUT с 0 сабтестов. Те же `target`-формы
  встречаются в `html/browsers` (15 файлов), `html/semantics` (10),
  `navigation-api/navigate-event` (4) — там не разобрано.
- **Стена прогона.** Именно этот класс задавал хвост корпусного прогона:
  `encoding/legacy-mb-japanese` — 134 TIMEOUT по 60–65 с, 8 500 из 9 941 с работы
  шарда (`capab/p4-cap4`). Исполнитель теперь обрывает «ушедший» тест через
  15 с (`executorlumen.py`, `LUMEN_WPT_FOREIGN_GRACE_S`), так что на скорость
  прогона баг больше не влияет — но вердикты по-прежнему 0.
- **Живые сайты.** Любая форма с `target` на `<iframe>` (платёжные виджеты,
  загрузка файлов через скрытый фрейм) уводит всю вкладку.

## Чего не знаем

- Поддержан ли `target` у формы при *клике* по кнопке на странице (путь
  `handle_click_at` → тот же `run_form_submission`, значит, вероятно, тоже нет).
- `formtarget` у кнопки не учитывается нигде (записано в BUG-480).
