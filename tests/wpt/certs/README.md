# WPT-RUN-2 pregenerated TLS cert

Not vendored from upstream WPT — generated locally for this project's offline
`.https.` test support (`docs/tasks/p2-wpt-runner-throughput.md`, WPT-RUN-2).
`tests/wpt/run_smoke.py` passes these paths to wptrunner's
`--ssl-type=pregenerated`, pinning https certificate allocation to a fixed,
committed cert instead of auto-detecting an `openssl` binary on `PATH` at run
time (see the comment above `run_smoke.py`'s `argv` for why that auto-detect
is not deterministic across machines/CI).

`host-cert.pem`/`host-key.pem` are one self-signed leaf certificate (100-year
expiry — this project's offline-only rule rules out ACME/live reissuance) for
`CN=127.0.0.1` with SAN `IP:127.0.0.1, DNS:web-platform.test, DNS:127.0.0.1`
(matches `browsers/lumen.py::env_options`'s `browser_host`). `ca-cert.pem` is
a copy of the same cert — `wptcommandline`'s `pregenerated` ssl type requires
a CA cert path to exist, but nothing in this executor (`LumenBrowser` has no
`webdriver_binary`-side trust-store injection) actually consumes it, so a
minimal single self-signed cert stands in for a full CA chain.

**This cert is not trusted by Lumen's own TLS client** (`crates/network`) —
Lumen validates against the real Mozilla root list like any browser, so a
`.https.` test currently fails fast with `TLS handshake: invalid peer
certificate: UnknownIssuer` instead of the pre-fix `invalid port: "None"`
hang. That is the documented, expected residual of WPT-RUN-2's HTTPS-port
half (DoD: "reaches and reports", not "passes") — making Lumen trust a test
CA is a separate, security-sensitive Rust-side change out of this task's
Python-tooling scope, not attempted here.

Regenerate (Git Bash, `openssl` from `/mingw64/bin`; the default
`OPENSSL_CONF`/`MSYS2_ARG_CONV_EXCL` gotchas below cost real time to work out
the first time):

```bash
cd tests/wpt/certs
MSYS2_ARG_CONV_EXCL="*" OPENSSL_CONF=/mingw64/etc/ssl/openssl.cnf openssl req -x509 \
  -newkey rsa:2048 -nodes -keyout host-key.pem -out host-cert.pem -days 36500 \
  -subj "/CN=127.0.0.1" \
  -addext "subjectAltName=IP:127.0.0.1,DNS:web-platform.test,DNS:127.0.0.1" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature,keyEncipherment" \
  -addext "extendedKeyUsage=serverAuth"
cp host-cert.pem ca-cert.pem
```

Gotchas hit generating these (Windows/Git Bash specific):
- The ambient `OPENSSL_CONF` env var on this machine points at an unrelated
  PostgreSQL-bundled `openssl.cnf` — override it to mingw64's own or `openssl
  req` fails to even start (`BIO_new_file: no such process`).
- Git Bash's POSIX-to-Windows path auto-conversion mangles any leading-slash
  argument, including `-subj "/CN=..."`, into a drive path unless
  `MSYS2_ARG_CONV_EXCL="*"` is set for the whole command.
- Without `basicConstraints=critical,CA:FALSE`, `openssl req -x509`'s default
  self-signed cert is marked `CA:TRUE` — rustls-webpki (Lumen's TLS stack)
  refuses to accept that as an end-entity/leaf cert at all
  (`CaUsedAsEndEntity`), a *different* and more confusing error than the
  expected `UnknownIssuer`.
