# TASKS — мастер-список задач zshift

Одна задача ≥ 30 минут работы (один срез = одна сессия). Статусы: ○ TODO,
● IN PROGRESS, ✓ DONE (дата). Очередь воркера 1 — ZS-STATUS-1.md (указатели
на строки этой таблицы); приоритет и зависимости срезов — ROADMAP.md,
резервация — опубликованная ветка (task-claim.sh или ветка zs1-<task>).

| id | задача | статус |
|---|---|---|
| VTT-RENDER-S1 | Разбор cue text (WebVTT §6.4) в дерево узлов — новый crates/engine/dom/src/vtt_text.rs; бриф docs/tasks/VTT-RENDER-slices.md §S1 | ✓ DONE (2026-11-04) |
| VTT-RENDER-S2 | Настройки cue в VttCueSettings + геометрия cue-бокса (crates/engine/dom/src/vtt_geometry.rs), доезд настроек до JS cue; бриф §S2 | ● IN PROGRESS (zs1) |
| VTT-RENDER-S3 | Поддерево боксов cue в UA-тени `<video>` (crates/engine/dom/src/vtt_tree.rs, ua_vtt_role); бриф §S3 | ○ TODO |
| VTT-RENDER-S4 | Раскладка cue: дети UA-тени в box_tree, стили §7.4 через apply_ua_vtt; бриф §S4 | ○ TODO |
| VTT-RENDER-S5 | Мост JS→DOM (__lumen_video_render_cues), удалить самодельный оверлей; закрывает BUG-1748; бриф §S5 | ○ TODO |
| VTT-RENDER-S6 | Обход перекрытий §7.2, BoxRole::VttCueContainer, обновление CAPABILITIES.md и docs/wpt-vendor-notes/webvtt.md; бриф §S6 | ○ TODO |
| WASM-V8-NATIVE-S1 | Убрать шим WebAssembly (родной глобал V8), set_allow_wasm_code_generation_callback; ветка p1-wasm-v8-native-s1 | ● IN PROGRESS |
| WASM-V8-NATIVE-S2 | compileStreaming/instantiateStreaming через set_wasm_streaming_callback, v8_runtime/wasm_streaming.rs; бриф §S2 | ○ TODO |
| WASM-V8-NATIVE-S3 | Удалить мёртвый интерпретатор crates/js/src/wasm/ (~5.7 тыс. строк), обновить CAPABILITIES.md и wpt-доки; бриф §S3 | ○ TODO |
