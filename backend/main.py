import asyncio
import json
from typing import List
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel
from core.agent import agent, client, MODEL, pending_cli_commands
from core.config import DEEPSEEK_API_KEY
from core.memory import memory
from core.screen import capture_screen
from core.cli_executor import execute_command
from core.software_scanner import scan_and_cache, is_software_scanned, get_all_categories, get_all_software, search_software as scanner_search_software, get_software_by_category

app = FastAPI()

software_scan_complete = False

@app.on_event("startup")
async def startup():
    asyncio.create_task(background_software_scan())

async def background_software_scan():
    global software_scan_complete
    print("[启动] 开始后台静默扫描电脑软件...")
    try:
        software_list = scan_and_cache()
        if software_list:
            software_scan_complete = True
            print(f"[启动] 软件扫描完成，已缓存 {len(software_list)} 个软件")
            cats = get_all_categories()
            print(f"[启动] 覆盖分类: {', '.join(sorted(cats.keys()))}")
    except Exception as e:
        print(f"[启动] 软件扫描失败(不影响主功能): {e}")
    finally:
        software_scan_complete = True

# CORS — 开发环境允许 localhost 前端访问
app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "http://localhost:5173",
        "http://127.0.0.1:5173",
        "http://localhost:8000",
        "http://127.0.0.1:8000",
        "file://",
    ],
    allow_methods=["*"],
    allow_headers=["*"],
)

# WebSocket 连接管理（用于主窗口和悬浮窗通信）
class ConnectionManager:
    def __init__(self):
        self.active: List[WebSocket] = []
    
    async def connect(self, ws: WebSocket):
        await ws.accept()
        self.active.append(ws)
    
    def disconnect(self, ws: WebSocket):
        if ws in self.active:
            self.active.remove(ws)
    
    async def broadcast(self, message: dict):
        dead = []
        for ws in self.active:
            try:
                await ws.send_json(message)
            except:
                dead.append(ws)
        for d in dead:
            self.disconnect(d)

manager = ConnectionManager()

# 数据模型
class ChatRequest(BaseModel):
    message: str
    use_screen: bool = False

class FileRequest(BaseModel):
    path: str
    content: str = None

# API 路由
@app.get("/health")
async def health_check():
    return {"status": "ok"}

@app.post("/chat")
async def chat(req: ChatRequest):
    """主对话接口"""
    img_b64 = None
    if req.use_screen:
        img_b64 = capture_screen()
    
    result = agent.run(req.message, img_b64)
    
    return result

@app.post("/file/read")
async def api_read_file(req: FileRequest):
    from core.file_tools import read_file
    return {"content": read_file(req.path)}

@app.post("/file/write")
async def api_write_file(req: FileRequest):
    from core.file_tools import write_file
    return {"result": write_file(req.path, req.content)}

@app.get("/files")
async def api_list_files(dir: str = ""):
    from core.file_tools import list_files
    return {"files": list_files(dir)}

@app.post("/memory/search")
async def api_memory_search(query: str):
    return {"results": memory.search(query)}

@app.post("/memory/add")
async def api_memory_add(content: str):
    return {"id": memory.add(content)}

# CLI 安全执行端点
@app.get("/cli/pending")
async def get_pending_commands():
    commands = []
    for cmd_id, data in pending_cli_commands.items():
        commands.append({
            "command_id": cmd_id,
            "command": data["command"],
            "reason": data["reason"],
            "operation_type": data["analysis"]["operation_type"],
            "affected_files": data["analysis"]["affected_files"]
        })
    return {"pending": commands}

@app.post("/cli/confirm/{command_id}")
async def confirm_cli(command_id: str):
    cmd_data = pending_cli_commands.pop(command_id, None)
    if not cmd_data:
        return {"success": False, "error": "命令不存在或已过期"}
    result = execute_command(cmd_data["command"], cmd_data.get("cwd"))
    return {"success": True, "result": result}

@app.post("/cli/reject/{command_id}")
async def reject_cli(command_id: str):
    cmd_data = pending_cli_commands.pop(command_id, None)
    if not cmd_data:
        return {"success": False, "error": "命令不存在或已过期"}
    return {"success": True, "result": "操作已取消"}

# 软件扫描端点
@app.get("/software/status")
async def software_scan_status():
    scanned = is_software_scanned()
    return {
        "scanned": software_scan_complete,
        "has_software": scanned,
        "categories": get_all_categories() if scanned else {},
    }

@app.get("/software/list")
async def list_software(category: str = ""):
    if not is_software_scanned():
        return {"software": [], "scanned": software_scan_complete}
    if category:
        sw_list = get_software_by_category(category)
        return {"software": [{"name": sw.name, "category": sw.category} for sw in sw_list]}
    cats = get_all_categories()
    result = []
    for cat, names in cats.items():
        for name in names:
            result.append({"name": name, "category": cat})
    return {"software": result, "scanned": software_scan_complete}

@app.get("/software/search")
async def search_software_api(query: str):
    if not is_software_scanned():
        return {"software": [], "scanned": software_scan_complete}
    software_list = get_all_software()
    results = scanner_search_software(query, software_list)
    return {"software": [{"name": sw.name, "path": sw.exec_path, "category": sw.category, "description": sw.description, "score": 1.0} for sw in results], "scanned": software_scan_complete}

# 文档读取端点
from core.doc_reader import list_directory, read_text_file, add_recent_path, get_recent_paths

@app.get("/docs/list")
async def api_list_directory(path: str):
    result = list_directory(path)
    return result

@app.get("/docs/read")
async def api_read_file(path: str):
    result = read_text_file(path)
    return result

@app.post("/docs/select-path")
async def api_select_path(req: FileRequest):
    path = req.path
    add_recent_path(path)
    recent = get_recent_paths()
    dir_info = list_directory(path)
    return {"recent_paths": recent, "directory": dir_info}

@app.get("/docs/recent-paths")
async def api_recent_paths():
    return {"recent_paths": get_recent_paths()}

@app.post("/docs/delete-path")
async def api_delete_path(req: FileRequest):
    from core.doc_reader import load_recent_paths, save_recent_paths
    paths = load_recent_paths()
    target = os.path.abspath(req.path)
    paths = [p for p in paths if os.path.abspath(p.get("path", "")) != target]
    save_recent_paths(paths)
    return {"recent_paths": paths}

VALID_ACTIONS = {"capture_and_ask", "show_window", "hide_window"}

@app.websocket("/ws")
async def websocket_endpoint(ws: WebSocket):
    await manager.connect(ws)
    try:
        while True:
            data = await ws.receive_text()
            if not data or len(data) > 65536:
                await ws.send_json({"type": "error", "message": "消息过长或为空"})
                continue
            msg = json.loads(data)
            if not isinstance(msg, dict):
                await ws.send_json({"type": "error", "message": "无效的消息格式"})
                continue
            
            action = msg.get("action")
            if action not in VALID_ACTIONS:
                await ws.send_json({"type": "error", "message": f"未知 action: {action}"})
                continue
            
            # 处理来自悬浮窗的命令
            if action == "capture_and_ask":
                img_b64 = capture_screen()
                # 限制问题长度防注入
                question = str(msg.get("question", "描述一下我屏幕上显示的内容"))[:500]
                result = agent.run(question, img_b64)
                await ws.send_json({
                    "type": "screen_result",
                    "data": result
                })
            elif action == "show_window":
                await manager.broadcast({"type": "command", "action": "show_main"})
            elif action == "hide_window":
                await manager.broadcast({"type": "command", "action": "hide_main"})
                
    except WebSocketDisconnect:
        manager.disconnect(ws)

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=8000)