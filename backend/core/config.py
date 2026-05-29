import os

# 尝试从 .env 文件加载（轻量，不依赖 python-dotenv）
# 查找顺序: 当前文件目录 → backend/ → 项目根目录
_env_paths = [
    os.path.join(os.path.dirname(os.path.abspath(__file__)), '.env'),
    os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), '.env'),
    os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), '.env'),
]
for _env_file in _env_paths:
    if os.path.exists(_env_file):
        try:
            with open(_env_file, encoding='utf-8') as f:
                for line in f:
                    line = line.strip()
                    if line and not line.startswith('#') and '=' in line:
                        key, val = line.split('=', 1)
                        key = key.strip()
                        val = val.strip().strip('"').strip("'")
                        if key == 'DEEPSEEK_API_KEY':
                            os.environ.setdefault(key, val)
        except Exception:
            pass
        break

# >>> 请通过环境变量或 .env 文件设置 DEEPSEEK_API_KEY <<<
# 从 https://platform.deepseek.com/api_keys 获取
DEEPSEEK_API_KEY = os.getenv("DEEPSEEK_API_KEY")
if not DEEPSEEK_API_KEY:
    raise RuntimeError(
        "未设置 DEEPSEEK_API_KEY 环境变量\n"
        "   方式1: 在项目根目录创建 .env 文件，写入: DEEPSEEK_API_KEY=sk-xxx\n"
        "   方式2: 通过 `set DEEPSEEK_API_KEY=sk-xxx` 设置环境变量\n"
        "   方式3: 通过系统环境变量面板永久设置"
    )

DEEPSEEK_BASE_URL = "https://api.deepseek.com"
MODEL = "deepseek-chat"
WORKSPACE = os.path.abspath("./workspace")
MEMORY_PATH = "./memory_db"

os.makedirs(WORKSPACE, exist_ok=True)