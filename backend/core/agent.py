import json
import base64
import uuid
from openai import OpenAI
from core.config import DEEPSEEK_API_KEY, DEEPSEEK_BASE_URL, MODEL
from core.file_tools import read_file, write_file, list_files
from core.screen import capture_screen
from core.memory import memory
from core.cli_executor import analyze_command, execute_command
from core.software_scanner import is_software_scanned, get_all_software, search_software as scanner_search_software, launch_software

client = OpenAI(api_key=DEEPSEEK_API_KEY, base_url=DEEPSEEK_BASE_URL)

pending_cli_commands = {}

# 工具定义
TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "capture_screen",
            "description": "截取用户屏幕并分析当前显示内容",
            "parameters": {
                "type": "object",
                "properties": {
                    "monitor": {"type": "integer", "description": "屏幕编号，1为主屏", "default": 1}
                },
                "required": []
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "读取工作区内的文件内容",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "文件相对路径，如 src/main.py"}
                },
                "required": ["path"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "write_file",
            "description": "创建或覆盖文件",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "list_files",
            "description": "列出工作区文件",
            "parameters": {
                "type": "object",
                "properties": {
                    "dir": {"type": "string", "description": "子目录，默认为根目录"}
                }
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "search_memory",
            "description": "从长期记忆中搜索相关信息",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "搜索关键词或问题"}
                },
                "required": ["query"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "add_memory",
            "description": "将重要信息保存到长期记忆",
            "parameters": {
                "type": "object",
                "properties": {
                    "content": {"type": "string"}
                },
                "required": ["content"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "execute_command",
            "description": "在电脑上执行命令行操作，如创建文件、安装依赖、运行脚本、查看目录等。注意：危险命令(rm -rf, sudo, del /f, format等)会被自动拦截。操作需要用户确认后才执行。",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "要执行的命令行命令"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "工作目录，默认为项目根目录",
                        "default": None
                    },
                    "reason": {
                        "type": "string",
                        "description": "说明为什么要执行这个命令，涉及哪些文件",
                        "default": ""
                    }
                },
                "required": ["command", "reason"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "find_software",
            "description": "在电脑上搜索已安装的软件。支持通过关键词搜索，如搜索'browser'找浏览器、'editor'找编辑器、'image'找图片处理软件等。结果包含软件名称、路径、分类。",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词，如软件名、分类名(browser/editor/image/video/office等)"
                    }
                },
                "required": ["query"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "launch_software",
            "description": "启动电脑上已安装的软件。必须先调用 find_software 确认软件存在且获取路径后，再调用此函数。",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "要启动的软件名称"
                    },
                    "path": {
                        "type": "string",
                        "description": "软件执行路径，从 find_software 的结果中获取"
                    }
                },
                "required": ["name", "path"]
            }
        }
    }
]

