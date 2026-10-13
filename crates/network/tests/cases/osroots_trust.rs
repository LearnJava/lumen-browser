//! COMPAT-TRUST-OSROOTS: a root found in the OS store makes a handshake that
//! would be `UnknownIssuer` succeed, and the same root listed in `Disallowed`
//! keeps it rejected. The OS store itself is simulated by an [`OsTrust`] value
//! (adding to the real `CurrentUser\Root` pops a confirmation dialog); the
//! Windows enumeration is covered by `tls::os_roots` unit tests.

use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{RootCertStore, ServerConfig};

use lumen_network::tls::os_roots::{apply_os_trust, OsTrust};
use lumen_network::tls::{self, TlsProfile};

const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// WPT's self-signed host cert/key (CN=127.0.0.1, SAN 127.0.0.1) — the stand-in
/// for an antivirus HTTPS-inspection root that is also the serving certificate.
const CERT_PEM: &str = include_str!("../../../../tests/wpt/certs/host-cert.pem");
const KEY_PEM: &str = include_str!("../../../../tests/wpt/certs/host-key.pem");

type BoxError = Box<dyn std::error::Error>;

fn cert_der() -> Result<CertificateDer<'static>, BoxError> {
    rustls_pemfile::certs(&mut CERT_PEM.as_bytes()).next().ok_or("no certificate")?.map_err(Into::into)
}

/// The built-in roots, as `tls::trusted_root_store` starts from (an empty store is rejected by `build_client_config`).
fn mozilla_store() -> RootCertStore {
    let mut store = RootCertStore::empty();
    store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    store
}

fn spawn_server() -> Result<(u16, thread::JoinHandle<()>), BoxError> {
    let certs: Vec<_> = rustls_pemfile::certs(&mut CERT_PEM.as_bytes()).collect::<Result<_, _>>()?;
    let key: PrivateKeyDer<'static> =
        rustls_pemfile::private_key(&mut KEY_PEM.as_bytes())?.ok_or("no private key")?;
    let cfg = Arc::new(ServerConfig::builder().with_no_client_auth().with_single_cert(certs, key)?);
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
            if let Ok(mut conn) = rustls::ServerConnection::new(cfg) {
                let _ = conn.complete_io(&mut stream);
            }
        }
    });
    Ok((port, handle))
}

fn handshake(port: u16, store: RootCertStore) -> Result<(), BoxError> {
    let name = rustls::pki_types::ServerName::try_from("127.0.0.1")?;
    let config = tls::build_client_config(TlsProfile::Standard, store);
    let mut conn = rustls::ClientConnection::new(Arc::new(config), name)?;
    let mut tcp = TcpStream::connect(("127.0.0.1", port))?;
    tcp.set_read_timeout(Some(IO_TIMEOUT))?;
    conn.complete_io(&mut tcp)?;
    Ok(())
}

#[test]
fn os_installed_root_makes_the_handshake_succeed() {
    let (port, server) = spawn_server().unwrap();
    let mut store = mozilla_store();
    apply_os_trust(&mut store, &OsTrust { trusted: vec![cert_der().unwrap()], disallowed: vec![] });
    handshake(port, store).expect("a root from the OS store must be trusted");
    server.join().expect("server thread");
}

#[test]
fn disallowed_os_root_stays_rejected() {
    let (port, server) = spawn_server().unwrap();
    let mut store = mozilla_store();
    let der = cert_der().unwrap();
    apply_os_trust(&mut store, &OsTrust { trusted: vec![der.clone()], disallowed: vec![der] });
    let err = handshake(port, store).expect_err("a Disallowed root must not be trusted");
    server.join().expect("server thread");
    assert!(format!("{err}").contains("UnknownIssuer"), "got: {err}");
}
