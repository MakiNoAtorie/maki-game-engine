"""A picture of the main window of a running program, with no focus and no input:
the owner can go on with their work. Windows only (PrintWindow).

    python tools/window_shot.py maki-editor shot.png

The first argument is the process name, without `.exe`. The window can be behind
others; it must not be minimized. Needs Pillow.
"""

import ctypes
import ctypes.wintypes as wt
import subprocess
import sys

from PIL import Image

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32
# Also the content that a GPU swap chain draws (Vulkan, egui).
PW_RENDERFULLCONTENT = 2


def process_ids(name: str) -> set[int]:
    out = subprocess.run(
        ["tasklist", "/FI", f"IMAGENAME eq {name}.exe", "/FO", "CSV", "/NH"],
        capture_output=True, text=True,
    ).stdout
    return {int(line.split('","')[1]) for line in out.splitlines() if line.startswith('"')}


def main_window(pids: set[int]) -> int | None:
    """The largest visible top-level window of the processes."""
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def visit(hwnd, _):
        pid = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pid.value in pids and user32.IsWindowVisible(hwnd):
            rect = wt.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(rect))
            found.append(((rect.right - rect.left) * (rect.bottom - rect.top), hwnd))
        return True

    user32.EnumWindows(visit, 0)
    return max(found)[1] if found else None


def capture(hwnd: int) -> Image.Image:
    rect = wt.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    width, height = rect.right, rect.bottom
    window_dc = user32.GetDC(hwnd)
    memory_dc = gdi32.CreateCompatibleDC(window_dc)
    bitmap = gdi32.CreateCompatibleBitmap(window_dc, width, height)
    gdi32.SelectObject(memory_dc, bitmap)
    # 1: the client area only.
    user32.PrintWindow(hwnd, memory_dc, PW_RENDERFULLCONTENT | 1)
    buffer = ctypes.create_string_buffer(width * height * 4)
    gdi32.GetBitmapBits(bitmap, len(buffer), buffer)
    gdi32.DeleteObject(bitmap)
    gdi32.DeleteDC(memory_dc)
    user32.ReleaseDC(hwnd, window_dc)
    return Image.frombuffer("RGB", (width, height), buffer, "raw", "BGRX", 0, 1)


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    # The size in pixels of the screen, not of a scaled display.
    ctypes.windll.shcore.SetProcessDpiAwareness(2)
    hwnd = main_window(process_ids(sys.argv[1]))
    if hwnd is None:
        print(f"no visible window of {sys.argv[1]}.exe")
        return 1
    if user32.IsIconic(hwnd):
        print("the window is minimized: PrintWindow gives no picture")
        return 1
    image = capture(hwnd)
    image.save(sys.argv[2])
    print(f"{sys.argv[2]}: {image.width} x {image.height}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