class Agent:
    def __init__(self):
        self.messages = [
            {"role": "system", "content": """你是江秋月，一个桌面AI助手，性格温柔可爱，对用户用"你"称呼。
你可以直接操作用户的文件、屏幕和电脑命令行，帮助用户完成各种任务。

能力：
- 当用户要求查看屏幕时，调用 capture_screen
- 当用户提到"记住"或"保存"某事时，调用 add_memory
- 当用户问起之前的对话时，调用 search_memory
- 当需要执行命令行操作（查看目录、安装依赖、运行脚本、创建文件、查看进程等）时，调用 execute_command
  * 调用 execute_command 时，必须用 reason 参数说明为什么要执行此命令、涉及哪些文件路径
  * 危险命令会被自动拦截，不用担心误操作
  * 操作需要用户确认后才能执行，所以先向用户说明你要做什么
- 当用户需要使用某个软件时（如打开浏览器、编辑图片、播放视频等），先调用 find_software 搜索已安装的软件，找到后调用 launch_software 启动它
- 电脑上的软件列表已经在后台扫描并记忆了，可以直接搜索

回答风格：
- 用"~"结尾让语气更亲切
- 代码使用markdown格式
- 回答简洁但温暖"""}
        ]
    
    def run(self, user_input: str, image_base64: str = None) -> dict:
        # 构建用户消息
        if image_base64:
            content = [
                {"type": "text", "text": user_input},
                {"type": "image_url", "image_url": {"url": f"data:image/png;base64,{image_base64}"}}
            ]
        else:
            content = user_input
            
        self.messages.append({"role": "user", "content": content})
        
        # 调用 API（流式/非流式均可，这里用非流式演示）
        response = client.chat.completions.create(
            model=MODEL,
            messages=self.messages,
            tools=TOOLS,
            tool_choice="auto",
            temperature=0.7
        )
        
        msg = response.choices[0].message
        self.messages.append(msg.to_dict() if hasattr(msg, 'to_dict') else {"role": msg.role, "content": msg.content})
        
        # 处理工具调用
        if msg.tool_calls:
            tool_results = []
            for tc in msg.tool_calls:
                result = self._execute_tool(tc.function.name, json.loads(tc.function.arguments))
                tool_results.append({
                    "tool_call_id": tc.id,
                    "role": "tool",
                    "name": tc.function.name,
                    "content": json.dumps(result, ensure_ascii=False)
                })
            
            # 将工具结果加入对话
            self.messages.extend(tool_results)
            
            # 再次调用获取最终回答
            final_resp = client.chat.completions.create(
                model=MODEL,
                messages=self.messages,
                temperature=0.7
            )
            final_msg = final_resp.choices[0].message
            self.messages.append(final_msg.to_dict() if hasattr(final_msg, 'to_dict') else {"role": final_msg.role, "content": final_msg.content})
            return {
                "reply": final_msg.content,
                "tool_calls": [tc.function.name for tc in msg.tool_calls]
            }
        
        return {"reply": msg.content, "tool_calls": []}
    
    def _execute_tool(self, name: str, args: dict):
        if name == "capture_screen":
            b64 = capture_screen(args.get("monitor", 1))
            return {"success": True, "image_base64": b64, "note": "已截图"}
        elif name == "read_file":
            return {"content": read_file(args["path"])}
        elif name == "write_file":
            return {"result": write_file(args["path"], args["content"])}
        elif name == "list_files":
            return {"files": list_files(args.get("dir", ""))}
        elif name == "search_memory":
            return {"memories": memory.search(args["query"])}
        elif name == "add_memory":
            mid = memory.add(args["content"])
            return {"memory_id": mid}
        elif name == "execute_command":
            cmd = args["command"]
            reason = args.get("reason", "")
            analysis = analyze_command(cmd)
            if not analysis.safe:
                return {
                    "status": "blocked",
                    "error": f"[!] 危险命令已被拦截: {analysis.reason}",
                    "command": cmd
                }
            cmd_id = str(uuid.uuid4())[:8]
            pending_cli_commands[cmd_id] = {
                "command": cmd,
                "cwd": args.get("cwd"),
                "reason": reason,
                "analysis": {
                    "operation_type": analysis.operation_type,
                    "affected_files": analysis.affected_files
                }
            }
            return {
                "status": "confirmation_required",
                "command_id": cmd_id,
                "command": cmd,
                "operation_type": analysis.operation_type,
                "affected_files": analysis.affected_files,
                "reason": reason,
                "message": f"需要你确认是否执行此操作~"
            }
        elif name == "find_software":
            query = args["query"]
            if not is_software_scanned():
                return {"status": "scanning", "message": "正在扫描电脑上的软件，请稍后重试~"}
            software_list = get_all_software()
            results = scanner_search_software(query, software_list)
            return {"software": [{"name": sw.name, "path": sw.exec_path, "category": sw.category, "description": sw.description, "score": 1.0} for sw in results]}
        elif name == "launch_software":
            sw_name = args["name"]
            sw_path = args["path"]
            result = launch_software(sw_path)
            return result
        return {"error": "未知工具"}

agent = Agent()