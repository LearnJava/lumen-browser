import ctypes, ctypes.wintypes as W
u = ctypes.WinDLL("user32")
WNDENUMPROC = ctypes.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
def windows_of(pid):
    out = []
    def cb(h, _):
        p = W.DWORD(); u.GetWindowThreadProcessId(h, ctypes.byref(p))
        if p.value == pid: out.append(h)
        return True
    u.EnumWindows(WNDENUMPROC(cb), 0)
    return out
_n = [0]
def poke(pid):
    _n[0] += 1
    ws = windows_of(pid)
    for h in ws:
        r = W.RECT(); u.GetWindowRect(h, ctypes.byref(r))
        u.ShowWindow(h, 8); u.SetWindowPos(h, None, 0, 0, 1040 + (_n[0] % 2), 800, 0x14)
        u.InvalidateRect(h, None, True)
        u.PostMessageW(h, 0x000F, 0, 0)  # WM_PAINT
        u.PostMessageW(h, 0x0000, 0, 0)
    return len(ws), [(r.left, r.top, r.right, r.bottom)] if ws else None
