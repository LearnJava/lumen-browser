#!/usr/bin/env python3
"""Generate ICC colour-management test images for TEST-97 (ICC-6).

Produces two profiled images under ``samples/images/``:

* ``icc_p3.png``  — an RGB PNG carrying a **Display P3** ICC profile. The pixels
  are encoded so that, *after* a correct P3 → sRGB matrix-shaper transform, they
  reproduce a known set of in-gamut sRGB swatch colours. A colour-managed engine
  (Edge, Lumen) recovers those exact swatches; an engine that ignores the profile
  shows over-saturated colours.
* ``icc_cmyk.jpg`` — a CMYK JPEG carrying a synthetic **CMYK** ICC profile with an
  ``A2B0`` (mft2) device→PCS LUT. A colour-managed engine drives the CMYK ink
  through the LUT to sRGB; both Edge and Lumen read the same CLUT, so they agree.

The maths mirrors ``lumen_core::icc`` / ``lumen_core::pcs`` byte-for-byte
(same Bradford matrices, same XYZ(D65)→sRGB matrix, same sRGB OETF, same TRC
table reused from the bundled sRGB profile) so the round-trip is exact up to
8-bit quantisation — and quantisation is shared by both engines, so it does not
cause Edge/Lumen divergence.

Run from the repo root:  ``python graphic_tests/gen_icc_images.py``
The generated images are committed; this script only needs re-running if the
swatches or the profile maths change.
"""
import os
import struct
import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
SRGB_ICC = os.path.join(ROOT, "crates", "core", "tests", "fixtures", "sRGB.icc")
OUT_DIR = os.path.join(ROOT, "samples", "images")

# --- Constants copied verbatim from lumen_core (pcs.rs / icc.rs) -------------
D50 = np.array([0.964_22, 1.0, 0.825_21])
D65 = np.array([0.950_47, 1.0, 1.088_83])
BRADFORD = np.array([
    [0.895_1, 0.266_4, -0.161_4],
    [-0.750_2, 1.713_5, 0.036_7],
    [0.038_9, -0.068_5, 1.029_6],
])
BRADFORD_INV = np.array([
    [0.986_992_9, -0.147_054_3, 0.159_962_7],
    [0.432_305_3, 0.518_360_3, 0.049_291_2],
    [-0.008_528_7, 0.040_042_8, 0.968_486_7],
])
XYZ_D65_TO_SRGB = np.array([
    [3.240_454_2, -1.537_138_5, -0.498_531_4],
    [-0.969_266_0, 1.876_010_8, 0.041_556_0],
    [0.055_643_4, -0.204_025_9, 1.057_225_2],
])
SRGB_TO_XYZ_D65 = np.linalg.inv(XYZ_D65_TO_SRGB)


def bradford(src_white, dst_white):
    """Bradford adaptation matrix, matching pcs::bradford_adaptation."""
    s = BRADFORD @ src_white
    d = BRADFORD @ dst_white
    diag = np.diag(d / s)
    return BRADFORD_INV @ (diag @ BRADFORD)


D65_TO_D50 = bradford(D65, D50)
D50_TO_D65 = bradford(D50, D65)


def srgb_encode(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.003_130_8, c * 12.92, 1.055 * np.power(c, 1.0 / 2.4) - 0.055)


def srgb_decode(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.040_45, c / 12.92, np.power((c + 0.055) / 1.055, 2.4))


# --- sRGB.icc skeleton: read tag table + the shared TRC table ---------------
def load_srgb_skeleton():
    data = bytearray(open(SRGB_ICC, "rb").read())
    ntags = struct.unpack(">I", data[128:132])[0]
    tags = {}
    off = 132
    for _ in range(ntags):
        sig = bytes(data[off:off + 4])
        toff, tlen = struct.unpack(">II", data[off + 4:off + 12])
        tags[sig] = (toff, tlen)
        off += 12
    # rTRC is a curveType table: 'curv', reserved, count(u32), count*u16
    toff, _ = tags[b"rTRC"]
    count = struct.unpack(">I", data[toff + 8:toff + 12])[0]
    table = np.array(struct.unpack(f">{count}H", data[toff + 12:toff + 12 + 2 * count]),
                     dtype=np.float64) / 65535.0
    return data, tags, table


