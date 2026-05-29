import threading
import webview
import uvicorn
from main import app, manager
import json

class JSApi:
    def __init__(self):
        self.main_window = None
        self.float_window = None
    
    def show_main(self):
        if self.main_window:
            self.main_window.show()
            self.main_window.restore()
    
    def hide_main(self):
        if self.main_window:
            self.main_window.hide()
    
    def drag_window(self, dx, dy):
        # pywebview 4.0+ 支持移动窗口
        if self.float_window:
            x, y = self.float_window.x, self.float_window.y
            self.float_window.move(x + dx, y + dy)

def start_server():
    uvicorn.run(app, host="0.0.0.0", port=8000, log_level="warning")

if __name__ == "__main__":
    # 1. 启动 FastAPI 服务（后台线程）
    server_thread = threading.Thread(target=start_server, daemon=True)
    server_thread.start()
    
    api = JSApi()
    
    # 2. 创建主窗口
    api.main_window = webview.create_window(
        "AI Agent 主控台",
        "http://localhost:8000/main_window.html",
        width=1200,
        height=800,
        min_size=(800, 600),
        text_select=True
    )
    
    # 3. 创建悬浮窗（透明、无边框、置顶）
    api.float_window = webview.create_window(
        "Agent Float",
        "http://localhost:8000/float.html",
        width=160,
        height=200,
        x=webview.screens[0].width - 180,
        y=webview.screens[0].height - 220,
        frameless=True,
        on_top=True,
        transparent=True,
        resizable=False,
        js_api=api
    )
    
    # 4. 启动 GUI
    webview.start(gui='edgechromium', debug=False)