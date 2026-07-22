# BUG-329: `system_fonts::tests::finds_bundled_inter` красный — DS-4 добавила 2 семейства шрифтов, тест ожидает ровно 1

**Статус:** FIXED 2026-07-22
**Дата:** 2026-07-22
**Компонент:** font (`crates/engine/font/src/system_fonts.rs:308`)
**Найден:** `scoped-test.sh` на ветке `p1-ds-5-idn-homoglyph` (P1, гейт перед мержем DS-5). Подтверждено:
красный уже на `main` (0000b645) — регрессия не от DS-5, а от предыдущего мержа DS-4 (26fdb0c8, шрифты
Golos Text + JetBrains Mono в `assets/fonts/`).
**Исправлен:** `aad9f0e5` — assertion в `finds_bundled_inter` обновлена на `family_count() == 3`
(Inter/Golos Text/JetBrains Mono), остальные Inter-специфичные проверки не тронуты.

## Симптом

```
thread 'system_fonts::tests::finds_bundled_inter' panicked at crates\engine\font\src\system_fonts.rs:311:9:
assertion `left == right` failed: should find exactly one family in assets/fonts
  left: 3
 right: 1
```

## Причина

Тест `finds_bundled_inter` (`system_fonts.rs:307-313`) жёстко ожидает `idx.family_count() == 1` для
директории `assets/fonts/`, комментируя это как «ровно одно семейство». DS-4 добавила туда Golos Text и
JetBrains Mono ([CLAUDE.md](../CLAUDE.md), [subsystems/shell.md](../subsystems/shell.md)) — теперь в
директории 3 шрифтовых семейства (Inter, Golos Text, JetBrains Mono), утверждение устарело. Остальные
проверки теста (что именно `Inter` находится один раз, метаданные) по-прежнему валидны и не задеты.

## Похожий/соседний файл теста

`bundled_inter_has_metadata` и `pick_face_returns_only_face_for_inter` фильтруют по имени `Inter` и не
зависят от общего количества семейств — они не сломаны.

## Предлагаемый фикс

Обновить assertion в `finds_bundled_inter` на `family_count() == 3` (или явно перечислить три ожидаемых
семейства — `Inter`/`Golos Text`/`JetBrains Mono`), не ослабляя остальные проверки конкретно про `Inter`.
