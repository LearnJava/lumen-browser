# BUG-169

**Статус:** FIXED 2026-06-16
**Компонент:** network + shell (Linux platform-cfg код)

## Корень

Проект разрабатывается на Windows (MSVC). `#[cfg(target_os = "linux"/"macos")]`-ветки
платформенного кода никогда не линтятся/тестируются на dev-машине, поэтому через merge
просочились ошибки сборки тестов и clippy, видимые только на Linux. Пять штук в двух
крейтах (все — тривиальные, поведение не меняют).

## Описание (network)

На Linux не компилируется тестовый бинарь `lumen-network`: два `#[cfg(target_os =
"linux")]`-теча в `crates/network/src/ctap2.rs` обращаются к `linux_hid::descriptor_is_fido`,
которая объявлена приватной (`fn descriptor_is_fido` в модуле `linux_hid`, ctap2.rs:1165).

```
error[E0603]: function `descriptor_is_fido` is private
   --> crates/network/src/ctap2.rs:1839:28
   --> crates/network/src/ctap2.rs:1850:29
```

Из-за этого `cargo test -p lumen-network` падает на этапе сборки тестов на Linux (на
Windows тесты под `#[cfg(target_os = "linux")]` исключены, поэтому баг не виден в
MSVC-сборке — отсюда и просочился через merge).

## Как воспроизвести

1. Linux.
2. `cargo test -p lumen-network` → ошибка сборки E0603 (function is private).

Дополнительно на Linux падает и `cargo clippy -p lumen-network --all-targets -D warnings`
(тот же `#[cfg(linux)]`-код ctap2 никогда не линтился на Windows-машине разработки):

```
error: unnecessary `unsafe` block            ctap2.rs:1128  (libc_poll — уже safe-обёртка)
error: this `if` can be collapsed into the outer `match`  ctap2.rs:1192
```

## Описание (shell)

На Linux падает `cargo clippy -p lumen-shell --all-targets -D warnings`:

```
error: unused imports: ScreenCaptureConfig, …  platform/screen_capture.rs:16
       (нужны только Windows-GDI-impl; на Linux stub реэкспортит Null-провайдер)
error: function `entry_from_path` is never used  platform/file_dialog.rs:116
       (зовётся только из platform-cfg'd ветки, исключённой на этой сборке)
```

## Подозрение / фикс

Все три проблемы — в `#[cfg(target_os = "linux")]`-коде `ctap2.rs`, который на основной
Windows-сборке исключён, поэтому просочились через merge. Фиксы тривиальны и не меняют
поведение:

network (ctap2.rs):
1. `descriptor_is_fido` → `pub(crate)` (1165) — видимость для in-crate тестов.
2. Снять лишний `unsafe { … }` вокруг `libc_poll` (1128) — обёртка уже safe.
3. Свернуть вложенный `if` в guard match-а (1192).

shell (platform/):
4. `#[allow(unused_imports)]` на use-блоке `screen_capture.rs:16` (no-op на Windows,
   гасит cross-platform false-positive; cfg-gating рискнул бы Windows-сборкой).
5. `#[allow(dead_code)]` на `entry_from_path` `file_dialog.rs:116`.

Все пять применены в ветке `p1-tcp-streaming-body` как Linux-unblock тестового и clippy
гейта для PH1-2a (иначе `cargo test`/`cargo clippy` для network/shell на Linux не
проходят вообще). Изменения нулевого риска, поведение не трогают. P3/P5 может закрыть
BUG как FIXED после ревизии — лучшее долгосрочное решение, вероятно, cfg-gating Linux-веток.
