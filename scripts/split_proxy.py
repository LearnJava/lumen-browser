#!/usr/bin/env python3
"""Локальный split-прокси для живых перф-прогонов при включённом VPN (TUN).

Зачем (docs/perf/journal.md, 2026-09-23): TLS-рукопожатие через VPN-туннель —
0.4-1 с с выбросами до 15 с у всех клиентов, напрямую — 50-60 мс. Сравнение
Lumen с Chrome через туннель меряет VPN, а не движки. Прокси отправляет трафик
мимо туннеля (исходящий сокет привязан к адресу физического интерфейса), а
хосты, недоступные напрямую, — в туннель (сокет без привязки, маршрут по
умолчанию = TUN).

Решение «напрямую / туннель» на хост:
  1. список --tunnel-hosts (суффиксы доменов, по одному на строке) — туннель;
  2. иначе напрямую (TCP-connect с таймаутом --direct-timeout). Если connect не
     прошёл или прямое соединение закрылось, когда клиент отправил байты, а
     сервер не ответил ни одним (сброс/чёрная дыра DPI после ClientHello), —
     дальше этот host:port идёт в туннель; решение живёт весь прогон, пишется
     в лог. Первый такой запрос клиент теряет, поэтому известные блокировки
     заданы списком заранее: `--probe <файл хостов>` делает TCP+TLS напрямую и
     печатает недоступные.

Протокол: HTTP-прокси — CONNECT (HTTPS) и absolute-form GET (HTTP).
Lumen: `proxy = "http://127.0.0.1:<port>"` в data/fingerprint.toml рядом с
lumen.exe (флаг CLI `--proxy` сейчас не действует); perf_audit.py — `--proxy`
проверяет, что строка на месте.

  python scripts/split_proxy.py --bind 192.168.0.111 --port 8899 \\
      --tunnel-hosts docs/perf/split-tunnel-hosts.txt --log .tmp/split.log
  python scripts/split_proxy.py --bind 192.168.0.111 --probe hosts.txt
"""
from __future__ import annotations

import argparse
import asyncio
import socket
import ssl
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

DIRECT: dict[str, bool] = {}
TUNNEL_SUFFIXES: list[str] = []
LOG = None
STATS = {"direct": 0, "tunnel": 0, "fail": 0}


def log(msg: str) -> None:
    line = f"{time.strftime('%H:%M:%S')} {msg}"
    if LOG:
        LOG.write(line + "\n")
        LOG.flush()


def in_tunnel_list(host: str) -> bool:
    h = host.lower().rstrip(".")
    return any(h == s or h.endswith("." + s) for s in TUNNEL_SUFFIXES)


async def open_conn(host: str, port: int, bind: str | None, timeout: float):
    local = (bind, 0) if bind else None
    return await asyncio.wait_for(
        asyncio.open_connection(host, port, local_addr=local), timeout)


async def connect_upstream(host: str, port: int, args):
    """-> (reader, writer, direct_key|None)."""
    if in_tunnel_list(host):
        STATS["tunnel"] += 1
        return (*await open_conn(host, port, None, 30), None)
    key = f"{host}:{port}"
    if DIRECT.get(key, True):
        try:
            conn = await open_conn(host, port, args.bind, args.direct_timeout)
            if key not in DIRECT:
                DIRECT[key] = True
            STATS["direct"] += 1
            return (*conn, key)
        except (OSError, asyncio.TimeoutError) as e:
            if key not in DIRECT:
                log(f"AUTO-TUNNEL {key}: direct failed ({type(e).__name__}: {e})")
            DIRECT[key] = False
    STATS["tunnel"] += 1
    return (*await open_conn(host, port, None, 30), None)


async def pipe(r: asyncio.StreamReader, w: asyncio.StreamWriter, cnt: dict, side: str) -> None:
    try:
        while True:
            data = await r.read(65536)
            if not data:
                break
            cnt[side] += len(data)
            w.write(data)
            await w.drain()
    except (OSError, asyncio.IncompleteReadError):
        pass
    finally:
        try:
            w.close()
        except OSError:
            pass


