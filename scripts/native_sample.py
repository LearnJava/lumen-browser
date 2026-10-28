"""Сэмплер горячего потока запущенного lumen.exe: native_sample.py <pid> <путь к exe> [число замеров]. RVA кадров -> native_resolve.py <exe> <rva...> (PDB dev-release, только публичные символы: имена приблизительные). Docs: docs/perf-method.md."""
import ctypes, ctypes.wintypes as wt, sys, time, collections, struct
sys.stdout.reconfigure(encoding="utf-8")
k32 = ctypes.WinDLL("kernel32", use_last_error=True)
dbg = ctypes.WinDLL("dbghelp")
pid = int(sys.argv[1]); exe = sys.argv[2]; n = int(sys.argv[3]) if len(sys.argv) > 3 else 40
THREAD_ALL = 0x1F03FF
TH32CS_SNAPTHREAD = 4
class THREADENTRY32(ctypes.Structure):
    _fields_ = [("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("th32ThreadID", wt.DWORD),
                ("th32OwnerProcessID", wt.DWORD), ("tpBasePri", wt.LONG), ("tpDeltaPri", wt.LONG), ("dwFlags", wt.DWORD)]
def threads():
    snap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    e = THREADENTRY32(); e.dwSize = ctypes.sizeof(e)
    out = []
    ok = k32.Thread32First(snap, ctypes.byref(e))
    while ok:
        if e.th32OwnerProcessID == pid: out.append(e.th32ThreadID)
        ok = k32.Thread32Next(snap, ctypes.byref(e))
    k32.CloseHandle(snap)
    return out
class CONTEXT(ctypes.Structure):
    _fields_ = [("P1Home", ctypes.c_uint64)] * 6 + [("ContextFlags", wt.DWORD), ("MxCsr", wt.DWORD)] + \
        [("SegCs", wt.WORD), ("SegDs", wt.WORD), ("SegEs", wt.WORD), ("SegFs", wt.WORD), ("SegGs", wt.WORD), ("SegSs", wt.WORD), ("EFlags", wt.DWORD)] + \
        [("Dr%d" % i, ctypes.c_uint64) for i in (0, 1, 2, 3, 6, 7)] + \
        [(r, ctypes.c_uint64) for r in ("Rax", "Rcx", "Rdx", "Rbx", "Rsp", "Rbp", "Rsi", "Rdi", "R8", "R9", "R10", "R11", "R12", "R13", "R14", "R15", "Rip")] + \
        [("pad", ctypes.c_byte * (976))]
k32.OpenThread.restype = wt.HANDLE
k32.OpenProcess.restype = wt.HANDLE
hp = k32.OpenProcess(0x1F0FFF, False, pid)
dbg.SymSetOptions(0x2 | 0x4 | 0x10)  # undecorated, deferred, load lines
dbg.SymInitialize(hp, None, True)
# find module base of exe
psapi = ctypes.WinDLL("psapi")
mods = (wt.HMODULE * 1024)(); need = wt.DWORD()
psapi.EnumProcessModules(hp, mods, ctypes.sizeof(mods), ctypes.byref(need))
base = mods[0]
dbg.SymLoadModuleExW.restype = ctypes.c_uint64
dbg.SymLoadModuleExW(hp, None, exe, None, ctypes.c_uint64(base), 0, None, 0)
class SYMBOL_INFO(ctypes.Structure):
    _fields_ = [("SizeOfStruct", wt.ULONG), ("TypeIndex", wt.ULONG), ("Reserved", ctypes.c_uint64 * 2), ("Index", wt.ULONG), ("Size", wt.ULONG),
                ("ModBase", ctypes.c_uint64), ("Flags", wt.ULONG), ("Value", ctypes.c_uint64), ("Address", ctypes.c_uint64), ("Register", wt.ULONG),
                ("Scope", wt.ULONG), ("Tag", wt.ULONG), ("NameLen", wt.ULONG), ("MaxNameLen", wt.ULONG), ("Name", ctypes.c_char * 1024)]
def sym(addr):
    s = SYMBOL_INFO(); s.SizeOfStruct = 88; s.MaxNameLen = 1024
    d = ctypes.c_uint64()
    if dbg.SymFromAddr(hp, ctypes.c_uint64(addr), ctypes.byref(d), ctypes.byref(s)):
        return s.Name.decode(errors="replace")
    return "?%x" % (addr - base) if base <= addr < base + 0x10000000 else "?ext"
def cpu(h):
    c, e, kk, u = (wt.FILETIME(), wt.FILETIME(), wt.FILETIME(), wt.FILETIME())
    k32.GetThreadTimes(h, ctypes.byref(c), ctypes.byref(e), ctypes.byref(kk), ctypes.byref(u))
    return ((kk.dwHighDateTime << 32) | kk.dwLowDateTime) + ((u.dwHighDateTime << 32) | u.dwLowDateTime)

class ADDR64(ctypes.Structure):
    _fields_ = [("Offset", ctypes.c_uint64), ("Segment", wt.WORD), ("Mode", ctypes.c_int)]
class STACKFRAME64(ctypes.Structure):
    _fields_ = [("AddrPC", ADDR64), ("AddrReturn", ADDR64), ("AddrFrame", ADDR64), ("AddrStack", ADDR64), ("AddrBStore", ADDR64),
                ("FuncTableEntry", ctypes.c_void_p), ("Params", ctypes.c_uint64 * 4), ("Far", wt.BOOL), ("Virtual", wt.BOOL),
                ("Reserved", ctypes.c_uint64 * 3), ("KdHelp", ctypes.c_byte * 264)]
dbg.SymFunctionTableAccess64.restype = ctypes.c_void_p
dbg.SymGetModuleBase64.restype = ctypes.c_uint64
FTA = ctypes.WINFUNCTYPE(ctypes.c_void_p, wt.HANDLE, ctypes.c_uint64)(("SymFunctionTableAccess64", dbg))
GMB = ctypes.WINFUNCTYPE(ctypes.c_uint64, wt.HANDLE, ctypes.c_uint64)(("SymGetModuleBase64", dbg))
def walk(h, ctx):
    sf = STACKFRAME64()
    sf.AddrPC.Offset = ctx.Rip; sf.AddrPC.Mode = 3
    sf.AddrFrame.Offset = ctx.Rbp; sf.AddrFrame.Mode = 3
    sf.AddrStack.Offset = ctx.Rsp; sf.AddrStack.Mode = 3
    out = []
    for _ in range(40):
        if not dbg.StackWalk64(0x8664, hp, h, ctypes.byref(sf), ctypes.byref(ctx), None, FTA, GMB, None):
            break
        if sf.AddrPC.Offset == 0: break
        out.append(sym(sf.AddrPC.Offset))
    return out
hs = {}
for t in threads():
    h = k32.OpenThread(THREAD_ALL, False, t)
    if h: hs[t] = h
c0 = {t: cpu(h) for t, h in hs.items()}
time.sleep(2)
c1 = {t: cpu(h) - c0[t] for t, h in hs.items()}
hot = sorted(c1, key=c1.get, reverse=True)[:3]
assert ctypes.sizeof(CONTEXT)==1232, ctypes.sizeof(CONTEXT)
print("hot threads (100ns/2s):", [(t, c1[t]) for t in hot])
leaf = collections.Counter(); chains = collections.Counter()
for tid in hot[:1]:
    h = hs[tid]
    for i in range(n):
        k32.SuspendThread(h)
        raw = ctypes.create_string_buffer(1232 + 32); addr = (ctypes.addressof(raw) + 15) & ~15; ctx = CONTEXT.from_address(addr); ctx.ContextFlags = 0x10001F
        ok = k32.GetThreadContext(h, ctypes.byref(ctx))
        if ok:
            rip, rsp = ctx.Rip, ctx.Rsp
            frames = walk(h, ctx)
            leaf[frames[0]] += 1
            chains[" < ".join(f[:60] for f in frames[:14])] += 1
        k32.ResumeThread(h)
        time.sleep(0.05)
print("LEAF"); [print(c, s) for s, c in leaf.most_common(8)]
print("CHAINS"); [print(c, s) for s, c in chains.most_common(5)]
