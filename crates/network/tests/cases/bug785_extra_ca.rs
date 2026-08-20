//! End-to-end regression test for BUG-785: a self-signed CA is rejected by
//! default (`UnknownIssuer`) but accepted once its certificate is added to
//! the `RootCertStore` the same way [`tls::trusted_root_store`] would add it
//! from `LUMEN_EXTRA_CA_CERT`.
//!
//! Runs a real TLS handshake over a loopback `TcpStream` against a server
//! using WPT's vendored self-signed test cert (`tests/wpt/certs/host-cert.pem`
//! — identical to `ca-cert.pem`, the cert is its own issuer). Deliberately
//! bypasses the `LUMEN_EXTRA_CA_CERT` env var / `OnceLock` cache path so it
//! stays safe to run alongside other tests in the same `tests/all.rs` binary
//! (BT-1 pattern) without env-var/init-order races.

use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Bounds every blocking socket read in this test so a handshake that never
/// completes (e.g. a peer that drops the connection without a clean TLS
/// close_notify, which rustls does not always send on a validation failure)
/// fails fast instead of hanging the test process indefinitely.
const IO_TIMEOUT: Duration = Duration::from_secs(5);

use rustls::pki_types::PrivateKeyDer;
use rustls::{RootCertStore, ServerConfig};

use lumen_network::tls::{self, TlsProfile};

// WPT's vendored self-signed test cert/key (`tests/wpt/certs/host-{cert,key}.pem`).
// CN=127.0.0.1, SAN covers 127.0.0.1 and web-platform.test. Inlined so this
// test does not depend on those files' path/existence.
const TEST_CERT_PEM: &str = "\
-----BEGIN CERTIFICATE-----
MIIDXjCCAkagAwIBAgIUQOPzlblqAQyDzD7lVY+V50ZCkGUwDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJMTI3LjAuMC4xMCAXDTI2MDgwMjA2MTcwNloYDzIxMjYw
NzA5MDYxNzA2WjAUMRIwEAYDVQQDDAkxMjcuMC4wLjEwggEiMA0GCSqGSIb3DQEB
AQUAA4IBDwAwggEKAoIBAQDF1qw9u3zMp1lz7iQB3lvvgh39kStU/wYfbKRA4KZ4
5siqVOtZpisUXgHwk0fuJu0blbUAqtbrJGyG4R9k48eeAPSDltbRL3LyeZZ0HtiY
GGIugr+mZERu/rfXL6fqIB1c4xpIoGoYANTA721ZFabaD0aTOG1Cx9aTGQjxcGnJ
t/PgfFLpWJfRYWg6Wi7UzwIS+4iH1J6KqflBRtft3fqOIxIIfje6xN9ZOaHd1X3t
dvkJ5xEMwYsziluhIFT9Q/N9/VnAyliMQlU7do2sCYDxY8RrdvMiqLRwwc1/yS1t
Q0zKRuAlwBS32SLheP82dum5DschgeFMWg0Jf0PxOD8JAgMBAAGjgaUwgaIwHQYD
VR0OBBYEFA/51BUIeePPEMzTQrnDNs2fX508MB8GA1UdIwQYMBaAFA/51BUIeePP
EMzTQrnDNs2fX508MC0GA1UdEQQmMCSHBH8AAAGCEXdlYi1wbGF0Zm9ybS50ZXN0
ggkxMjcuMC4wLjEwDAYDVR0TAQH/BAIwADAOBgNVHQ8BAf8EBAMCBaAwEwYDVR0l
BAwwCgYIKwYBBQUHAwEwDQYJKoZIhvcNAQELBQADggEBAG8wxKQcdTekk/SqNJAT
pUgBbhWfh7QuacrlYqPZ9PvXzyp7DW2bMSSUE0Bu/tyXp03XACKbqf56Lc93dWPO
SuVTKdJ0NzyDGVu/ZLgXq3sfKbl+TNMWOWhCE0gQiFilM3pkqcURnkLJmbFB/8Ns
14QUq7cHj0Y3wX1/s3y+yLh5Yht/j1BNnSPzV8cP3rvZ6rVB5IfPSGuyhzP3r0nV
T+N0lu+g+Vk4UfyZ6+GRYUwA78hs2CNV3MqMqMLYgb8zveL/cVaWWtSXppu+BI3M
5wtZ1ZzlCFyUeFeF1Yso1Sxv2hLEM7zMtrjfmqLKkJLuglzxuDRWNFQjERqi0A/o
lwI=
-----END CERTIFICATE-----
";

