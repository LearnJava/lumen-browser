//! Управление жизненным циклом подпроцесса `lumen-network-service`.
//!
//! Шелл запускает сетевой сервис при старте (`--network-service` флаг),
//! подключается к нему через TCP-IPC и использует `RemoteNetworkTransport`
//! вместо встроенного `HttpClient`.
//!
//! Протокол запуска:
//! 1. Шелл вызывает `NetworkServiceHandle::spawn()`.
//! 2. Сервис привязывается к случайному порту и печатает его в stdout.
//! 3. Шелл читает порт — `spawn()` возвращает `(handle, transport)`.
//! 4. Шелл передаёт `transport` в загрузчик ресурсов вместо встроенного `HttpClient`.
//! 5. При дропе хендла дочерний процесс убивается.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use lumen_core::error::{Error, Result};
use lumen_network::RemoteNetworkTransport;

/// Хендл живого подпроцесса `lumen-network-service`.
///
/// Дроп хендла убивает дочерний процесс (и тем самым закрывает TCP-соединение).
/// Транспорт (`RemoteNetworkTransport`) возвращается отдельно из `spawn()` и
/// передаётся в загрузчик ресурсов.
pub struct NetworkServiceHandle {
    child: Child,
}

impl NetworkServiceHandle {
    /// Запустить `lumen-network-service` из той же директории, что и текущий исполняемый файл.
    ///
    /// Блокирует до тех пор, пока сервис не напечатает порт и шелл не подключится.
    ///
    /// Возвращает `(handle, transport)`:
    /// - `handle` — хендл процесса; держи живым, пока нужен сервис
    /// - `transport` — готовый IPC-транспорт, замени им `HttpClient` в загрузчике
    pub fn spawn() -> Result<(Self, Arc<RemoteNetworkTransport>)> {
        let exe_path = std::env::current_exe()
            .map_err(|e| Error::Io(format!("current_exe: {e}")))?;
        let exe_dir = exe_path
            .parent()
            .ok_or_else(|| Error::Io("current_exe has no parent dir".into()))?;

        // On Windows the binary has .exe extension; on Unix there's none.
        #[cfg(windows)]
        let svc_name = "lumen-network-service.exe";
        #[cfg(not(windows))]
        let svc_name = "lumen-network-service";

        let svc_path = exe_dir.join(svc_name);

        let mut child = Command::new(&svc_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| Error::Io(format!("spawn {}: {e}", svc_path.display())))?;

        // Read the port number printed by the service on its first stdout line.
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Io("no stdout from network service".into()))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| Error::Io(format!("read port from network service: {e}")))?;

        let port: u16 = line.trim().parse().map_err(|e| {
            Error::Io(format!(
                "network service printed invalid port {:?}: {e}",
                line.trim()
            ))
        })?;

        let transport = Arc::new(RemoteNetworkTransport::connect(port)?);
        Ok((Self { child }, transport))
    }
}

impl Drop for NetworkServiceHandle {
    fn drop(&mut self) {
        // Kill the child; it exits cleanly when its TCP socket is closed anyway.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    /// Smoke test: verify NetworkServiceHandle::spawn() fails gracefully when the
    /// binary is absent (expected in test environment — no actual binary built).
    #[test]
    fn spawn_missing_binary_returns_error() {
        // In the test environment the binary doesn't exist; we just check that
        // the error path is reached without panic.
        let result = super::NetworkServiceHandle::spawn();
        // It's OK for the test binary to not exist; we only care that it returns Err.
        assert!(result.is_err(), "expected Err when binary is absent");
    }
}
