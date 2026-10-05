"""WPT-RUN-14: are a run's reftest FAILs a layout difference or just edge AA?

For every FAIL reftest of a `run_corpus.py` out-dir, render the test and its
`rel=match` reference with `lumen --screenshot` at one size and compare pixel by
pixel. A diff made only of isolated 1-px lines (`thin-only`) is anti-aliasing /
pixel snapping, not geometry (BUG-1249); `thick` is a real difference;
`size-differs` means the two screenshots have different dimensions.

    python tests/wpt/reftest_pixdiff.py --out-dir .tmp/wpt-run14/flexbox         --prefix /css/css-flexbox/ [--binary target/dev-release/lumen.exe]

Run from the repository root. Writes `<out-dir>/pixdiff.json` (`--output`). No third-party
dependencies (the PNGs are decoded with `zlib`). Only `rel=match`: `mismatch`
and multi-reference reftests are classified `no-match-ref` or by their first
reference.

`--screenshot` captures the whole page, not the viewport, so two pages of
different height used to come out `size-differs` regardless of what is
visible (359 of 1009 in `css-writing-modes`, WPT-RUN-14 S2). A reftest
compares the viewport only, so both captures are cropped to `--viewport`
first; `size-differs` now means a capture *smaller* than the viewport.

`--viewport 800x600` is what wptrunner renders reftests at; the 300×250
default is kept so numbers stay comparable with earlier slices.
`--ahem` applies the `docs/probe-method.md` recipe: `assets/fonts/Ahem.ttf`
is copied into `.tmp/fontsroot/Microsoft/Windows/Fonts/` and every capture
runs with `LOCALAPPDATA` pointing there and `LUMEN_CPU_SYSTEM_FONTS=1`, so
`font-family: Ahem` resolves even though `/fonts/ahem.css` does not load
over `file://`.
"""
import json, os, re, shutil, struct, subprocess, sys, zlib, collections, tempfile
from concurrent.futures import ThreadPoolExecutor

ROOT = os.getcwd().replace(chr(92), '/')
BIN = 'target/dev-release/lumen.exe'  # overridden by --binary
OUT = tempfile.mkdtemp(prefix='fx-')
W, H = 300, 250  # overridden by --viewport
ENV = None  # overridden by --ahem


def unfilter(p):
    d = open(p, 'rb').read()
    pos = 8
    idat = b''
    while pos < len(d):
        n, = struct.unpack('>I', d[pos:pos + 4])
        t = d[pos + 4:pos + 8]
        b = d[pos + 8:pos + 8 + n]
        if t == b'IHDR':
            w, h, bd, ct = struct.unpack('>IIBB', b[:10])
        if t == b'IDAT':
            idat += b
        pos += 12 + n
    raw = zlib.decompress(idat)
    bpp = 4 if ct == 6 else 3
    st = w * bpp
    rows = []
    prev = bytes(st)
    i = 0
    for y in range(h):
        f = raw[i]
        line = bytearray(raw[i + 1:i + 1 + st])
        i += 1 + st
        if f == 0:
            pass
        elif f == 2:
            for x in range(st):
                line[x] = (line[x] + prev[x]) & 255
        else:
            for x in range(st):
                a = line[x - bpp] if x >= bpp else 0
                b = prev[x]
                c = prev[x - bpp] if x >= bpp else 0
                if f == 1:
                    line[x] = (line[x] + a) & 255
                elif f == 3:
                    line[x] = (line[x] + (a + b) // 2) & 255
                else:
                    p = a + b - c
                    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                    line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(line))
        prev = rows[-1]
    return w, h, bpp, rows


def shot(rel, tag):
    out = os.path.join(OUT, tag + '.png')
    subprocess.run([BIN, '--viewport', f'{W}x{H}', '--screenshot', out, 'file:///' + ROOT + '/tests/wpt' + rel],
                   capture_output=True, timeout=60, env=ENV)
    return out if os.path.isfile(out) else None


