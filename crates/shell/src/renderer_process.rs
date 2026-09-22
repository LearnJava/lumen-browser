//! Управление жизненным циклом подпроцесса `lumen-renderer` (PH3-GPUSANDBOX
//! Phase A, срез A3 — шаг 3 из `docs/tasks/ph3-gpu-process-sandbox.md`).
//!
//! Зеркалит `network_service.rs`: `spawn()` запускает бинарник, читает порт из
//! его первой строки stdout, подключается и возвращает готовый `IpcClient`.
//! Дроп хендла убивает дочерний процесс.
//!
//! # Статус подключения
//!
//! Ничего в шелле пока не вызывает `RendererProcessHandle::spawn()` — шелл
//! по-прежнему рисует через `backend_factory::create_backend()` (in-process
//! путь). `lumen-renderer` (срез A2) отвечает на `GpuInit`/`GpuRender` без
//! реального `wgpu::Device`, так что подмена живого бэкенда на этот процесс
//! сейчас дала бы пустой экран. Замена (`RemoteRenderBackend`, шаг 5) —
//! отдельный срез, после того как рендерер получит настоящий wgpu-путь.
#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use lumen_core::error::{Error, Result};
use lumen_ipc::IpcClient;

/// Хендл живого подпроцесса `lumen-renderer`.
///
/// Дроп хендла убивает дочерний процесс (и тем самым закрывает TCP-соединение).
pub struct RendererProcessHandle {
    child: Child,
}

impl RendererProcessHandle {
    /// Запустить `lumen-renderer` из той же директории, что и текущий исполняемый файл.
    ///
    /// Блокирует до тех пор, пока процесс не напечатает порт и шелл не подключится.
    ///
    /// Возвращает `(handle, client)`:
    /// - `handle` — хендл процесса; держи живым, пока нужен рендерер
    /// - `client` — IPC-клиент для отправки `GpuInit`/`GpuRender`/`GpuResize`/`GpuSurfaceLost`
    pub fn spawn() -> Result<(Self, IpcClient)> {
        let exe_path = std::env::current_exe()
            .map_err(|e| Error::Io(format!("current_exe: {e}")))?;
        let exe_dir = exe_path
            .parent()
            .ok_or_else(|| Error::Io("current_exe has no parent dir".into()))?;

        // On Windows the binary has .exe extension; on Unix there's none.
        #[cfg(windows)]
        let bin_name = "lumen-renderer.exe";
        #[cfg(not(windows))]
        let bin_name = "lumen-renderer";

        let bin_path = exe_dir.join(bin_name);

        let mut child = Command::new(&bin_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| Error::Io(format!("spawn {}: {e}", bin_path.display())))?;

        // Read the port number printed by lumen-renderer on its first stdout line.
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Io("no stdout from renderer process".into()))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| Error::Io(format!("read port from renderer process: {e}")))?;

        let port: u16 = line.trim().parse().map_err(|e| {
            Error::Io(format!(
                "renderer process printed invalid port {:?}: {e}",
                line.trim()
            ))
        })?;

        let client = IpcClient::connect(port)?;
        Ok((Self { child }, client))
    }
}

impl Drop for RendererProcessHandle {
    fn drop(&mut self) {
        // Kill the child; it exits cleanly when its TCP socket is closed anyway.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    /// Smoke test: verify RendererProcessHandle::spawn() fails gracefully when
    /// the binary is absent (expected in test environment — no actual binary
    /// built as part of `cargo test -p lumen-shell`).
    #[test]
    fn spawn_missing_binary_returns_error() {
        let result = super::RendererProcessHandle::spawn();
        assert!(result.is_err(), "expected Err when binary is absent");
    }
}
