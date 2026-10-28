"""resolve.py <exe> rva_hex... : resolve RVAs using the exe's PDB through dbghelp (offline)."""
import ctypes, ctypes.wintypes as wt, sys, struct
sys.stdout.reconfigure(encoding="utf-8")
exe = sys.argv[1]
dbg = ctypes.WinDLL("dbghelp")
dbg.SymSetOptions(0x2 | 0x10)  # undecorated, load lines (not deferred)
fake = ctypes.c_void_p(0x1234)
assert dbg.SymInitialize(fake, None, False)
data = open(exe, "rb").read(4096)
pe = struct.unpack_from("<I", data, 0x3C)[0]
size_of_image = struct.unpack_from("<I", data, pe + 24 + 56)[0]
image_base = struct.unpack_from("<Q", data, pe + 24 + 24)[0]
BASE = 0x7ff600000000
dbg.SymLoadModuleExW.restype = ctypes.c_uint64
r = dbg.SymLoadModuleExW(fake, None, exe, None, ctypes.c_uint64(BASE), size_of_image, None, 0)
print("load", hex(r), "size", hex(size_of_image), "pref base", hex(image_base), file=sys.stderr)
class SYMBOL_INFO(ctypes.Structure):
    _fields_ = [("SizeOfStruct", wt.ULONG), ("TypeIndex", wt.ULONG), ("Reserved", ctypes.c_uint64 * 2), ("Index", wt.ULONG), ("Size", wt.ULONG),
                ("ModBase", ctypes.c_uint64), ("Flags", wt.ULONG), ("Value", ctypes.c_uint64), ("Address", ctypes.c_uint64), ("Register", wt.ULONG),
                ("Scope", wt.ULONG), ("Tag", wt.ULONG), ("NameLen", wt.ULONG), ("MaxNameLen", wt.ULONG), ("Name", ctypes.c_char * 2048)]
def sym(rva):
    s = SYMBOL_INFO(); s.SizeOfStruct = 88; s.MaxNameLen = 2048
    d = ctypes.c_uint64()
    if dbg.SymFromAddr(fake, ctypes.c_uint64(BASE + rva), ctypes.byref(d), ctypes.byref(s)):
        return s.Name.decode(errors="replace") + "+0x%x" % d.value
    return "?%x" % rva
for a in sys.argv[2:]:
    print(a, sym(int(a, 16)))
