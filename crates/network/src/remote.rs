//! `RemoteNetworkTransport` — реализация `NetworkTransport` через IPC к сетевому сервису.
//!
//! Используется когда сетевой стек вынесен в отдельный процесс (`lumen-network-service`).
//! Внутри — блокирующий TCP-вызов через `lumen_ipc::IpcClient`; для параллельных запросов
//! оберни в `Arc<Mutex<RemoteNetworkTransport>>`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use lumen_core::error::{Error, Result};
use lumen_core::ext::NetworkTransport;
use lumen_core::url::Url;
use lumen_ipc::{FetchRequest, IpcClient, IpcRequest, IpcResponse};

/// Реализация `NetworkTransport`, делегирующая HTTP-запросы в отдельный процесс
/// `lumen-network-service` через TCP-IPC.
///
/// Создаётся шеллом после запуска подпроцесса:
/// ```no_run
/// use lumen_network::RemoteNetworkTransport;
/// let transport = RemoteNetworkTransport::connect(12345).unwrap();
/// ```
pub struct RemoteNetworkTransport {
    client: Mutex<IpcClient>,
    next_id: AtomicU64,
}

impl RemoteNetworkTransport {
    /// Подключиться к сетевому сервису, слушающему на `127.0.0.1:port`.
    pub fn connect(port: u16) -> Result<Self> {
        let client = IpcClient::connect(port)?;
        Ok(Self { client: Mutex::new(client), next_id: AtomicU64::new(1) })
    }
}

impl NetworkTransport for RemoteNetworkTransport {
    fn fetch(&self, url: &Url) -> Result<Vec<u8>> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = IpcRequest::Fetch(FetchRequest {
            id,
            url: url.to_string(),
            method: "GET".into(),
            headers: vec![],
            body: None,
        });
        let mut client = self.client.lock().map_err(|_| Error::Io("ipc lock poisoned".into()))?;
        match client.request(&req)? {
            IpcResponse::FetchOk(ok) => Ok(ok.body),
            IpcResponse::FetchErr(err) => Err(Error::Network(err.error)),
            _ => Err(Error::Network("unexpected IPC response type".into())),
        }
    }
}
