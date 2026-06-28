# ============================================================
#  江秋月 Agent CLI 启动脚本
#  功能: 环境预检 → 沙箱隔离 → 编译 → 启动 → 模拟任务
# ============================================================

param(
    [switch]$Test,           # 启动后自动运行模拟任务
    [switch]$BuildOnly,      # 仅编译不启动
    [switch]$NoSandbox,      # 跳过沙箱检查
    [string]$Port = "8000",  # 服务端口
    [string]$LogLevel = "info" # 日志级别: trace|debug|info|warn|error
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$BackendDir = Join-Path $ScriptDir "backend-rust\backend-server"
$CoreDir = Join-Path $ScriptDir "backend-rust\backend-core"
$SandboxRoot = Join-Path $ScriptDir "sandbox_workspace"
$LogFile = Join-Path $ScriptDir "agent_runtime.log"
$Timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"

# ============================================================
# 颜色输出辅助函数
# ============================================================
function Write-Header { Write-Host "`n$('='*60)" -ForegroundColor Cyan }
function Write-Step { param($msg) Write-Host "  [>] $msg" -ForegroundColor Yellow }
function Write-Pass { param($msg) Write-Host "  [✓] $msg" -ForegroundColor Green }
function Write-Fail { param($msg) Write-Host "  [✗] $msg" -ForegroundColor Red }
function Write-Warn { param($msg) Write-Host "  [!] $msg" -ForegroundColor Magenta }
function Write-Info { param($msg) Write-Host "  [i] $msg" -ForegroundColor Gray }
function Write-KeyVal { param($k, $v) Write-Host "  $k" -NoNewline -ForegroundColor DarkCyan; Write-Host ": $v" -ForegroundColor White }

# ============================================================
# 阶段 0: 环境预检
# ============================================================
Write-Header
Write-Host "  江秋月 Agent CLI 启动脚本" -ForegroundColor Cyan
Write-Host "  启动时间: $Timestamp" -ForegroundColor Gray
Write-Header

Write-Host "`n[阶段 0] 环境预检" -ForegroundColor Cyan
Write-Host "──────────────────────────────────────────────────" -ForegroundColor DarkGray

# 0.1 操作系统
Write-Step "操作系统检查"
$os = Get-CimInstance Win32_OperatingSystem
Write-KeyVal "  系统" "$($os.Caption)"
Write-KeyVal "  架构" "$env:PROCESSOR_ARCHITECTURE"
Write-KeyVal "  内存" "$([math]::Round($os.TotalVisibleMemorySize/1MB,1)) GB"
Write-Pass "操作系统兼容"

# 0.2 Rust 工具链
Write-Step "Rust 工具链检查"
$cargoPath = "$env:USERPROFILE\.cargo\bin\cargo.exe"
$rustupPath = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin"
if (Test-Path $cargoPath) {
    $env:PATH = "$rustupPath;$env:USERPROFILE\.cargo\bin;$env:PATH"
    $cargoVer = & cargo --version 2>$null
    if ($LASTEXITCODE -eq 0) {
        Write-Pass "Cargo: $cargoVer"
    } else {
        Write-Fail "Cargo 不可用"
    }
} else {
    Write-Fail "未找到 Cargo ($cargoPath)"
}

$rustcVer = & rustc --version 2>$null
if ($LASTEXITCODE -eq 0) {
    Write-Pass "Rustc: $rustcVer"
} else {
    Write-Fail "Rustc 不可用"
}

# 0.3 项目文件完整性
Write-Step "项目结构检查"
$requiredFiles = @(
    "$BackendDir\Cargo.toml",
    "$CoreDir\Cargo.toml",
    "$CoreDir\src\agent.rs",
    "$CoreDir\src\cli_executor.rs",
    "$CoreDir\src\config.rs",
    "$CoreDir\src\file_tools.rs",
    "$CoreDir\src\memory.rs"
)
$allPresent = $true
foreach ($f in $requiredFiles) {
    if (Test-Path $f) {
        Write-Pass "存在: $(Split-Path $f -Leaf)"
    } else {
        Write-Fail "缺失: $f"
        $allPresent = $false
    }
}
if (-not $allPresent) {
    Write-Fail "项目文件不完整，请检查"
    exit 1
}

# 0.4 端口占用检查
Write-Step "端口 $Port 占用检查"
$portInUse = netstat -ano | Select-String ":$Port " | Select-String "LISTENING"
if ($portInUse) {
    Write-Warn "端口 $Port 已被占用:"
    Write-Host $portInUse -ForegroundColor Gray
    $choice = Read-Host "  是否终止占用进程? (y/n)"
    if ($choice -eq 'y') {
        $pidMatch = [regex]::Match($portInUse, '\s+(\d+)$')
        if ($pidMatch.Success) {
            $occupyPid = $pidMatch.Groups[1].Value
            Stop-Process -Id $occupyPid -Force -ErrorAction SilentlyContinue
            Write-Pass "已终止 PID $occupyPid"
            Start-Sleep -Seconds 1
        }
    } else {
        Write-Warn "端口冲突，可能无法启动"
    }
} else {
    Write-Pass "端口 $Port 空闲"
}

# 0.5 LLM API 配置检查
Write-Step "LLM API 配置检查"
$apiKey = $env:LLM_API_KEY
if (-not $apiKey) { $apiKey = $env:DEEPSEEK_API_KEY }
$provider = $env:LLM_PROVIDER
if (-not $provider) { $provider = "deepseek" }
$baseUrl = $env:LLM_BASE_URL
if (-not $baseUrl) { $baseUrl = $env:DEEPSEEK_BASE_URL }
$model = $env:LLM_MODEL
if (-not $model) { $model = $env:MODEL }
if ($apiKey) {
    $maskedKey = $apiKey.Substring(0, [Math]::Min(12, $apiKey.Length)) + "..."
    Write-Pass "LLM_API_KEY: 已设置 (${maskedKey})"
    Write-KeyVal "  LLM_PROVIDER" "$provider"
    Write-KeyVal "  LLM_BASE_URL" "$baseUrl"
    Write-KeyVal "  LLM_MODEL" "$model"
} else {
    Write-Warn "LLM_API_KEY 或 DEEPSEEK_API_KEY 未设置"
    Write-Info "Agent 将使用运行时配置，请在启动后通过 /config API 设置"
}

# 0.6 磁盘空间
Write-Step "磁盘空间检查"
$drive = Get-PSDrive -Name (Split-Path $ScriptDir -Qualifier).TrimEnd(':')
$freeGB = [math]::Round($drive.Free/1GB, 1)
Write-KeyVal "  可用空间" "$freeGB GB"
if ($freeGB -lt 1) {
    Write-Warn "磁盘空间不足 1GB，可能影响编译"
}

# ============================================================
# 阶段 1: 沙箱隔离
# ============================================================
if (-not $NoSandbox) {
    Write-Host "`n[阶段 1] 沙箱隔离" -ForegroundColor Cyan
    Write-Host "──────────────────────────────────────────────────" -ForegroundColor DarkGray

    Write-Step "创建沙箱工作目录"
    if (-not (Test-Path $SandboxRoot)) {
        New-Item -ItemType Directory -Path $SandboxRoot -Force | Out-Null
        Write-Pass "已创建: $SandboxRoot"
    } else {
        Write-Pass "已存在: $SandboxRoot"
    }

    # 设置环境变量限制工作目录
    $env:AGENT_SANDBOX_ROOT = $SandboxRoot
    $env:AGENT_WORKSPACE = $SandboxRoot
    Write-Pass "环境变量已设置: AGENT_SANDBOX_ROOT=$SandboxRoot"

    Write-Step "沙箱安全边界"
    Write-KeyVal "  允许写入" "$SandboxRoot"
    Write-KeyVal "  禁止访问" "C:\Windows, C:\Program Files, 系统目录"
    Write-KeyVal "  禁止命令" "rm -rf /, format, del /f /s 系统目录"
    Write-Info "沙箱模式: 文件操作限制在 $SandboxRoot 内"
} else {
    Write-Host "`n[阶段 1] 沙箱隔离 (已跳过)" -ForegroundColor DarkGray
}

# ============================================================
# 阶段 2: 编译
# ============================================================
Write-Host "`n[阶段 2] 编译构建" -ForegroundColor Cyan
Write-Host "──────────────────────────────────────────────────" -ForegroundColor DarkGray

Write-Step "编译 backend-core (Release)"
Push-Location $CoreDir
$env:RUST_LOG = $LogLevel
$buildStart = Get-Date
$buildOutput = cargo build --release 2>&1
$buildExit = $LASTEXITCODE
$buildElapsed = (Get-Date) - $buildStart
Pop-Location

if ($buildExit -eq 0) {
    Write-Pass "编译成功 (耗时: $($buildElapsed.TotalSeconds.ToString('0.0'))s)"
} else {
    Write-Fail "编译失败 (exit code: $buildExit)"
    Write-Host "`n编译输出:" -ForegroundColor Red
    Write-Host $buildOutput -ForegroundColor Red
    exit 1
}

Write-Step "编译 backend-server (Release)"
Push-Location $BackendDir
$buildStart = Get-Date
$buildOutput = cargo build --release 2>&1
$buildExit = $LASTEXITCODE
$buildElapsed = (Get-Date) - $buildStart
Pop-Location

if ($buildExit -eq 0) {
    Write-Pass "编译成功 (耗时: $($buildElapsed.TotalSeconds.ToString('0.0'))s)"
} else {
    Write-Fail "编译失败 (exit code: $buildExit)"
    Write-Host "`n编译输出:" -ForegroundColor Red
    Write-Host $buildOutput -ForegroundColor Red
    exit 1
}

# 验证二进制文件
$binary = "$BackendDir\target\release\backend-server.exe"
if (Test-Path $binary) {
    $binSize = [math]::Round((Get-Item $binary).Length/1MB, 1)
    Write-Pass "二进制文件: backend-server.exe ($binSize MB)"
} else {
    Write-Fail "未找到二进制文件"
    exit 1
}

if ($BuildOnly) {
    Write-Host "`n[完成] 仅编译模式，跳过启动" -ForegroundColor Green
    exit 0
}

# ============================================================
# 阶段 3: 启动服务
# ============================================================
Write-Host "`n[阶段 3] 启动 Agent 服务" -ForegroundColor Cyan
Write-Host "──────────────────────────────────────────────────" -ForegroundColor DarkGray

# 设置运行时环境变量
$env:RUST_LOG = $LogLevel
$env:MEMORY_PATH = "$ScriptDir\memory_data"
$env:WORKSPACE = if ($NoSandbox) { "$ScriptDir\workspace" } else { $SandboxRoot }

# 确保目录存在
if (-not (Test-Path $env:MEMORY_PATH)) {
    New-Item -ItemType Directory -Path $env:MEMORY_PATH -Force | Out-Null
}
if (-not (Test-Path $env:WORKSPACE)) {
    New-Item -ItemType Directory -Path $env:WORKSPACE -Force | Out-Null
}

Write-Step "启动配置"
Write-KeyVal "  监听地址" "http://127.0.0.1:$Port"
Write-KeyVal "  日志级别" "$LogLevel"
Write-KeyVal "  工作目录" "$env:WORKSPACE"
Write-KeyVal "  记忆目录" "$env:MEMORY_PATH"
Write-KeyVal "  日志文件" "$LogFile"

Write-Step "启动后端服务..."
$proc = Start-Process -FilePath $binary -NoNewWindow -PassThru -RedirectStandardOutput $LogFile -RedirectStandardError $LogFile

Write-Pass "进程已启动 (PID: $($proc.Id))"

# 等待服务就绪
Write-Step "等待服务就绪..."
$maxWait = 30
$ready = $false
for ($i = 1; $i -le $maxWait; $i++) {
    Start-Sleep -Seconds 1
    try {
        $resp = Invoke-WebRequest -Uri "http://127.0.0.1:$Port/health" -TimeoutSec 2 -ErrorAction SilentlyContinue
        if ($resp.StatusCode -eq 200) {
            Write-Pass "服务就绪 (耗时: ${i}s)"
            $ready = $true
            break
        }
    } catch {
        Write-Info "等待中... ($i/$maxWait)"
    }
}

if (-not $ready) {
    Write-Fail "服务启动超时 ($maxWait 秒)"
    Write-Host "`n最近的日志输出:" -ForegroundColor Yellow
    if (Test-Path $LogFile) {
        Get-Content $LogFile -Tail 20 | ForEach-Object { Write-Host "  $_" -ForegroundColor Gray }
    }
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    exit 1
}

# ============================================================
# 阶段 4: 模拟任务测试 (可选)
# ============================================================
if ($Test) {
    Write-Host "`n[阶段 4] 模拟任务测试" -ForegroundColor Cyan
    Write-Host "──────────────────────────────────────────────────" -ForegroundColor DarkGray

    # 创建测试工作区
    $testDir = Join-Path $env:WORKSPACE "test_project"
    if (-not (Test-Path $testDir)) {
        New-Item -ItemType Directory -Path $testDir -Force | Out-Null
    }

    # 任务 1: 创建并修改文件
    Write-Host "`n  ── 测试 1: 创建文件 + 修改文件 ──" -ForegroundColor Yellow

    $testFile = "$testDir\hello.py"
    $initialCode = @"
print("Hello World")
"@
    Set-Content -Path $testFile -Value $initialCode
    Write-Info "已创建测试文件: $testFile"

    Write-Info "发送 Agent 任务: 修改 hello.py, 将 print 改为接收用户输入的名字"
    $task1 = @{
        message = "修改 test_project/hello.py，让程序询问用户名字然后打印 Hello, {name}"
        use_screen = $false
    } | ConvertTo-Json

    try {
        $resp = Invoke-WebRequest -Uri "http://127.0.0.1:$Port/chat" `
            -Method POST `
            -ContentType "application/json" `
            -Body $task1 `
            -TimeoutSec 120
        $result = $resp.Content | ConvertFrom-Json
        Write-Pass "Agent 响应:"
        Write-Host "    $($result.reply)" -ForegroundColor White
        Write-KeyVal "  工具调用次数" $result.tool_calls.Count
        Write-KeyVal "  迭代次数" $result.iterations
        Write-KeyVal "  工具列表" ($result.tool_calls -join ", ")
    } catch {
        Write-Fail "请求失败: $($_.Exception.Message)"
    }

    # 任务 2: 执行命令
    Write-Host "`n  ── 测试 2: 执行命令 ──" -ForegroundColor Yellow

    Write-Info "发送 Agent 任务: 列出 test_project 目录下的文件"
    $task2 = @{
        message = "列出 test_project 目录下的所有文件"
        use_screen = $false
    } | ConvertTo-Json

    try {
        $resp = Invoke-WebRequest -Uri "http://127.0.0.1:$Port/chat" `
            -Method POST `
            -ContentType "application/json" `
            -Body $task2 `
            -TimeoutSec 120
        $result = $resp.Content | ConvertFrom-Json
        Write-Pass "Agent 响应:"
        Write-Host "    $($result.reply)" -ForegroundColor White
        Write-KeyVal "  工具调用次数" $result.tool_calls.Count
        Write-KeyVal "  迭代次数" $result.iterations
        Write-KeyVal "  工具列表" ($result.tool_calls -join ", ")
    } catch {
        Write-Fail "请求失败: $($_.Exception.Message)"
    }

    Write-Host "`n  ── 测试完成 ──" -ForegroundColor Green
}

# ============================================================
# 完成
# ============================================================
Write-Host "`n$('='*60)" -ForegroundColor Cyan
Write-Host "  Agent 服务运行中" -ForegroundColor Green
Write-Host "  地址: http://127.0.0.1:$Port" -ForegroundColor White
Write-Host "  日志: $LogFile" -ForegroundColor Gray
Write-Host "  进程 PID: $($proc.Id)" -ForegroundColor Gray
Write-Host "  Ctrl+C 停止服务" -ForegroundColor Yellow
Write-Host "$('='*60)" -ForegroundColor Cyan

# 实时显示日志
Write-Host "`n实时日志输出 (按 Ctrl+C 停止):" -ForegroundColor DarkGray
Write-Host "──────────────────────────────────────────────────" -ForegroundColor DarkGray

try {
    Get-Content $LogFile -Wait -Tail 0
} catch {
    # 用户中断
} finally {
    Write-Host "`n正在停止服务..." -ForegroundColor Yellow
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    Write-Host "服务已停止" -ForegroundColor Green
}