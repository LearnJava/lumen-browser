# BUG-702 — `Animation.prototype.commitStyles`/`.persist` missing entirely

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:12954-13070`, Web Animations `WEB_API_SHIM` — `Animation.prototype`)
**Найден:** P2, WPT-VENDOR-web-animations, 2026-08-09

## Симптом

Категория `web-animations` (`tests/wpt/web-animations/`, 236 файлов) — вендорена
и прогнана целиком (`run_report.py --all --root web-animations --recursive`,
~4 мин, 139 отобранных id): 121/139 harness OK, 930/4147 сабтестов. Крупнейший
одиночный драйвер провалов — уже отдельно задокументированный
[BUG-670](BUG-670-OPEN.md) (`getComputedTiming` отсутствует, ~556 вхождений
текста ошибки по трём вариантам вызывающего кода: `effect.`/`anim.effect.`/
`animation.effect.getComputedTiming is not a function`).

Второй по величине, ранее не покрытый ни одним тикетом кластер:

```
54 TypeError: animation.commitStyles is not a function
 4 TypeError: animA.commitStyles is not a function
 8 TypeError: animA.persist is not a function
```

`grep -rn "commitStyles\|\.persist\b" crates/js/src/dom.rs` — ноль совпадений;
оба метода отсутствуют на `Animation.prototype` целиком, а не просто
некорректны.

## Причина

`crates/js/src/dom.rs:12954-13070` определяет на `Animation.prototype` только
`play`/`pause`/`cancel`/`finish`/`reverse`/`updatePlaybackRate` (плюс
внутренние `_scheduleRaf`/`_cancelRaf`/`_tick`/`_applyAtP`/`_clearStyles`/
`_onFinish`). Спека (Web Animations, `Animation` interface) требует ещё два
публичных метода:

- `commitStyles()` — синхронно записывает текущий computed-styling эффекта в
  inline `style` таргета (используется для «заморозки» финального кадра
  анимации без удержания самой анимации живой);
- `persist()` — переводит анимацию из replaceable-состояния в persisted,
  предотвращая её авто-удаление GC браузера после завершения (актуально для
  `animation.replaceState`, тоже нигде не реализованного в шиме —
  `_wa_doc_get_animations`/`getAnimations()` не фильтруют по replace state).

Оба отсутствуют как таковые, не заглушки и не частичные реализации.

## Масштаб

`commitStyles`/`persist` — часть базового Animation-интерфейса WAAPI Level 1,
используются в тестах категории напрямую (не зависят от невендоренных
внешних хелперов) — `animation-model/keyframe-effects/*`,
`interfaces/Animation/{commitStyles,persist}*.html` и смежные. 62 сайта вызова
суммарно провалились этим TypeError в данном прогоне.

## Дальше

Fix scope: добавить `Animation.prototype.commitStyles` (вычислить текущий
composited-стиль эффекта при текущем `currentTime` и записать в
`this.effect.target.style`, бросая `InvalidStateError`/`NoModificationAllowed
Error` по applicability-правилам спеки) и `Animation.prototype.persist`
(флаг `_persisted = true`, читаемый местом, где реализуется auto-removal —
если такого места ещё нет в шиме, `replaceState` тоже придётся завести).
Вне скоупа этой WPT-VENDOR-задачи (только вендоринг + прогон + живая проба).