def trc_decode(table, x):
    """Linear-interpolated table lookup, matching icc::eval_table."""
    n = len(table)
    pos = np.clip(x, 0.0, 1.0) * (n - 1)
    i = np.clip(np.floor(pos).astype(int), 0, n - 2)
    frac = pos - i
    return table[i] + (table[i + 1] - table[i]) * frac


def trc_encode(table, y):
    """Inverse of trc_decode: find x with table_lerp(x) = y (table monotone)."""
    n = len(table)
    xs = np.linspace(0.0, 1.0, n)
    return np.interp(np.clip(y, 0.0, 1.0), table, xs)


def xy_to_XYZ_matrix(primaries_xy, white):
    """RGB(linear)->XYZ matrix for primaries with the given white tristimulus."""
    r, g, b = primaries_xy
    def to_XYZ(xy):
        x, y = xy
        return np.array([x / y, 1.0, (1.0 - x - y) / y])
    Mp = np.column_stack([to_XYZ(r), to_XYZ(g), to_XYZ(b)])
    S = np.linalg.solve(Mp, white)
    return Mp * S  # columns scaled so R+G+B = white


def patch_xyz_tag(data, off, xyz):
    """Overwrite an XYZType payload (s15Fixed16 X,Y,Z) at tag offset `off`."""
    for k, v in enumerate(xyz):
        fixed = int(round(v * 65536.0))
        struct.pack_into(">i", data, off + 8 + 4 * k, fixed)


def make_p3_png():
    data, tags, table = load_srgb_skeleton()
    # Display P3 primaries (SMPTE EG 432-1) + D65 white.
    p3_xy = [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)]
    Mp_d65 = xy_to_XYZ_matrix(p3_xy, D65)          # linear-P3 -> XYZ(D65)
    # Store colorants in the D50 PCS (Lumen re-adapts D50->D65).
    colorants_d50 = D65_TO_D50 @ Mp_d65
    for sig, col in ((b"rXYZ", 0), (b"gXYZ", 1), (b"bXYZ", 2)):
        patch_xyz_tag(data, tags[sig][0], colorants_d50[:, col])

    # Target sRGB swatches we want to *see* after colour management.
    swatches = [
        (0xC8, 0x32, 0x50),  # crimson
        (0x2E, 0x8B, 0x57),  # sea green
        (0x41, 0x69, 0xE1),  # royal blue
        (0xDA, 0xA5, 0x20),  # goldenrod
        (0x80, 0x80, 0x80),  # mid grey (neutral: stresses white balance)
        (0xF0, 0xF0, 0xF0),  # near white
    ]
    # Inverse pipeline: sRGB swatch -> P3-encoded device pixel.
    inv_Mp = np.linalg.inv(Mp_d65)
    px = []
    for (r, g, b) in swatches:
        lin_srgb = srgb_decode(np.array([r, g, b]) / 255.0)
        xyz65 = SRGB_TO_XYZ_D65 @ lin_srgb
        lin_p3 = inv_Mp @ xyz65
        enc = trc_encode(table, lin_p3)
        px.append(np.round(enc * 255.0).astype(np.uint8))
    px = np.array(px, dtype=np.uint8)  # (6,3)

    # 6 vertical swatch columns, 600x400.
    W, H = 900, 280
    img = np.zeros((H, W, 3), dtype=np.uint8)
    cols = np.array_split(np.arange(W), len(swatches))
    for i, c in enumerate(cols):
        img[:, c[0]:c[-1] + 1, :] = px[i]

    out = os.path.join(OUT_DIR, "icc_p3.png")
    Image.fromarray(img, "RGB").save(out, icc_profile=bytes(data))

    # --- self-check: run Lumen's *forward* pipeline, confirm we get swatches.
    Mp_recovered = D50_TO_D65 @ colorants_d50
    M = XYZ_D65_TO_SRGB @ Mp_recovered
    print("icc_p3.png  P3->sRGB round-trip (target -> recovered):")
    worst = 0
    for i, (r, g, b) in enumerate(swatches):
        lin = trc_decode(table, px[i] / 255.0)
        out_lin = M @ lin
        out_b = np.round(srgb_encode(out_lin) * 255.0).astype(int)
        err = np.max(np.abs(out_b - np.array([r, g, b])))
        worst = max(worst, err)
        print(f"  ({r:3d},{g:3d},{b:3d}) -> ({out_b[0]:3d},{out_b[1]:3d},{out_b[2]:3d})  d={err}")
    print(f"  worst channel error: {worst}/255")
    return out