def classify(t):
    try:
        src = open('tests/wpt' + t, encoding='utf-8', errors='replace').read()
    except OSError:
        return t, 'nosrc', 0
    m = re.search(r'<link[^>]+rel=["\']match["\'][^>]*href=["\']([^"\']+)["\']', src) or \
        re.search(r'<link[^>]+href=["\']([^"\']+)["\'][^>]*rel=["\']match["\']', src)
    if not m:
        return t, 'no-match-ref', 0
    ref = m.group(1)
    ref = ref if ref.startswith('/') else os.path.dirname(t) + '/' + ref
    ref = os.path.normpath(ref).replace(chr(92), '/')
    tag = re.sub(r'\W', '_', t)[-60:]
    a, b = shot(t, tag + 'a'), shot(ref, tag + 'b')
    if not a or not b:
        return t, 'shot-failed', 0
    wa, ha, bpp, ra = unfilter(a)
    wb, hb, bppb, rb = unfilter(b)
    if min(wa, wb) < W or min(ha, hb) < H or bpp != bppb:
        return t, 'size-differs', 0
    # Only the viewport is compared, as wptrunner does (module docstring).
    ra = [row[:W * bpp] for row in ra[:H]]
    rb = [row[:W * bpp] for row in rb[:H]]
    wa, ha = W, H
    diff = set()
    for y in range(ha):
        if ra[y] == rb[y]:
            continue
        for x in range(wa):
            o = x * bpp
            if ra[y][o:o + 3] != rb[y][o:o + 3]:
                diff.add((x, y))
    if not diff:
        return t, 'identical', 0
    thin = 0
    for (x, y) in diff:
        if ((x - 1, y) not in diff and (x + 1, y) not in diff) or ((x, y - 1) not in diff and (x, y + 1) not in diff):
            thin += 1
    return t, ('thin-only' if thin == len(diff) else 'thick'), len(diff)


def main():
    import argparse
    global BIN, W, H, ENV
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--out-dir', required=True)
    ap.add_argument('--prefix', default='/')
    ap.add_argument('--binary', default=BIN)
    ap.add_argument('--jobs', type=int, default=6)
    ap.add_argument('--viewport', default=f'{W}x{H}', help='WxH, default %(default)s; wptrunner uses 800x600')
    ap.add_argument('--ahem', action='store_true', help='resolve font-family: Ahem (docs/probe-method.md recipe)')
    ap.add_argument('--output', default='pixdiff.json', help='file name inside --out-dir')
    args = ap.parse_args()
    BIN = args.binary
    W, H = (int(v) for v in args.viewport.lower().split('x'))
    if args.ahem:
        fonts = os.path.join(ROOT, '.tmp', 'fontsroot', 'Microsoft', 'Windows', 'Fonts')
        os.makedirs(fonts, exist_ok=True)
        shutil.copyfile(os.path.join(ROOT, 'assets', 'fonts', 'Ahem.ttf'), os.path.join(fonts, 'Ahem.ttf'))
        ENV = dict(os.environ, LOCALAPPDATA=os.path.join(ROOT, '.tmp', 'fontsroot'), LUMEN_CPU_SYSTEM_FONTS='1')
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import run_corpus as rc
    res, _, _ = rc.load_results(args.out_dir)
    ids = sorted(i for i, r in res.items() if i.startswith(args.prefix) and r['status'] == 'FAIL'
                 and not r.get('subtests') and not i.endswith(('-ref.html', '-ref.xht')))
    print(len(ids), 'failing reftests', flush=True)
    with ThreadPoolExecutor(args.jobs) as ex:
        rows = list(ex.map(classify, ids))
    out = os.path.join(args.out_dir, args.output)
    with open(out, 'w', encoding='utf-8') as fh:
        json.dump(rows, fh, indent=0)
    print(dict(collections.Counter(r[1] for r in rows)), '->', out)


if __name__ == '__main__':
    main()
