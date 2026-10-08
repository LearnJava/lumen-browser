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
`CN=127.0.0.1` with SAN `IP:127.0.0.1, DNS:web-platform.test, DNS:127.0.0.1,
DNS:localhost, DNS:*.localhost` plus the 30 explicit wptserve subdomains
(`www.localhost`, `www1.localhost`, …, see below). `ca-cert.pem` is a copy of the same cert —
`wptcommandline`'s `pregenerated` ssl type requires a CA cert path to exist,
but nothing in this executor (`LumenBrowser` has no `webdriver_binary`-side
trust-store injection) actually consumes it, so a minimal single self-signed
cert stands in for a full CA chain.

**`localhost`/`*.localhost` added 2026-09-22 ([BUG-1069](../../bugs/BUG-1069-FIXED.md)).**
The original SAN (`web-platform.test`/`127.0.0.1` only) stopped matching
`browsers/lumen.py::env_options`'s `browser_host` (`WPT-RUN-10`, 2026-09-04,
moved it to `"localhost"`), and this README's old note calling the mismatch
"moot" (written before [BUG-785](../../bugs/BUG-785-FIXED.md) made the CA
trusted) went stale: once the root is trusted, hostname matching is the only
check left, and it failed on every single `.https.` test
(`certificate not valid for name "localhost"`).

**Explicit subdomain SANs added 2026-10-05 ([BUG-1271](../../bugs/BUG-1271-FIXED.md)).**
The `*.localhost` wildcard matches nothing: rustls-webpki, like NSS, ignores
a wildcard with fewer than two labels after `*`, so every https request to
wptserve's subdomains (`https://www1.localhost:18443/…`) failed with
`certificate not valid for this hostname` once [BUG-1070](../../bugs/BUG-1070-FIXED.md)
made them resolve. The SAN now lists every name `tools/serve/serve.py::_subdomains`
builds (`www`, `www1`, `www2` and the two IDN labels in punycode, all one- and
two-level combinations) with the `.localhost` suffix. If wptserve's subdomain
set changes, regenerate with the list below.

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
SANS=$(python -c "
from itertools import product, chain
s = ['www', 'www1', 'www2', '天気の良い日'.encode('idna').decode(), 'élève'.encode('idna').decode()]
subs = sorted({'.'.join(x) for x in chain(*(product(s, repeat=i) for i in (1, 2)))})
print(','.join(['IP:127.0.0.1', 'DNS:web-platform.test', 'DNS:127.0.0.1', 'DNS:localhost', 'DNS:*.localhost']
               + ['DNS:%s.localhost' % x for x in subs]))")
MSYS2_ARG_CONV_EXCL="*" OPENSSL_CONF=/mingw64/etc/ssl/openssl.cnf openssl req -x509 \
  -newkey rsa:2048 -nodes -keyout host-key.pem -out host-cert.pem -days 36500 \
  -subj "/CN=127.0.0.1" \
  -addext "subjectAltName=$SANS" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature,keyEncipherment" \
  -addext "extendedKeyUsage=serverAuth"
cp host-cert.pem ca-cert.pem
```

Gotchas hit generating these (Windows/Git Bash specific):
- The ambient `OPENSSL_CONF` env var on this machine points at an unrelated
  PostgreSQL-bundled `openssl.cnf` — override it to mingw64's own or `openssl
  req` fails to even start (`BIO_new_file: no such process`). In a shell
  where MSYS path conversion is disabled globally, `/mingw64/...` reaches
  the native `openssl.exe` unconverted and fails the same way — pass
  `OPENSSL_CONF="$(cygpath -m /mingw64/etc/ssl/openssl.cnf)"` instead
  (2026-10-05, BUG-1271).
- Git Bash's POSIX-to-Windows path auto-conversion mangles any leading-slash
  argument, including `-subj "/CN=..."`, into a drive path unless
  `MSYS2_ARG_CONV_EXCL="*"` is set for the whole command.
- Without `basicConstraints=critical,CA:FALSE`, `openssl req -x509`'s default
  self-signed cert is marked `CA:TRUE` — rustls-webpki (Lumen's TLS stack)
  refuses to accept that as an end-entity/leaf cert at all
  (`CaUsedAsEndEntity`), a *different* and more confusing error than the
  expected `UnknownIssuer`.