# --- CMYK profile (A2B0 mft2) + CMYK JPEG -----------------------------------
SRGB_TO_XYZ_D65_M = SRGB_TO_XYZ_D65  # alias for clarity in CMYK maths


def cmyk_to_xyz_d50(c, m, y, k):
    """Naive multiplicative ink model -> linear sRGB -> XYZ(D50).

    Deliberately simple and smooth so an N-linear CLUT reproduces it well; it is
    the *ground truth* both Edge and Lumen read from the embedded CLUT, so the
    exact model only has to be self-consistent."""
    lin = np.array([(1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k)])
    xyz65 = SRGB_TO_XYZ_D65_M @ lin
    return D65_TO_D50 @ xyz65


def build_cmyk_profile(grid=9):
    """Build a minimal valid CMYK device profile with an mft2 A2B0 LUT."""
    def be32(v):
        return struct.pack(">I", v & 0xFFFFFFFF)

    def s15f16(v):
        return struct.pack(">i", int(round(v * 65536.0)))

    def xyz_tag(xyz):
        return b"XYZ " + b"\0\0\0\0" + b"".join(s15f16(v) for v in xyz)

    def text_tag(s):
        b = s.encode("ascii") + b"\0"
        return b"text" + b"\0\0\0\0" + b

    def desc_tag(s):
        b = s.encode("ascii") + b"\0"
        body = b"desc" + b"\0\0\0\0" + be32(len(b)) + b
        body += b"\0\0\0\0"          # unicode lang + count
        body += b"\0\0\0\0"
        body += b"\0\0" + b"\0" + b"\0" * 67  # scriptcode
        return body

    # mft2 A2B0: 4 in, 3 out, grid points, identity in/out curves (2 entries).
    def mft2():
        t = bytearray()
        t += be32(0x6D667432)          # 'mft2'
        t += b"\0\0\0\0"               # reserved
        t += bytes([4, 3, grid, 0])    # in, out, grid, pad
        # 3x3 s15Fixed16 matrix (ignored for 4-channel input) = identity
        ident = [1, 0, 0, 0, 1, 0, 0, 0, 1]
        for v in ident:
            t += s15f16(v)
        t += struct.pack(">H", 2)      # input table entries
        t += struct.pack(">H", 2)      # output table entries
        # input curves: 4 channels x [0, 65535]
        for _ in range(4):
            t += struct.pack(">HH", 0, 65535)
        # CLUT: grid^4 nodes, 3 outputs (XYZ-D50 u1Fixed15)
        axis = np.linspace(0.0, 1.0, grid)
        scale = 32768.0  # X = v / 32768
        for ic in range(grid):
            for im in range(grid):
                for iy in range(grid):
                    for ik in range(grid):
                        xyz = cmyk_to_xyz_d50(axis[ic], axis[im], axis[iy], axis[ik])
                        for comp in xyz:
                            v = int(round(np.clip(comp, 0.0, 2.0) * scale))
                            t += struct.pack(">H", min(v, 65535))
        # output curves: 3 channels x [0, 65535]
        for _ in range(3):
            t += struct.pack(">HH", 0, 65535)
        return bytes(t)

    desc = desc_tag("Lumen Test CMYK")
    cprt = text_tag("Public Domain test profile")
    wtpt = xyz_tag(D50)
    a2b0 = mft2()

    tags = [(b"desc", desc), (b"cprt", cprt), (b"wtpt", wtpt), (b"A2B0", a2b0)]
    ntags = len(tags)
    header_len = 128
    table_len = 4 + ntags * 12
    body = bytearray()
    offsets = []
    cur = header_len + table_len
    for _, blob in tags:
        # 4-byte align each tag
        pad = (-cur) % 4
        cur += pad
        offsets.append((cur, len(blob)))
        cur += len(blob)
    total = cur

    out = bytearray(b"\0" * total)
    struct.pack_into(">I", out, 0, total)          # profile size
    out[12:16] = b"prtr"                            # device class: output
    out[16:20] = b"CMYK"                            # data colour space
    out[20:24] = b"XYZ "                            # PCS
    out[36:40] = b"acsp"
    struct.pack_into(">i", out, 68, int(round(D50[0] * 65536)))  # PCS illuminant
    struct.pack_into(">i", out, 72, int(round(D50[1] * 65536)))
    struct.pack_into(">i", out, 76, int(round(D50[2] * 65536)))
    struct.pack_into(">I", out, 8, 0x02400000)     # version 2.4
    struct.pack_into(">I", out, 128, ntags)
    off = 132
    blobs_concat = bytearray(out)
    # write tag table + blobs
    for (sig, blob), (toff, tlen) in zip(tags, offsets):
        out[off:off + 4] = sig
        struct.pack_into(">II", out, off + 4, toff, tlen)
        out[toff:toff + tlen] = blob
        off += 12
    return bytes(out)


