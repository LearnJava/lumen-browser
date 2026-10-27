#!/usr/bin/env python3
"""Print the `subjectAltName` value for `host-cert.pem` (see README.md).

Every name wptserve publishes for the runner's `browser_host`
(`web-platform.test`, the alternate `not-web-platform.test`, their 30
subdomains each and the two `nonexistent.*` names — 66 in all) comes from
`ConfigBuilder` itself via `host_audit.configured_domains`, so the SAN list
cannot drift from the server's. Explicit names only: rustls-webpki ignores a
wildcard (BUG-1271). The `localhost` family stays for ad-hoc probes that still
serve on it.

Usage (repo root, wpt venv): `python tests/wpt/certs/sans.py`
"""

import os
import sys
from itertools import chain, product

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import host_audit  # noqa: E402


def localhost_names():
    labels = ["www", "www1", "www2",
              "天気の良い日".encode("idna").decode(), "élève".encode("idna").decode()]
    subs = {".".join(x) for x in chain(*(product(labels, repeat=i) for i in (1, 2)))}
    return ["localhost"] + sorted(f"{s}.localhost" for s in subs)


def main():
    families, _override, _not_domains = host_audit.configured_domains()
    wpt = sorted({name for hosts in families.values() for name in hosts.values()})
    names = ["IP:127.0.0.1", "DNS:127.0.0.1"]
    names += [f"DNS:{n}" for n in wpt + localhost_names()]
    print(",".join(names))
    print(f"{len(wpt)} wptserve names, {len(names)} SAN entries", file=sys.stderr)


if __name__ == "__main__":
    main()
