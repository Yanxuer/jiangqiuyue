import mss
import io
import base64
from PIL import Image

def capture_screen(monitor_index: int = 1) -> str:
    """
    截图并返回 base64
    monitor_index: 1=主屏, 2=副屏, 0=所有屏幕
    """
    with mss.mss() as sct:
        if monitor_index == 0:
            mon = sct.monitors[0]
        else:
            mon = sct.monitors[monitor_index] if monitor_index < len(sct.monitors) else sct.monitors[1]
        
        screenshot = sct.grab(mon)
        img = Image.frombytes("RGB", screenshot.size, screenshot.bgra, "raw", "BGRX")
        
        buffered = io.BytesIO()
        img.save(buffered, format="PNG")
        return base64.b64encode(buffered.getvalue()).decode()