def make_cmyk_jpg():
    profile = build_cmyk_profile(grid=9)
    # CMYK swatches (each channel 0..1); chosen on grid nodes (k/8) where the
    # CLUT is exact, so the engine output is deterministic.
    swatches = [
        (0.0, 0.0, 0.0, 0.0),    # paper white
        (1.0, 0.0, 0.0, 0.0),    # cyan
        (0.0, 1.0, 0.0, 0.0),    # magenta
        (0.0, 0.0, 1.0, 0.0),    # yellow
        (0.0, 0.0, 0.0, 1.0),    # black (K)
        (0.0, 0.0, 0.0, 0.5),    # mid grey via K
    ]
    W, H = 900, 280
    arr = np.zeros((H, W, 4), dtype=np.uint8)
    cols = np.array_split(np.arange(W), len(swatches))
    for i, (c, m, y, k) in enumerate(swatches):
        # PIL CMYK stores 0=no ink..255=full ink; libjpeg writes Adobe-inverted.
        ink = np.array([c, m, y, k]) * 255.0
        arr[:, cols[i][0]:cols[i][-1] + 1, :] = np.round(ink).astype(np.uint8)
    img = Image.fromarray(arr, "CMYK")
    out = os.path.join(OUT_DIR, "icc_cmyk.jpg")
    img.save(out, "JPEG", quality=100, subsampling=0, icc_profile=profile)

    # expected sRGB for each swatch (engine forward pipeline)
    print("icc_cmyk.jpg expected sRGB (CMYK -> profile -> sRGB):")
    for (c, m, y, k) in swatches:
        xyz50 = cmyk_to_xyz_d50(c, m, y, k)
        xyz65 = D50_TO_D65 @ xyz50
        lin = XYZ_D65_TO_SRGB @ xyz65
        srgb = np.round(srgb_encode(lin) * 255).astype(int)
        print(f"  CMYK({c:.1f},{m:.1f},{y:.1f},{k:.1f}) -> sRGB{tuple(int(x) for x in srgb)}")
    return out


if __name__ == "__main__":
    os.makedirs(OUT_DIR, exist_ok=True)
    p = make_p3_png()
    print("wrote", p)
    q = make_cmyk_jpg()
    print("wrote", q)