const TEST_KEY_PEM: &str = "\
-----BEGIN PRIVATE KEY-----
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQDF1qw9u3zMp1lz
7iQB3lvvgh39kStU/wYfbKRA4KZ45siqVOtZpisUXgHwk0fuJu0blbUAqtbrJGyG
4R9k48eeAPSDltbRL3LyeZZ0HtiYGGIugr+mZERu/rfXL6fqIB1c4xpIoGoYANTA
721ZFabaD0aTOG1Cx9aTGQjxcGnJt/PgfFLpWJfRYWg6Wi7UzwIS+4iH1J6KqflB
Rtft3fqOIxIIfje6xN9ZOaHd1X3tdvkJ5xEMwYsziluhIFT9Q/N9/VnAyliMQlU7
do2sCYDxY8RrdvMiqLRwwc1/yS1tQ0zKRuAlwBS32SLheP82dum5DschgeFMWg0J
f0PxOD8JAgMBAAECggEAFTL0IOdKr6lNAgmmDRc1FbyFFysrj/+Fue9LyHHqFLFy
FiJkV6ZhHl0Wax91CTVdmeOYUhp8ThUIlglgclCgDrO+f601lpO1hvr1XrsBbYbL
Wn2DKMK8vIIJ1AKUxRcs3kutgNPDmo/YPFZLisyxpNMXNmZI+utr+DYqCakIhOdD
e3ttOsPoXkKrxskI/pivfZgT9SuLZRI+ItLLCGdAvNBCGbq8yGl/1bV8hSo4LWNi
NhhiXb+w2jWcv41I62WnlDkRC8e2gPLOnuFlQPXre7UU9v97Mt+ZiF0kE/BI8NwR
l+M0mt0NUvvUOADNTIE8pu/APjGg1ZcnWwPG/DNl4wKBgQDk8VSDEtfJ8YlEFPnU
wTLWDB4O9bIOTBXoOvmYlDsIG2w1V/owCCSLn6helEIJyqPhOdXh8SokFw6heo0K
kwK3fQHeMWD9SJjYzXr2Anvii4lZ0pQv2+Rm+HqREVqhUeD4sSVunXyOlg7xzCS7
D00cQfsnpfaAGnVaVPzlWZLcrwKBgQDdOEk7kkrPgjp0/CKf5mHJjb5N+xGJdbE0
QVeNED5iA5DAlW+m1Vay3OXrZAPLDqnEu/ozdY71vf7DdXFjpyrMM/6poDB1lBGd
d/PSod+NPdL0iYW0xd81k02CIQa2NNQtRqavHoXBJm6hJlpAfxJIrJhUAY4ThWkV
asaTTlc9xwKBgFD5uraRl5lpwO8/rA3AN8bVilwoMs4zwxvcoCODak23xVIox+jt
OF/aHKc3MRRdhBFJb4j2z7zsGtSqj/BJhxB3Oo3oUTHE16r3IqKYxlCeofoPLTKw
R9zTziY5SSD94OCVZ3P0Z/XWxXpohiVTiCaSf87KOKGeuhs1LC3CvNspAoGAcKMN
HqjhMIEVdKVAl/v8xFxIjnoMttnXDU1L38ZqjQtVs8ki3WZ4y3+QDeeRyt0/ca1o
urTbwqInyqvMvTnLn8fFneazZdqrkWsXGaNUKR1WgS5Yhu/NNAE5kM1yFmoVsqvr
iPTYk70WzTSy9W3+CEThFrzn82aVV9NTIoPcBdcCgYBNhf0JhMG8O/3aaIPEPXIe
wkTLQS7ZwjO4rDXiVwzNp6Q6Dlyzq3RvRufq+R0O76iKqZNHYBnLo9dq+FfS61qr
Phx7vmT6skdETd7oz/AAYsknKdH2YxFSYkqCXJcTNBTwwCI+qGtx2QS05SBMMPW9
RuDzxAblWY6RXXkrlovc+Q==
-----END PRIVATE KEY-----
";

// `expect()`/`unwrap()` are only lint-exempt inside a `#[test]` fn body
// (`clippy.toml`'s `allow-*-in-tests`), so these helpers propagate `Result`
// instead and the `#[test]` fns unwrap at the call site.
type BoxError = Box<dyn std::error::Error>;

fn test_server_config() -> Result<Arc<ServerConfig>, BoxError> {
    let certs: Vec<_> =
        rustls_pemfile::certs(&mut TEST_CERT_PEM.as_bytes()).collect::<Result<_, _>>()?;
    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut TEST_KEY_PEM.as_bytes())?
        .ok_or("test key PEM contained no private key")?;
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(Arc::new(cfg))
}

/// Spawn a one-shot TLS server on loopback and return its port. The spawned
/// thread performs exactly one handshake (`complete_io`) and exits; handshake
/// errors on the server side (expected in the "untrusted" case, where the
/// client aborts after seeing `UnknownIssuer`) are swallowed since only the
/// client-side result is under test.
fn spawn_test_server() -> Result<u16, BoxError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let cfg = test_server_config()?;
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
            if let Ok(mut conn) = rustls::ServerConnection::new(cfg) {
                let _ = conn.complete_io(&mut stream);
            }
        }
    });
    Ok(port)
}

fn client_handshake(port: u16, root_store: RootCertStore) -> Result<(), BoxError> {
    let server_name = rustls::pki_types::ServerName::try_from("127.0.0.1")?;
    let config = tls::build_client_config(TlsProfile::Standard, root_store);
    let mut conn = rustls::ClientConnection::new(Arc::new(config), server_name)?;
    let mut tcp = TcpStream::connect(("127.0.0.1", port))?;
    tcp.set_read_timeout(Some(IO_TIMEOUT))?;
    conn.complete_io(&mut tcp)?;
    Ok(())
}

#[test]
fn handshake_fails_without_the_extra_ca() {
    let port = spawn_test_server().unwrap();
    // Built-in webpki roots only — mirrors every pre-fix call site, and the
    // exact BUG-785 symptom: WPT's self-signed CA is not in that bundle.
    let root_store = tls::trusted_root_store();
    let err = client_handshake(port, root_store).expect_err("self-signed cert must be rejected");
    let msg = format!("{err}");
    assert!(msg.contains("UnknownIssuer"), "expected UnknownIssuer, got: {msg}");
}

#[test]
fn handshake_succeeds_once_the_cert_is_added_to_the_root_store() {
    let port = spawn_test_server().unwrap();
    let mut root_store = RootCertStore::empty();
    let certs: Vec<_> = rustls_pemfile::certs(&mut TEST_CERT_PEM.as_bytes())
        .collect::<Result<_, _>>()
        .unwrap();
    let (added, rejected) = root_store.add_parsable_certificates(certs);
    assert_eq!((added, rejected), (1, 0));

    client_handshake(port, root_store).expect("handshake must succeed once the CA is trusted");
}
