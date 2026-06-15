# BUG-057

**Статус:** FIXED 2026-06-03
**Компонент:** paint
**Файл:** `crates/engine/paint/src/renderer.rs:1578`

## Описание

wgpu Vulkan crash on first render after page load: «Encoder is invalid» validation error → double panic при drop SurfaceAcquireSemaphores; воспроизводится на Windows Vulkan backend; fix: DX12 backend по умолчанию на Windows в Renderer::new_async + new_headless_async
