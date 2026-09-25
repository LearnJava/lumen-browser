# BUG-642: `new Notification()` never fires `error` when permission is not `"granted"`

**Статус:** FIXED 2026-09-25 (P3)
**Компонент:** js (`crates/js/src/notifications_bindings.rs`, `NOTIFICATIONS_SHIM`)
**Найден:** P2, WPT-VENDOR-notifications, 2026-08-05

## Симптом

`notifications` (скоуп ⬜, кандидат) — вендорена и прогнана целиком
(`run_report.py --all --root notifications --recursive`, ~5 мин, 24
отобранных id из 46 вендоренных файлов, 11 `-manual` исключены раннером):
**2/24 harness OK, 5/7 сабтестов**.

`constructor-non-secure.html` (не-`.https.`, реально исполняется, не
упирается в TLS-гэп) — единственный сабтест TIMEOUT вместо PASS/FAIL:

```js
async_test(function (t) {
  const notification = new Notification("Sup.");
  notification.onerror = t.step_func_done(e => {
    assert_equals(e.type, "error");
  });
}, "new Notification calls onerror in non-secure contexts")
```

`onerror` никогда не вызывается — тест висит до внешнего таймаута
wptrunner.

## Причина

`crates/js/src/notifications_bindings.rs`, конструктор `Notification` в
`NOTIFICATIONS_SHIM`:

```js
// Spec §6: if permission is granted, queue a task to show the notification.
if (_permission === 'granted') {
  var self = this;
  try {
    _lumen_show_notification(this._id, this.title, this.body);
  } catch(e) {}
  queueMicrotask(function() {
    if (!self._closed) {
      self._fire('show');
    }
  });
}
```

Ветка `_permission !== 'granted'` (то есть `'denied'` — дефолтное
значение движка, «privacy-first», см. doc-комментарий модуля — или
`'default'`) не делает ничего: не фейлит конструктор, не диспатчит
никакого события. По W3C Notifications API Level 1 §6 «create a
notification»: если разрешение источника не `"granted"` (в частности —
несекьюрный контекст, который спека трактует как отсутствие
разрешения), должна быть поставлена задача на диспатч события `error`
на созданном объекте `Notification`, после чего конструктор завершает
работу без показа. Сейчас этот путь — молчаливый no-op: объект
`Notification` создаётся синхронно и остаётся висеть без каких-либо
последующих событий вообще, поэтому `onclick`/`onerror`/`onshow`/
`onclose`-обработчики никогда не вызываются в denied-состоянии.

Одной строкой воспроизводится и вне WPT:

```js
new Notification('x').onerror = () => console.log('fired');
// permission = 'denied' (дефолт) → ничего не происходит вообще
```

## Масштаб

Единственный не-`.https.` сигнал категории, помимо `historical.any.html`
(2/2, не касается конструктора) и `permission.html` (1/1, читает только
`Notification.permission`). `permissions-non-secure.html` (Test TIMEOUT,
2/3 сабтеста passed, 1 unexpected) падает по другой, уже известной
причине — третий сабтест грузит `resources/permission-worker.js` как
`new Worker(url, {type:'module'})` с обычным внешним URL, что подпадает
под [BUG-364](BUG-364-FIXED.md) (внешний URL воркера не фетчится, тело
пустое) — реконфирмация, не новая находка. Остальные 20/24 id — все
`.https.`, TIMEOUT на уже задокументированном TLS-гэпе `UnknownIssuer`
(`docs/wpt-status.md:25-28`) либо ERROR/TIMEOUT на несвязанных с этим
багом причинах (relative-URL fetch в service worker — класс BUG-346/347,
не проверялось детально: HTTPS-гэп срабатывает раньше).

## Дальше

Fix scope: в `else`-ветке (или явном `if (_permission !== 'granted')`)
добавить `queueMicrotask(function() { self._fire('error'); });` —
симметрично существующей `granted`-ветке. Не требует изменения
`_permission`-модели или native-биндингов; вне скоупа этой
WPT-VENDOR-задачи (только вендоринг + прогон).

## Исправление (P3, 2026-09-25)

Конструктор в `NOTIFICATIONS_SHIM` получил ветку для не-`granted`
разрешения (`denied`, `default`, несекьюрный контекст): на созданном
объекте ставится отложенный `_fire('error')` — симметрично `show` в
`granted`-ветке, через тот же `queueMicrotask`. Отложенность существенна:
страница (и WPT) назначает `onerror` уже *после* `new`, синхронный
диспатч никто бы не услышал. Уведомление в этой ветке, как и раньше, не
показывается и в очередь ОС не попадает.

Проверка:

- регресс-тесты `onerror_fired_when_not_granted` (обработчик и слушатель
  `addEventListener` получают `error`, назначенные после `new`, при
  отложенной очереди микрозадач) и `onerror_not_fired_when_granted`;
  `cargo test -p lumen-js --features v8-backend --lib notifications_bindings`
  — 30/30;
- живой WPT (`run_report.py --all --root notifications --recursive`,
  dev-release сборка слота): `constructor-non-secure.html` TIMEOUT → **PASS
  (UNEXPECTED-PASS)**, файл ожиданий
  `tests/wpt/metadata/notifications/constructor-non-secure.html.ini` удалён.
  Прогон категории целиком: 8/28 harness OK, 29/160 сабтестов.

Попутно в том же прогоне `constructor-basic.https.html` и
`idlharness.https.any.html` дали UNEXPECTED-OK против ожиданий `ERROR` —
это `.https.`-тесты, которых правка не касается (дрейф после TLS-работ
P3-tlshard); их `.ini` не тронуты — переснятие ожиданий категории отдельная
работа.
