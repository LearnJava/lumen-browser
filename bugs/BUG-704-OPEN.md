# BUG-704 — `Animation.prototype.commitStyles`/`.persist` missing entirely

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


## Замер 2026-08-23 (WPT-RUN-6, срез 25): отсутствует не только `persist`/`commitStyles`, но и вся механика замены анимаций

`tests/wpt/verify_focus_mutation_animation_gaps.py --variant wa-persist`
(dev-release, Linux, `main` = `530d0a444`, `--seconds 5`, страница жива):

```
wp-api persist=undefined commitStyles=undefined replaceState=undefined
       getAnimations=function
wp-second-created count=2
wp-after count=2 a.replaceState=undefined opacity=0.25
wp-persist-throws TypeError: a.persist is not a function
wp-commit-throws  TypeError: b.commitStyles is not a function
```

Новое по сравнению с исходной формулировкой бага:

* `Animation.replaceState` не существует как свойство (не только методы);
* **автоматическая замена не происходит вовсе** — вторая `fill: forwards`
  анимация того же свойства не переводит первую в `removed`,
  `getAnimations()` продолжает возвращать обе, `onremove` не приходит;
* при этом сам эффект считается верно (`opacity=0.25` — выигрывает вторая).

То есть закрыть баг добавлением двух методов не выйдет: нужна процедура
«remove replaced animations» (Web Animations §5.4) целиком. Даёт 3 id
остатка снимка WPT-RUN-5: `Animation/persist.html`, `Animation/onremove.html`,
`keyframe-effects/effect-value-replaced-animations.html`. Соседние дыры того
же объекта — [BUG-860](BUG-860-DUPLICATE.md) (не `EventTarget`) и
[BUG-861](BUG-861-OPEN.md) (перемотка завершённой анимации).
