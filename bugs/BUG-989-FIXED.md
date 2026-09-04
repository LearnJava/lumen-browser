# BUG-989 — встроенный блокировщик отклоняет главный документ навигации, а не подресурс

**Статус:** FIXED 2026-09-05
**Заведён:** 2026-09-04 (живой прогон корпуса «топ-100 зарубежных»)
**Область:** `crates/network/src/filter/easylist.rs`
**Владелец:** P3

## Симптом

Навигация на верхнеуровневый адрес завершается сетевой ошибкой от собственного
блокировщика, и сайт не открывается вовсе:

```
Ошибка загрузки https://www.temu.com/: network error: blocked: easylist
```

## Что говорит измерение

Прогон 100 сайтов 2026-09-04 (`.tmp/perf-audit/20260904-150604/results.json`):
три сайта получили `TIMEOUT` именно по этой причине — **temu, imgur,
soundcloud**. Это 3 из 29 таймаутов прогона; остальные 26 объясняются ответами
403/401, сбоем DNS, HTTP/2 и TLS.

Для сравнения: на подресурсах блокировщик отработал 60 раз на 27 сайтах и
никаких проблем не создал — это его штатная работа.

## Причина

Дело не в передаче типа ресурса (top-level навигация уже была помечена
`RequestContext::is_top_level`, и BUG-292 уже исключал из top-level матчинга
правила с явным `$script`/`$image`/… — `crates/network/src/filter/easylist.rs`,
`RuleOptions::matches`). Дефект — в конкретных правилах из `easylist.txt`/
`easyprivacy.txt`, реально сработавших на этих трёх доменах:

- `||temu.com^$popup,domain=pornproxy.art` — `popup` не входит в
  `type_option_bit` (нет бита `ResourceType`), `domain=` — тоже
  нераспознанный модификатор; оба отбрасывались при разборе, и
  `RuleOptions.types` оставался `None`.
- `||imgur.com^$domain=ghostbin.me|up-load.io` — нет вообще ни одного
  распознанного типа, только `domain=`, тоже отброшенное.
- `||soundcloud.com^$ping` — `ping` не входит в `type_option_bit` по той же
  причине, что `popup`.

Во всех трёх случаях `RuleOptions.types == None` — а по существовавшей логике
(`is_top_level && self.types.is_some()`) именно `None` считался «нетипизированным,
безусловным доменным блоком» и по BUG-292-исключению из top-level матчинга НЕ
выводился, то есть блокировал документ верхнего уровня наравне с подресурсами.
Но семантически все три правила условны — они писались для конкретного
подресурса/попапа/реферера, а не для домена целиком; молчаливое отбрасывание
непонятого модификатора превращало условное правило в безусловное.

## Фикс

`crates/network/src/filter/easylist.rs`: у `RuleOptions` появилось поле
`narrows_beyond_domain: bool`, выставляемое при `domain=` и при любом
распознанном, но не смоделированном ключевом слове типа (`popup`, `popunder`,
`ping`, `websocket`, `webrtc`, `document`, `elemhide`, `generichide`,
`genericblock` — `UNMODELLED_TYPE_KEYWORDS`). Поле **не сужает** матчинг
подресурсов (сохранена прежняя консервативная семантика «непонятый модификатор
не должен снижать блокировку», покрыта тестами
`domain_option_ignored_not_narrowing`/`unmodelled_option_does_not_narrow`), но
добавлено в уже существующее условие исключения top-level навигации из
матчинга:

```rust
if ctx.is_top_level && (self.types.is_some() || self.narrows_beyond_domain) {
    return false;
}
```

Итог: правило типа `$popup,domain=…`/`$domain=…`/`$ping` по-прежнему блокирует
подресурсы домена как и раньше (никакого over-allow), но больше не
срабатывает на прямую навигацию верхнего уровня — только настоящий
безусловный `||host^` без единого `$`-модификатора продолжает блокировать сам
документ (`top_level_navigation_still_blocked_by_untyped_rule`).

### Проверка фикса

Новые тесты `top_level_navigation_not_blocked_by_domain_option_rule` (imgur)
и `top_level_navigation_not_blocked_by_unmodelled_type_rule` (soundcloud
`$ping`, temu `$popup,domain=`) воспроизводят ровно три правила из отчёта и
проверяют, что: (1) top-level навигация на сайт больше не блокируется, (2)
подресурсы того же домена всё ещё блокируются. `cargo test -p lumen-network
--lib filter::easylist` — 33/33 (включая старые `domain_option_ignored_not_narrowing`,
`unmodelled_option_does_not_narrow`, `top_level_navigation_not_blocked_by_typed_rule`,
`top_level_navigation_still_blocked_by_untyped_rule` — без регрессии). `cargo
clippy -p lumen-network --all-targets -- -D warnings` чисто.

Живой повторный прогон корпуса (temu/imgur/soundcloud открываются) не
выполнялся в этой сессии — фикс верифицирован юнит-тестами, воспроизводящими
точные правила из `results.json` прошлого прогона.

## Сырые данные

`.tmp/perf-audit/20260904-150604/results.json` (slug `temu`, `imgur`,
`soundcloud`), `live.stderr.*.log`.