async def handle(cr: asyncio.StreamReader, cw: asyncio.StreamWriter, args) -> None:
    try:
        head = await asyncio.wait_for(cr.readuntil(b"\r\n\r\n"), 30)
    except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, asyncio.TimeoutError, OSError):
        cw.close()
        return
    first, *hdrs = head.decode("latin-1").split("\r\n")
    parts = first.split(" ")
    if len(parts) < 3:
        cw.close()
        return
    method, target, ver = parts[0], parts[1], parts[2]
    try:
        if method.upper() == "CONNECT":
            host, _, port = target.rpartition(":")
            host = host.strip("[]")
            ur, uw, dkey = await connect_upstream(host, int(port or 443), args)
            cw.write(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            await cw.drain()
        else:
            # absolute-form: http://host[:port]/path
            if not target.lower().startswith("http://"):
                cw.write(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                cw.close()
                return
            rest = target[7:]
            hostport, slash, path = rest.partition("/")
            host, _, port = hostport.partition(":")
            ur, uw, dkey = await connect_upstream(host, int(port or 80), args)
            keep = [h for h in hdrs if h and not h.lower().startswith(("proxy-", "connection:"))]
            req = f"{method} /{path} {ver}\r\n" + "\r\n".join(keep) + "\r\nConnection: close\r\n\r\n"
            uw.write(req.encode("latin-1"))
            await uw.drain()
    except (OSError, asyncio.TimeoutError, ValueError) as e:
        STATS["fail"] += 1
        log(f"FAIL {method} {target}: {type(e).__name__}: {e}")
        try:
            cw.write(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
            await cw.drain()
        except OSError:
            pass
        cw.close()
        return
    if args.log_conns:
        log(f"CONN {method} {target} {'direct' if dkey else 'tunnel'}")
    cnt = {"up": 0, "down": 0}
    await asyncio.gather(pipe(cr, uw, cnt, "up"), pipe(ur, cw, cnt, "down"))
    # Прямое соединение: клиент отправил байты (ClientHello), сервер не ответил
    # ничем и соединение закрылось — сброс/обрыв DPI. Дальше этот хост — в туннель.
    if dkey and cnt["up"] > 0 and cnt["down"] == 0 and DIRECT.get(dkey, True):
        DIRECT[dkey] = False
        log(f"AUTO-TUNNEL {dkey}: direct sent {cnt['up']} B, got 0 B")


def probe_one(host: str, bind: str, timeout: float) -> tuple[str, str]:
    try:
        s = socket.create_connection((host, 443), timeout=timeout, source_address=(bind, 0))
    except OSError as e:
        return host, f"tcp:{type(e).__name__}"
    try:
        ctx = ssl.create_default_context()
        s.settimeout(timeout)
        with ctx.wrap_socket(s, server_hostname=host):
            return host, "ok"
    except ssl.SSLCertVerificationError:
        return host, "ok"  # рукопожатие дошло — не блокировка
    except OSError as e:
        return host, f"tls:{type(e).__name__}"
    finally:
        s.close()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bind", required=True, help="IP физического интерфейса (мимо TUN)")
    ap.add_argument("--port", type=int, default=8899)
    ap.add_argument("--tunnel-hosts", help="файл суффиксов доменов, которые всегда идут в туннель")
    ap.add_argument("--direct-timeout", type=float, default=4.0)
    ap.add_argument("--log", help="лог решений AUTO-TUNNEL / FAIL")
    ap.add_argument("--log-conns", action="store_true",
                    help="писать в лог каждое клиентское соединение (CONN) — число TCP на хост за прогон")
    ap.add_argument("--probe", help="файл хостов: проверить TCP+TLS напрямую, напечатать недоступные")
    args = ap.parse_args()

    if args.probe:
        hosts = [h.strip() for h in Path(args.probe).read_text(encoding="utf-8").splitlines()
                 if h.strip() and not h.startswith("#")]
        with ThreadPoolExecutor(32) as ex:
            res = list(ex.map(lambda h: probe_one(h, args.bind, 6.0), hosts))
        for h, st in sorted(res):
            if st != "ok":
                print(f"{h}\t{st}")
        print(f"# {sum(st != 'ok' for _, st in res)} из {len(res)} недоступны напрямую", file=sys.stderr)
        return 0

    global LOG
    if args.tunnel_hosts:
        TUNNEL_SUFFIXES.extend(
            ln.split()[0].lower() for ln in Path(args.tunnel_hosts).read_text(encoding="utf-8").splitlines()
            if ln.strip() and not ln.startswith("#"))
    if args.log:
        Path(args.log).parent.mkdir(parents=True, exist_ok=True)
        LOG = open(args.log, "a", encoding="utf-8")

    async def serve():
        srv = await asyncio.start_server(lambda r, w: handle(r, w, args), "127.0.0.1", args.port)
        log(f"listen 127.0.0.1:{args.port} bind={args.bind} tunnel_suffixes={len(TUNNEL_SUFFIXES)}")
        print(f"split-proxy on 127.0.0.1:{args.port}", flush=True)

        async def stats():
            while True:
                await asyncio.sleep(60)
                log(f"STATS {STATS} auto_tunnel={sum(1 for v in DIRECT.values() if not v)}")
        asyncio.create_task(stats())
        async with srv:
            await srv.serve_forever()

    asyncio.run(serve())
    return 0


if __name__ == "__main__":
    sys.exit(main())
