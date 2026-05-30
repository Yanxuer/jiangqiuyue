const API_BASE = 'http://127.0.0.1:8000';
const WS_URL = 'ws://127.0.0.1:8000/ws';

let isScreenMode = false;
let isThinking = false;
let ws = null;
let reconnectTimer = null;
let backendReady = false;
let connectingActive = false;

let currentModel = 'deepseek-chat';
let currentBaseUrl = 'https://api.deepseek.com';
let currentApiKey = '';

const dialogMessages = document.getElementById('dialogMessages');
const dialogInput = document.getElementById('dialogInput');
const dialogSendBtn = document.getElementById('dialogSendBtn');
const connStatus = document.getElementById('connStatus');

/* ==================== 日志系统 ==================== */
function appLog(...args) {
    const msg = args.join(' ');
    console.log('[App]', msg);
    if (window.electronAPI?.log) {
        window.electronAPI.log(msg).catch(() => {});
    }
}

function appLogError(...args) {
    const msg = args.join(' ');
    console.error('[App ERROR]', msg);
    if (window.electronAPI?.logError) {
        window.electronAPI.logError(msg).catch(() => {});
    }
}

/* ==================== Entrance Animation Sequence ==================== */

function showMainUI() {
    appLog('进入主界面');
    hideConnectingOverlay();
    const appContainer = document.getElementById('appContainer');
    appContainer.classList.add('show');
    if (window.electronAPI?.showFloatWindow) {
        appLog('显示悬浮窗');
        window.electronAPI.showFloatWindow();
    }
    connectWebSocket();
    checkBackendConfig();
}

function startEntranceSequence() {
    appLog('Arknights 促融共竞 转场动画开始 (总时长2.5s)');

    const splash = document.getElementById('arknightsSplash');

    // Phase 1 (0-0.6s): 前置蓄能·基底生成
    // 哑光深灰金属基底浮现 + 7层同心圆能量波纹扩散 + 金属粒子漂移 + 顶部冷光扫入
    setTimeout(() => {
        splash.classList.add('phase-1');
        appLog('Phase 1: 前置蓄能 - 金属基底 + 同心波纹 + 粒子漂移');
    }, 50);

    // Phase 2 (0.6s-1.4s): 核心蚀刻·Logo塑形（主视觉阶段）
    // 激光蚀刻动效 + Logo实体化 + 径向能量爆纹 + 镜头微推 + 高光闪烁
    setTimeout(() => {
        splash.classList.remove('phase-1');
        splash.classList.add('phase-2');
        appLog('Phase 2: 核心蚀刻 - Logo激光蚀刻成型 + 能量爆纹 + 镜头推进');
    }, 600);

    // Phase 3 (1.4s-2.2s): 融合竞态·动态强化
    // 波纹二次翻涌脉冲 + 材质流动 + 几何碎片环绕 + 冷色硬光横扫 + 亮度起伏
    setTimeout(() => {
        splash.classList.remove('phase-2');
        splash.classList.add('phase-3');
        appLog('Phase 3: 融合竞态 - 波纹共振 + 材质流动 + 碎片环绕 + 硬光扫射');
    }, 1400);

    // Phase 4 (2.2s-2.5s): 收尾定格·平稳收束
    // 波纹/粒子/碎片收缩消散 + Logo定格 + 光效收敛
    setTimeout(() => {
        splash.classList.remove('phase-3');
        splash.classList.add('phase-4');
        appLog('Phase 4: 收尾定格 - 元素收敛 + Logo定格 + 干净收束');
    }, 2200);

    // 动画完成 (2.6s): 隐藏Splash，进入登录流程
    setTimeout(() => {
        appLog('Arknights 促融共竞 转场完成，进入登录流程');
        splash.classList.add('hidden');
        splash.classList.remove('phase-4');

        const loginScreen = document.getElementById('loginScreen');
        loginScreen.classList.add('show');

        const loginCard = document.querySelector('.login-card');
        setTimeout(() => loginCard.classList.add('show'), 100);

        appLog('阶段2: 开始打字动画');
        typeName('Jiang Qiuyue', () => {
            appLog('阶段3: 开始密码填充');
            fillPasswordDots(() => {
                appLog('阶段4: 登录完成，检查后端状态');
                setTimeout(() => {
                    loginScreen.classList.add('hidden');
                    setTimeout(() => {
                        loginScreen.style.display = 'none';
                        splash.style.display = 'none';
                        if (backendReady) {
                            appLog('后端已就绪，直接进入主界面');
                            showMainUI();
                        } else {
                            appLog('后端未就绪，显示连接等待界面');
                            showConnectingOverlay();
                            waitForBackendAndStart();
                        }
                    }, 500);
                }, 600);
            });
        });
    }, 2600);
    appLog('Arknights 促融共竞 转场动画已调度');
}

// 监听主进程的后端状态通知
appLog('注册 backend-status 监听器');
if (window.electronAPI?.onBackendStatus) {
    window.electronAPI.onBackendStatus((ready) => {
        appLog(`收到主进程后端状态通知: ready=${ready}`);
        backendReady = ready;
        if (ready && connectingActive) {
            appLog('后端就绪，立即进入主界面');
            showMainUI();
        }
    });
}

function showConnectingOverlay() {
    appLog('显示连接等待浮层');
    connectingActive = true;
    const overlay = document.getElementById('connectingOverlay');
    if (overlay) {
        overlay.classList.add('show');
        overlay.style.display = 'flex';
    }
}

function hideConnectingOverlay() {
    appLog('隐藏连接等待浮层');
    connectingActive = false;
    const overlay = document.getElementById('connectingOverlay');
    if (overlay) {
        overlay.classList.remove('show');
        setTimeout(() => { overlay.style.display = 'none'; }, 500);
    }
}

async function waitForBackendAndStart() {
    const maxRetries = 60;
    let attempts = 0;

    appLog('开始轮询后端健康检查');

    function updateStatus(msg) {
        const status = document.getElementById('connectStatus');
        if (status) status.textContent = msg;
    }

    function updateProgress(pct) {
        const bar = document.getElementById('connectProgress');
        if (bar) bar.style.width = pct + '%';
    }

    function attempt() {
        if (!connectingActive) {
            appLog('连接已取消，停止轮询');
            return;
        }
        attempts++;
        updateStatus(`正在连接后端服务... (${attempts}/${maxRetries})`);
        updateProgress(Math.min((attempts / maxRetries) * 100, 95));

        fetch('http://127.0.0.1:8000/health', { signal: AbortSignal.timeout(3000) })
            .then(res => {
                appLog(`健康检查响应 status=${res.status}`);
                if (res.ok) {
                    updateStatus('连接成功！');
                    updateProgress(100);
                    appLog('健康检查通过，进入主界面');
                    setTimeout(() => {
                        showMainUI();
                    }, 400);
                } else {
                    retry();
                }
            })
            .catch((err) => {
                appLog(`健康检查失败: ${err.name}:${err.message}`);
                retry();
            });
    }

    function retry() {
        if (attempts >= maxRetries) {
            appLogError(`连接超时，已尝试 ${maxRetries} 次`);
            updateStatus('后端连接超时，请检查服务是否运行或重试');
            document.getElementById('connectRetryBtn').style.display = 'inline-block';
            return;
        }
        setTimeout(attempt, 2000);
    }

    setTimeout(attempt, 1000);
}

function retryBackendConnection() {
    appLog('用户点击重试连接');
    document.getElementById('connectRetryBtn').style.display = 'none';
    connectingActive = true;
    waitForBackendAndStart();
}

function typeName(name, callback) {
    const container = document.getElementById('loginNameDisplay');
    container.innerHTML = '';
    let i = 0;

    function addChar() {
        if (i >= name.length) {
            setTimeout(callback, 400);
            return;
        }
        const span = document.createElement('span');
        span.className = 'char';
        span.textContent = name[i];
        span.style.animationDelay = '0s';
        container.appendChild(span);
        i++;
        setTimeout(addChar, 120);
    }

    setTimeout(addChar, 300);
}

function fillPasswordDots(callback) {
    const dots = document.querySelectorAll('.pwd-dot');
    let i = 0;

    function fillNext() {
        if (i >= dots.length) {
            setTimeout(callback, 500);
            return;
        }
        dots[i].textContent = '●';
        dots[i].classList.add('filled');
        i++;
        setTimeout(fillNext, 200);
    }

    setTimeout(fillNext, 300);
}

document.addEventListener('DOMContentLoaded', function() {
    console.log('[App] script loaded directly, entrance may be triggered from inline');
});

/* ==================== Dialog Nav ==================== */

document.querySelectorAll('.dialog-nav-item').forEach(item => {
    item.addEventListener('click', function() {
        document.querySelectorAll('.dialog-nav-item').forEach(n => n.classList.remove('active'));
        this.classList.add('active');
    });
});

/* ==================== WebSocket ==================== */

function connectWebSocket() {
    if (ws && ws.readyState === WebSocket.OPEN) return;

    ws = new WebSocket(WS_URL);

    ws.onopen = () => {
        updateConnectionStatus(true);
    };

    ws.onmessage = (event) => {
        const data = JSON.parse(event.data);
        if (data.type === 'agent_reply') {
            addMessage('agent', data.reply, data.tools_used);
            setIsThinking(false);
        }
    };

    ws.onclose = () => {
        updateConnectionStatus(false);
        scheduleReconnect();
    };

    ws.onerror = () => {
        updateConnectionStatus(false);
    };
}

function scheduleReconnect() {
    if (reconnectTimer) return;
    reconnectTimer = setInterval(() => {
        if (!ws || ws.readyState !== WebSocket.OPEN) {
            connectWebSocket();
        } else {
            clearInterval(reconnectTimer);
            reconnectTimer = null;
        }
    }, 3000);
}

function updateConnectionStatus(connected) {
    const dot = connStatus.querySelector('.status-dot');
    const label = connStatus.querySelector('.status-label');
    if (dot) {
        dot.className = 'status-dot' + (connected ? ' connected' : ' disconnected');
    }
    if (label) {
        label.textContent = connected ? '已连接' : '已断开';
    }
}

/* ==================== Chat Functions ==================== */

function autoResize(textarea) {
    textarea.style.height = 'auto';
    textarea.style.height = Math.min(textarea.scrollHeight, 120) + 'px';
}

function handleKeyDown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        sendMessage();
    }
}

async function sendMessage() {
    const text = dialogInput.value.trim();
    if (!text || isThinking) return;

    dialogInput.value = '';
    dialogInput.style.height = 'auto';

    addMessage('user', text);
    setIsThinking(true);

    try {
        const res = await fetch(`${API_BASE}/chat`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                message: text,
                use_screen: isScreenMode
            })
        });

        if (!res.ok) throw new Error(`HTTP ${res.status}`);

        const data = await res.json();
        setIsThinking(false);

        if (data.reply) {
            addMessage('agent', data.reply, data.tool_calls);
        }

        checkPendingCli();
    } catch (err) {
        addMessage('agent', `⚠️ 请求失败: ${err.message}，请检查后端服务是否运行`);
        setIsThinking(false);
    }
}

function setIsThinking(val) {
    isThinking = val;
    dialogSendBtn.disabled = val;
    dialogSendBtn.style.opacity = val ? '0.5' : '1';

    removeThinkingIndicator();
    if (val) {
        const div = document.createElement('div');
        div.className = 'thinking-indicator';
        div.id = 'thinkingIndicator';
        div.innerHTML = '<div class="thinking-dot"></div><div class="thinking-dot"></div><div class="thinking-dot"></div>';
        dialogMessages.appendChild(div);
        scrollToBottom();
    }
}

function removeThinkingIndicator() {
    const el = document.getElementById('thinkingIndicator');
    if (el) el.remove();
}

function addMessage(role, content, toolsUsed) {
    removeWelcomeMessage();

    const div = document.createElement('div');
    div.className = `message ${role}`;

    const avatar = document.createElement('div');
    avatar.className = 'message-avatar';
    avatar.textContent = role === 'user' ? '👤' : '✦';

    const bubble = document.createElement('div');
    bubble.className = 'message-bubble';

    if (role === 'agent') {
        if (typeof marked !== 'undefined') {
            bubble.innerHTML = marked.parse(content);
        } else {
            bubble.textContent = content;
        }
        bubble.querySelectorAll('pre code').forEach((block) => {
            if (typeof hljs !== 'undefined') {
                hljs.highlightElement(block);
            }

            const pre = block.parentElement;
            const copyBtn = document.createElement('button');
            copyBtn.className = 'copy-btn';
            copyBtn.textContent = '📋 复制';
            copyBtn.onclick = async () => {
                try {
                    await navigator.clipboard.writeText(block.textContent);
                    copyBtn.textContent = '✅ 已复制';
                    setTimeout(() => { copyBtn.textContent = '📋 复制'; }, 2000);
                } catch {
                    const range = document.createRange();
                    range.selectNode(block);
                    window.getSelection().removeAllRanges();
                    window.getSelection().addRange(range);
                    document.execCommand('copy');
                    copyBtn.textContent = '✅ 已复制';
                    setTimeout(() => { copyBtn.textContent = '📋 复制'; }, 2000);
                }
            };
            pre.appendChild(copyBtn);
        });

        if (toolsUsed && toolsUsed.length > 0) {
            const toolsInfo = document.createElement('div');
            toolsInfo.style.cssText = 'margin-top: 8px; padding-top: 8px; border-top: 1px solid rgba(255,255,255,0.1); font-size: 12px; color: var(--text-muted);';
            toolsInfo.textContent = `🔧 使用了: ${toolsUsed.join(', ')}`;
            bubble.appendChild(toolsInfo);
        }
    } else {
        bubble.textContent = content;
    }

    div.appendChild(avatar);
    div.appendChild(bubble);
    dialogMessages.appendChild(div);
    scrollToBottom();
}

function removeWelcomeMessage() {
    const welcome = document.querySelector('.welcome-message');
    if (welcome) welcome.remove();
}

function scrollToBottom() {
    dialogMessages.scrollTop = dialogMessages.scrollHeight;
}

function clearChat() {
    dialogMessages.innerHTML = '';
    const welcome = document.createElement('div');
    welcome.className = 'welcome-message';
    welcome.innerHTML = `
        <div class="welcome-avatar">✦</div>
        <h2>你好呀~ 我是江秋月</h2>
        <p>游走于信息与创意之间，以理性架构梳理脉络，<br>以细腻感知回应诉求。不问边界，不限场景，<br>做你随心可用的专属智能载体。</p>
        <p class="welcome-hint">在下方输入你的指令开始吧</p>
    `;
    dialogMessages.appendChild(welcome);
}

/* ==================== File Manager ==================== */

let fileModal = document.getElementById('fileModal');

async function openFileManager() {
    fileModal.classList.add('show');
    await refreshFileList();
}

function closeFileManager() {
    fileModal.classList.remove('show');
    // 关闭文件管理器后自动跳转到对话
    const navItems = document.querySelectorAll('.dialog-nav-item');
    navItems.forEach(n => n.classList.remove('active'));
    if (navItems.length > 0) navItems[0].classList.add('active');
}

async function refreshFileList() {
    try {
        const res = await fetch(`${API_BASE}/files?dir=`);
        const data = await res.json();
        const fileList = document.getElementById('fileList');
        fileList.innerHTML = '';

        if (data.files) {
            const files = typeof data.files === 'string'
                ? data.files.split('\n').filter(Boolean)
                : data.files;

            files.forEach((file) => {
                const item = document.createElement('div');
                item.className = 'file-item';
                const ext = file.split('.').pop();
                const icon = ['py', 'js', 'ts', 'html', 'css', 'json'].includes(ext) ? '📄' : '📎';
                item.innerHTML = `<span class="file-icon">${icon}</span><span>${file}</span>`;
                item.onclick = () => loadFile(file);
                fileList.appendChild(item);
            });
        }
    } catch (err) {
        document.getElementById('fileList').innerHTML = `<div class="file-item" style="color: #ef4444;">⚠️ 加载失败: ${err.message}</div>`;
    }
}

async function browseCustomPath() {
    const input = document.getElementById('filePathInput');
    const path = input.value.trim();
    if (!path) {
        showFileBrowserError('请输入有效的路径');
        return;
    }

    try {
        const res = await fetch(`${API_BASE}/docs/list?path=${encodeURIComponent(path)}`);

        if (!res.ok) {
            throw new Error(`HTTP ${res.status}`);
        }

        const data = await res.json();
        if (!data.success) {
            throw new Error(data.error || '读取目录失败');
        }
        renderFileBrowser(path, data);
    } catch (err) {
        showFileBrowserError(`读取失败: ${err.message}`);
    }
}

function renderFileBrowser(currentPath, data) {
    document.getElementById('fileBrowserPath').textContent = currentPath;

    const list = document.getElementById('fileBrowserList');
    list.innerHTML = '';

    const entries = [];

    if (data.directories) {
        data.directories.forEach(dir => {
            entries.push({ name: dir.name, is_dir: true });
        });
    }

    if (data.files) {
        data.files.forEach(file => {
            entries.push({ name: file.name, is_dir: false, size: file.size });
        });
    }

    // Sort: directories first, then files
    entries.sort((a, b) => {
        if (a.is_dir && !b.is_dir) return -1;
        if (!a.is_dir && b.is_dir) return 1;
        return a.name.localeCompare(b.name);
    });

    document.getElementById('fileCount').textContent = `${entries.length} 项`;

    if (entries.length === 0) {
        list.innerHTML = '<div class="file-browser-empty"><div class="empty-icon">📂</div><div>此目录为空</div><div class="empty-hint">尝试输入其他路径</div></div>';
        return;
    }

    entries.forEach(entry => {
        const item = document.createElement('div');
        item.className = `file-browser-item${entry.is_dir ? ' is-dir' : ''}`;

        const icon = entry.is_dir ? '📁' : getFileIcon(entry.name);

        const fullPath = currentPath.replace(/\\$/, '') + '\\' + entry.name;

        item.innerHTML = `
            <span class="item-icon">${icon}</span>
            <span class="item-name">${entry.name}</span>
            <span class="item-meta">${entry.is_dir ? '文件夹' : getFileSize(entry)}</span>
        `;

        if (entry.is_dir) {
            item.onclick = () => browseToDir(fullPath);
        } else {
            item.onclick = () => previewFile(fullPath);
        }

        list.appendChild(item);
    });
}

function browseToDir(path) {
    document.getElementById('filePathInput').value = path;
    browseCustomPath();
}

async function previewFile(path) {
    try {
        const res = await fetch(`${API_BASE}/file/read`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path })
        });
        const data = await res.json();
        const content = data.content || '';
        const preview = content.substring(0, 2000);

        // Show in a temporary message in chat
        closeFileManager();
        addMessage('agent', `📄 **文件预览**: \`${path}\`\n\n\`\`\`\n${preview}\n\`\`\`\n\n*仅显示前 2000 字符*`);
    } catch (err) {
        addMessage('agent', `⚠️ 读取文件失败: ${err.message}`);
    }
}

function showFileBrowserError(msg) {
    document.getElementById('fileBrowserPath').textContent = '错误';
    document.getElementById('fileCount').textContent = '';
    document.getElementById('fileBrowserList').innerHTML = `<div class="file-browser-error">⚠️ ${msg}</div>`;
}

function getFileIcon(filename) {
    const ext = filename.split('.').pop().toLowerCase();
    const iconMap = {
        'py': '🐍', 'js': '📜', 'ts': '📘', 'html': '🌐', 'css': '🎨',
        'json': '📋', 'xml': '📋', 'md': '📝', 'txt': '📄', 'pdf': '📕',
        'jpg': '🖼️', 'jpeg': '🖼️', 'png': '🖼️', 'gif': '🖼️', 'svg': '🖼️',
        'zip': '📦', 'rar': '📦', '7z': '📦', 'exe': '⚙️', 'dll': '🔧',
        'doc': '📘', 'docx': '📘', 'xls': '📊', 'xlsx': '📊', 'ppt': '📙',
        'mp3': '🎵', 'mp4': '🎬', 'wav': '🎵', 'avi': '🎬', 'mov': '🎬'
    };
    return iconMap[ext] || '📄';
}

function getFileSize(entry) {
    if (entry.size !== undefined) {
        const bytes = parseInt(entry.size);
        if (bytes < 1024) return `${bytes} B`;
        if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
        return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    }
    return '';
}

async function loadFile(path) {
    // Navigate to the directory containing this file and show in browser
    const dir = path.substring(0, path.lastIndexOf('\\'));
    const fileName = path.substring(path.lastIndexOf('\\') + 1);

    document.getElementById('filePathInput').value = dir;
    await browseCustomPath();

    // Highlight the file in the browser
    const items = document.querySelectorAll('.file-browser-item');
    items.forEach(item => {
        const nameEl = item.querySelector('.item-name');
        if (nameEl && nameEl.textContent === fileName) {
            item.style.background = 'rgba(36, 194, 216, 0.12)';
            item.style.borderColor = 'rgba(36, 194, 216, 0.3)';
            item.scrollIntoView({ block: 'center' });
        }
    });
}

const memoryModal = document.getElementById('memoryModal');

async function openMemorySearch() {
    memoryModal.classList.add('show');
    setTimeout(() => document.getElementById('memorySearchInput').focus(), 200);
}

function closeMemorySearch() {
    memoryModal.classList.remove('show');
}

async function executeMemorySearch() {
    const query = document.getElementById('memorySearchInput').value.trim();
    if (!query) return;

    closeMemorySearch();

    try {
        const res = await fetch(`${API_BASE}/memory/search`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ query })
        });
        const data = await res.json();

        if (data.results && data.results.length > 0) {
            let msg = '📝 **记忆搜索结果**\n\n';
            data.results.forEach((r, i) => {
                msg += `${i + 1}. ${r.content}\n`;
                if (r.category || r.time) {
                    msg += `   *${r.category || ''} | ${r.time || ''}*\n`;
                }
                if (r.distance !== null && r.distance !== undefined) {
                    msg += `   *(相似度: ${(1 - r.distance).toFixed(2)})*\n`;
                }
                msg += '\n';
            });
            addMessage('agent', msg);
        } else {
            addMessage('agent', '没有找到相关的记忆~');
        }
    } catch (err) {
        addMessage('agent', `⚠️ 搜索失败: ${err.message}`);
    }

    document.getElementById('memorySearchInput').value = '';
}

document.addEventListener('click', (e) => {
    if (e.target === fileModal) {
        closeFileManager();
    }
    if (e.target === memoryModal) {
        closeMemorySearch();
    }
});

document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
        if (fileModal.classList.contains('show')) closeFileManager();
        if (memoryModal.classList.contains('show')) closeMemorySearch();
        if (cliModal.classList.contains('show')) closeCliConfirm();
    }
    if (e.key === 'Enter' && memoryModal.classList.contains('show')) {
        executeMemorySearch();
    }
});

/* ==================== CLI 安全确认 ==================== */

const cliModal = document.getElementById('cliConfirmModal');
let currentCliId = null;

async function checkPendingCli() {
    try {
        const res = await fetch(`${API_BASE}/cli/pending`);
        const data = await res.json();
        if (data.pending && data.pending.length > 0) {
            const cmd = data.pending[0];
            showCliConfirm(cmd);
        }
    } catch (err) {
        appLog('检查待确认CLI命令失败:', err.message);
    }
}

function showCliConfirm(cmd) {
    currentCliId = cmd.command_id;
    document.getElementById('cliOpType').textContent = cmd.operation_type || '执行命令';
    document.getElementById('cliReason').textContent = cmd.reason || 'AI 需要执行一个命令行操作';
    document.getElementById('cliCommandText').textContent = cmd.command;

    const filesSection = document.getElementById('cliFilesSection');
    const filesList = document.getElementById('cliFilesList');
    if (cmd.affected_files && cmd.affected_files.length > 0) {
        filesSection.style.display = 'block';
        filesList.innerHTML = cmd.affected_files.map(f =>
            `<div class="cli-file-item">${f}</div>`
        ).join('');
    } else {
        filesSection.style.display = 'none';
    }

    cliModal.classList.add('show');
    appLog('显示CLI确认对话框:', cmd.command);
}

function closeCliConfirm() {
    cliModal.classList.remove('show');
    currentCliId = null;
}

async function confirmCliCommand() {
    if (!currentCliId) return;

    const btn = document.querySelector('.cli-btn-confirm');
    btn.disabled = true;
    btn.textContent = '⏳ 执行中...';

    try {
        const res = await fetch(`${API_BASE}/cli/confirm/${currentCliId}`, {
            method: 'POST'
        });
        const data = await res.json();

        closeCliConfirm();
        btn.disabled = false;
        btn.textContent = '✅ 确认执行';

        if (data.success && data.result) {
            const output = data.result.output || '(无输出)';
            const statusIcon = data.result.success ? '✅' : '❌';
            addMessage('agent',
                `**${statusIcon} 命令执行完成**\n\n` +
                `\`\`\`\n${output.substring(0, 2000)}\n\`\`\`\n` +
                `> 退出码: ${data.result.exit_code}`
            );
        } else {
            addMessage('agent', `⚠️ 命令执行失败: ${data.error || '未知错误'}`);
        }
    } catch (err) {
        closeCliConfirm();
        btn.disabled = false;
        btn.textContent = '✅ 确认执行';
        addMessage('agent', `⚠️ 执行请求失败: ${err.message}`);
    }
}

async function rejectCliCommand() {
    if (!currentCliId) return;

    try {
        await fetch(`${API_BASE}/cli/reject/${currentCliId}`, {
            method: 'POST'
        });
    } catch (err) {
        appLog('取消CLI命令失败:', err.message);
    }

    addMessage('agent', '已取消该操作~ 如果有什么需要调整的，告诉我就好 ✨');
    closeCliConfirm();
}

function copyCliCommand() {
    const text = document.getElementById('cliCommandText').textContent;
    navigator.clipboard.writeText(text).then(() => {
        const btn = document.querySelector('.cli-copy-btn');
        btn.textContent = '✅ 已复制';
        setTimeout(() => { btn.textContent = '📋 复制'; }, 2000);
    });
}

document.addEventListener('click', (e) => {
    if (e.target === cliModal) {
        closeCliConfirm();
    }
    if (e.target === softwareModal) {
        closeSoftwarePanel();
    }
});

/* ==================== 软件面板 ==================== */

const softwareModal = document.getElementById('softwareModal');
let softwareData = null;

async function refreshSoftwareBadge() {
    try {
        const res = await fetch(`${API_BASE}/software/status`);
        const data = await res.json();
        const countEl = document.getElementById('softwareCount');
        if (data.scanned) {
            countEl.textContent = data.total !== undefined ? data.total : '0';
        } else {
            countEl.textContent = '...';
        }
        return data;
    } catch {
        document.getElementById('softwareCount').textContent = '?';
        return null;
    }
}

async function openSoftwarePanel() {
    softwareModal.classList.add('show');

    const statusEl = document.getElementById('softwareScanStatus');
    const grid = document.getElementById('softwareGrid');

    statusEl.textContent = '加载中...';
    statusEl.className = 'software-scan-status scanning';
    grid.innerHTML = '<div class="software-loading">正在获取软件列表...</div>';

    try {
        const res = await fetch(`${API_BASE}/software/list`);
        const data = await res.json();
        softwareData = data;

        if (data.scanned) {
            statusEl.textContent = '扫描完成';
            statusEl.className = 'software-scan-status done';
        } else {
            statusEl.textContent = '扫描中...';
            statusEl.className = 'software-scan-status scanning';
        }

        renderSoftwareGrid(data);
    } catch (err) {
        grid.innerHTML = `<div class="software-loading">加载失败: ${err.message}</div>`;
        statusEl.textContent = '加载失败';
        statusEl.className = 'software-scan-status';
    }
}

function closeSoftwarePanel() {
    softwareModal.classList.remove('show');
}

function renderSoftwareGrid(data) {
    const grid = document.getElementById('softwareGrid');

    if (!data.software || data.software.length === 0) {
        grid.innerHTML = '<div class="software-loading">暂未发现已安装的软件</div>';
        return;
    }

    const cats = {};
    for (const sw of data.software) {
        const cat = sw.category || 'other';
        if (!cats[cat]) cats[cat] = [];
        cats[cat].push(sw.name);
    }

    let html = '';
    const catLabels = {
        browser: '浏览器', editor: '编辑器', terminal: '终端', office: '办公',
        image: '图片处理', video: '视频处理', audio: '音频处理',
        development: '开发工具', database: '数据库', design: '设计创作',
        communication: '通讯社交', utility: '实用工具', game: '游戏',
        music: '音乐播放', pdf: 'PDF阅读', compression: '压缩解压',
        download: '下载工具', other: '其他',
    };

    for (const [cat, names] of Object.entries(cats)) {
        const label = catLabels[cat] || cat;
        html += '<div class="software-category-section">';
        html += `<div class="software-category-label">${label} (${names.length})</div>`;
        html += '<div class="software-category-items">';
        for (const name of names.slice(0, 30)) {
            html += `<span class="software-chip">${name}</span>`;
        }
        if (names.length > 30) {
            html += `<span class="software-chip">+${names.length - 30} 更多</span>`;
        }
        html += '</div></div>';
    }

    grid.innerHTML = html;
}

async function searchSoftware() {
    const query = document.getElementById('softwareSearchInput').value.trim();
    if (!query) {
        renderSoftwareGrid(softwareData || { software: [] });
        return;
    }

    const grid = document.getElementById('softwareGrid');
    grid.innerHTML = '<div class="software-loading">搜索中...</div>';

    try {
        const res = await fetch(`${API_BASE}/software/search?query=${encodeURIComponent(query)}`);
        const data = await res.json();

        if (!data.software || data.software.length === 0) {
            grid.innerHTML = '<div class="software-loading">未找到匹配的软件</div>';
            return;
        }

        let html = '<div class="software-search-results">';
        for (const sw of data.software) {
            html += '<div class="software-search-item">';
            html += `<span class="software-search-item-name">${sw.name}</span>`;
            html += `<span class="software-search-item-category">${sw.category}</span>`;
            html += '</div>';
        }
        html += '</div>';
        grid.innerHTML = html;
    } catch (err) {
        grid.innerHTML = `<div class="software-loading">搜索失败: ${err.message}</div>`;
    }
}

// 定期刷新软件数量
setInterval(refreshSoftwareBadge, 10000);
refreshSoftwareBadge();

/* ==================== 记忆状态 & 模式面板 ==================== */

let memoryPanelOpen = false;
let memoryRetryInProgress = false;

async function refreshMemoryStatus() {
    try {
        const res = await fetch(`${API_BASE}/memory/status`);
        const data = await res.json();
        const dot = document.getElementById('memoryStatusDot');
        const text = document.getElementById('memoryStatusText');
        const badge = document.getElementById('memoryStatus');
        if (data.mode === 'vector') {
            dot.className = 'memory-status-dot online';
            text.textContent = '记忆';
            badge.title = '向量记忆库在线';
        } else if (data.mode === 'retrying') {
            dot.className = 'memory-status-dot online';
            text.textContent = '重试中';
            badge.title = '向量记忆正在重试...';
        } else {
            dot.className = 'memory-status-dot fallback';
            text.textContent = '记忆';
            badge.title = '使用SQL降级模式';
        }
        if (memoryPanelOpen && document.getElementById('memoryPanel').classList.contains('open')) {
            updateMemoryPanel(data);
        }
    } catch {
        const dot = document.getElementById('memoryStatusDot');
        dot.className = 'memory-status-dot offline';
    }
}

function toggleMemoryPanel() {
    const panel = document.getElementById('memoryPanel');
    memoryPanelOpen = !panel.classList.contains('open');
    if (memoryPanelOpen) {
        panel.classList.add('open');
        refreshMemoryPanel();
    } else {
        panel.classList.remove('open');
    }
}

function closeMemoryPanel() {
    document.getElementById('memoryPanel').classList.remove('open');
    memoryPanelOpen = false;
}

async function refreshMemoryPanel() {
    try {
        const res = await fetch(`${API_BASE}/memory/status`);
        const data = await res.json();
        updateMemoryPanel(data);
    } catch (err) {
        appLog('获取记忆状态失败:', err.message);
    }
}

function updateMemoryPanel(data) {
    const modeMap = { vector: '向量模式', sql: 'SQL模式', retrying: '重试中...' };
    const statusMap = { vector: '✅ 正常运行', sql: '⚠️ 降级运行', retrying: '🔄 正在重试' };

    document.getElementById('memoryPanelMode').textContent = modeMap[data.mode] || data.mode;
    document.getElementById('memoryPanelStatus').textContent = statusMap[data.mode] || '-';
    document.getElementById('memoryPanelRetryCount').textContent = `${data.retry_count}/${data.max_retries}`;

    const errorRow = document.getElementById('memoryPanelErrorRow');
    const errorEl = document.getElementById('memoryPanelError');
    if (data.last_error) {
        errorRow.style.display = 'flex';
        errorEl.textContent = data.last_error;
    } else {
        errorRow.style.display = 'none';
        errorEl.textContent = '';
    }

    const btnVector = document.getElementById('memoryBtnVector');
    const btnSql = document.getElementById('memoryBtnSql');
    const btnRetry = document.getElementById('memoryBtnRetry');

    btnVector.disabled = (data.mode === 'vector' || memoryRetryInProgress);
    btnSql.disabled = (data.mode === 'sql' || memoryRetryInProgress);
    btnRetry.disabled = memoryRetryInProgress || (data.mode === 'vector');

    btnVector.classList.toggle('active', data.mode === 'vector');
    btnSql.classList.toggle('active', data.mode === 'sql');

    renderMemoryLogs(data.vector_logs || []);
}

function renderMemoryLogs(logs) {
    const container = document.getElementById('memoryPanelLogs');
    if (!logs || logs.length === 0) {
        container.innerHTML = '<div class="memory-log-empty">暂无日志</div>';
        return;
    }
    container.innerHTML = logs.map(log => {
        const levelClass = log.level.toUpperCase();
        return `<div class="memory-log-entry">
            <span class="memory-log-time">${escapeHtml(log.time)}</span>
            <span class="memory-log-level ${levelClass}">${escapeHtml(log.level)}</span>
            <span class="memory-log-msg">${escapeHtml(log.message)}</span>
        </div>`;
    }).join('');
    container.scrollTop = container.scrollHeight;
}

async function switchMemoryMode(mode) {
    if (memoryRetryInProgress) return;
    try {
        const res = await fetch(`${API_BASE}/memory/switch?mode=${mode}`, { method: 'POST' });
        const data = await res.json();
        if (data.success) {
            appLog(`记忆模式已切换到: ${mode}`);
            await refreshMemoryPanel();
            await refreshMemoryStatus();
        }
    } catch (err) {
        appLog('切换记忆模式失败:', err.message);
    }
}

async function retryVectorMemory() {
    if (memoryRetryInProgress) return;
    memoryRetryInProgress = true;
    const btn = document.getElementById('memoryBtnRetry');
    btn.disabled = true;
    btn.textContent = '⏳ 重试中...';

    try {
        const res = await fetch(`${API_BASE}/memory/retry`, { method: 'POST' });
        const data = await res.json();
        await refreshMemoryPanel();
        await refreshMemoryStatus();

        if (data.success) {
            appLog('向量记忆重试成功');
        } else {
            appLog('向量记忆重试失败:', data.last_error || data.error);
            if (data.retry_count >= data.max_retries) {
                showMemoryRetryError(data.last_error || data.error || '未知错误');
            }
        }
    } catch (err) {
        appLog('向量记忆重试请求失败:', err.message);
        showMemoryRetryError(err.message);
    } finally {
        memoryRetryInProgress = false;
        btn.disabled = false;
        btn.textContent = '🔄 重试向量';
        await refreshMemoryPanel();
        await refreshMemoryStatus();
    }
}

function showMemoryRetryError(errorDetail) {
    document.getElementById('memoryRetryErrorDetail').textContent = errorDetail || '无详细信息';
    document.getElementById('memoryRetryErrorModal').classList.add('open');
}

function closeMemoryRetryError() {
    document.getElementById('memoryRetryErrorModal').classList.remove('open');
}

setInterval(refreshMemoryStatus, 15000);
refreshMemoryStatus();

/* ==================== 文档交互 ==================== */

let currentDocPath = null;

function toggleDocSidebar() {
    const sidebar = document.getElementById('docSidebar');
    const btn = document.getElementById('docNavBtn');
    const isOpen = sidebar.classList.contains('open');

    if (isOpen) {
        sidebar.classList.remove('open');
        btn.classList.remove('active');
    } else {
        sidebar.classList.add('open');
        btn.classList.add('active');
        loadRecentPaths();
    }
}

async function loadRecentPaths() {
    try {
        const res = await fetch(`${API_BASE}/docs/recent-paths`);
        const data = await res.json();
        renderRecentPaths(data.recent_paths || []);
    } catch (err) {
        appLog('加载最近路径失败:', err.message);
    }
}

function renderRecentPaths(paths) {
    const list = document.getElementById('docRecentList');
    const hint = document.getElementById('docSidebarHint');

    if (!paths || paths.length === 0) {
        hint.style.display = 'block';
        list.innerHTML = '';
        return;
    }

    hint.style.display = 'none';
    list.innerHTML = paths.map((p, i) => {
        const isActive = currentDocPath && currentDocPath.path === p.path;
        return `
            <div class="doc-recent-item ${isActive ? 'active' : ''}" onclick="selectDocPath(${i})">
                <span class="doc-recent-icon">📁</span>
                <div class="doc-recent-info">
                    <div class="doc-recent-name">${escapeHtml(p.name)}</div>
                    <div class="doc-recent-path">${escapeHtml(p.path)}</div>
                </div>
                <button class="doc-recent-delete" onclick="event.stopPropagation(); deleteDocPath(${i})" title="移除">✕</button>
            </div>
        `;
    }).join('');
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function showPathInput() {
    document.getElementById('pathInputModal').classList.add('show');
    setTimeout(() => document.getElementById('pathInput').focus(), 200);
}

function closePathInput() {
    document.getElementById('pathInputModal').classList.remove('show');
    document.getElementById('pathInput').value = '';
}

async function browsePath() {
    if (window.electronAPI?.selectDirectory) {
        try {
            const dir = await window.electronAPI.selectDirectory();
            if (dir) {
                document.getElementById('pathInput').value = dir;
            }
        } catch (err) {
            appLog('浏览文件夹失败:', err.message);
        }
    } else {
        // Fallback: prompt for Electron remote
        document.getElementById('pathInput').value = prompt('请输入文件夹路径：', 'C:\\');
    }
}

async function quickPath(type) {
    let path = '';
    const home = 'C:\\Users';
    switch (type) {
        case 'desktop':
            path = home + '\\Public\\Desktop';
            break;
        case 'downloads':
            path = home + '\\Public\\Downloads';
            break;
        case 'documents':
            path = home + '\\Public\\Documents';
            break;
        case 'workspace':
            path = home + '\\Public\\Desktop';
            break;
    }
    document.getElementById('pathInput').value = path;
    await confirmPathSelect();
}

async function confirmPathSelect() {
    const input = document.getElementById('pathInput');
    const path = input.value.trim();
    if (!path) return;

    closePathInput();

    try {
        const res = await fetch(`${API_BASE}/docs/select-path`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path })
        });
        const data = await res.json();

        if (data.recent_paths) {
            if (data.directory && !data.directory.success) {
                addMessage('agent', `⚠️ **路径访问失败**\n\`${path}\`\n> ${data.directory.error}`);
            } else {
                currentDocPath = { path: data.directory.path, name: data.directory.name };
                addMessage('agent', `📂 **已选择文件夹**\n\n**路径**: \`${data.directory.path}\`\n**内容**: ${data.directory.directories.length} 个文件夹, ${data.directory.files.length} 个文件\n\n正在分析文件内容，请稍候...`);
                await analyzeDirectory(data.directory);
            }
            renderRecentPaths(data.recent_paths);
        }
    } catch (err) {
        addMessage('agent', `⚠️ 路径选择失败: ${err.message}`);
    }
}

async function selectDocPath(index) {
    const res = await fetch(`${API_BASE}/docs/recent-paths`);
    const data = await res.json();
    const paths = data.recent_paths || [];
    if (index >= paths.length) return;

    const p = paths[index];
    currentDocPath = { path: p.path, name: p.name };
    renderRecentPaths(paths);

    const dirRes = await fetch(`${API_BASE}/docs/list?path=${encodeURIComponent(p.path)}`);
    const dirData = await dirRes.json();

    if (dirData.success) {
        addMessage('agent', `📂 **查看文件夹**\n\n**路径**: \`${dirData.path}\`\n**内容**: ${dirData.directories.length} 个文件夹, ${dirData.files.length} 个文件\n\n正在分析文件内容，请稍候...`);
        await analyzeDirectory(dirData);
    } else {
        addMessage('agent', `⚠️ **读取失败**: ${dirData.error}`);
    }
}

async function deleteDocPath(index) {
    const res = await fetch(`${API_BASE}/docs/recent-paths`);
    const data = await res.json();
    const paths = data.recent_paths || [];
    if (index >= paths.length) return;

    const target = paths[index];
    if (currentDocPath && currentDocPath.path === target.path) {
        currentDocPath = null;
    }

    try {
        await fetch(`${API_BASE}/docs/delete-path`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path: target.path })
        });
    } catch (err) {
        appLog('删除路径失败:', err.message);
    }

    await loadRecentPaths();
}

async function analyzeDirectory(dirData) {
    const textFiles = dirData.files.filter(f => f.is_text).slice(0, 20);

    if (textFiles.length === 0) {
        addMessage('agent', '📄 该文件夹中没有找到可读取的文本文件（如 .txt, .md, .py, .js 等）。');
        return;
    }

    let allContent = '';
    let readCount = 0;

    for (const file of textFiles) {
        try {
            const fullPath = dirData.path + '\\' + file.name;
            const res = await fetch(`${API_BASE}/docs/read?path=${encodeURIComponent(fullPath)}`);
            const fileData = await res.json();

            if (fileData.success) {
                const header = `\n\n===== ${file.name} (${fileData.lines}行) =====\n\n`;
                const content = fileData.content.substring(0, 5000);
                allContent += header + content;
                readCount++;
            }
        } catch (err) {
            appLog(`读取文件 ${file.name} 失败:`, err.message);
        }

        // Prevent too large content
        if (allContent.length > 50000) break;
    }

    if (readCount === 0) {
        addMessage('agent', '📄 未能读取到任何文件内容。');
        return;
    }

    const summary = `📂 **文件夹分析完成**\n\n` +
        `**路径**: \`${dirData.path}\`\n` +
        `**共读取 ${readCount}/${textFiles.length} 个文件**\n\n` +
        `正在将内容发送给 AI 进行分析...`;

    addMessage('agent', summary);

    // Send to agent
    try {
        const chatRes = await fetch(`${API_BASE}/chat`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                message: `请分析以下文件夹中的文件内容，帮我总结项目的结构和关键信息。\n\n文件夹路径: ${dirData.path}\n\n文件内容如下:\n${allContent.substring(0, 80000)}`
            })
        });
        const chatData = await chatRes.json();
        if (chatData.reply) {
            addMessage('agent', chatData.reply);
        }
    } catch (err) {
        addMessage('agent', `⚠️ AI分析失败: ${err.message}`);
    }
}

// Close doc sidebar when clicking outside
document.addEventListener('click', (e) => {
    const sidebar = document.getElementById('docSidebar');
    if (sidebar.classList.contains('open')) {
        const isSidebar = sidebar.contains(e.target);
        const isNavBtn = document.getElementById('docNavBtn').contains(e.target);
        if (!isSidebar && !isNavBtn) {
            sidebar.classList.remove('open');
            document.getElementById('docNavBtn').classList.remove('active');
        }
    }
});

/* ==================== 配置 / 设置系统 ==================== */

async function checkBackendConfig() {
    try {
        const res = await fetch(`${API_BASE}/config`);
        if (!res.ok) return;
        const data = await res.json();
        appLog('后端配置状态:', JSON.stringify(data));

        currentBaseUrl = data.deepseek_base_url;
        currentModel = data.model;
        populateModelSelector(data.model);

        if (!data.configured) {
            const setupWizard = document.getElementById('setupWizard');
            setupWizard.classList.add('show');
        }
    } catch (err) {
        appLog('获取配置失败:', err.message);
    }
}

function populateModelSelector(activeModel) {
    const sel = document.getElementById('modelSelector');
    sel.innerHTML = '';

    const presets = [
        { label: 'DeepSeek Chat', value: 'deepseek-chat', url: 'https://api.deepseek.com' },
        { label: 'DeepSeek Reasoner', value: 'deepseek-reasoner', url: 'https://api.deepseek.com' },
        { label: 'GPT-4o', value: 'gpt-4o', url: 'https://api.openai.com' },
        { label: 'GPT-4-turbo', value: 'gpt-4-turbo', url: 'https://api.openai.com' },
        { label: 'GPT-3.5-turbo', value: 'gpt-3.5-turbo', url: 'https://api.openai.com' },
        { label: 'Moonshot v1 8k', value: 'moonshot-v1-8k', url: 'https://api.moonshot.cn/v1' },
        { label: 'Moonshot v1 32k', value: 'moonshot-v1-32k', url: 'https://api.moonshot.cn/v1' },
        { label: 'Qwen Plus', value: 'qwen-plus', url: 'https://dashscope.aliyuncs.com/compatible-mode/v1' },
        { label: 'Qwen Max', value: 'qwen-max', url: 'https://dashscope.aliyuncs.com/compatible-mode/v1' },
        { label: 'SiliconFlow DeepSeek V3', value: 'deepseek-ai/DeepSeek-V3', url: 'https://api.siliconflow.cn/v1' },
        { label: 'SiliconFlow Qwen', value: 'Qwen/Qwen2.5-72B-Instruct', url: 'https://api.siliconflow.cn/v1' },
        { label: '本地模型 (LM Studio)', value: 'local-model', url: 'http://localhost:1234/v1' },
    ];

    const alreadyAdded = new Set();
    const opt = document.createElement('option');
    opt.value = '__custom__';
    opt.textContent = '✏️ 自定义模型...';
    sel.appendChild(opt);

    const separator = document.createElement('option');
    separator.disabled = true;
    separator.textContent = '──────────';
    sel.appendChild(separator);

    for (const p of presets) {
        if (alreadyAdded.has(p.value)) continue;
        alreadyAdded.add(p.value);
        const option = document.createElement('option');
        option.value = p.value;
        option.textContent = p.label;
        option.dataset.url = p.url;
        sel.appendChild(option);
    }

    if (activeModel) {
        const exists = presets.some(p => p.value === activeModel);
        if (exists) {
            sel.value = activeModel;
        } else {
            const customOpt = document.createElement('option');
            customOpt.value = activeModel;
            customOpt.textContent = `✏️ ${activeModel}`;
            customOpt.selected = true;
            sel.insertBefore(customOpt, sel.firstChild);
        }
    }
}

function onModelChanged(value) {
    if (value === '__custom__') {
        openSettings();
        return;
    }
    const sel = document.getElementById('modelSelector');
    const selectedOpt = sel.querySelector(`option[value="${value}"]`);
    const url = selectedOpt?.dataset?.url;
    if (url) {
        currentBaseUrl = url;
        currentModel = value;
        saveConfigToBackend(currentApiKey, url, value);
    }
}

async function saveConfigToBackend(apiKey, baseUrl, model) {
    try {
        const res = await fetch(`${API_BASE}/config`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                deepseek_api_key: apiKey || null,
                deepseek_base_url: baseUrl || null,
                model: model || null
            })
        });
        const data = await res.json();
        appLog('配置保存结果:', JSON.stringify(data));
        return data;
    } catch (err) {
        appLogError('保存配置失败:', err.message);
        return { success: false };
    }
}

function openSettings() {
    const modal = document.getElementById('settingsModal');
    document.getElementById('settingsBaseUrl').value = currentBaseUrl;
    document.getElementById('settingsModel').value = currentModel;
    document.getElementById('settingsApiKey').value = currentApiKey;
    document.getElementById('settingsConfigStatus').textContent = '';
    document.querySelectorAll('.settings-provider-card').forEach(c => c.classList.remove('active'));
    modal.classList.add('show');
}

function closeSettings() {
    document.getElementById('settingsModal').classList.remove('show');
}

async function saveSettings() {
    const apiKey = document.getElementById('settingsApiKey').value.trim();
    const baseUrl = document.getElementById('settingsBaseUrl').value.trim();
    const model = document.getElementById('settingsModel').value.trim();

    if (!apiKey && !baseUrl.match(/^http:\/\/localhost/)) {
        document.getElementById('settingsConfigStatus').textContent = '⚠️ 请输入 API Key (本地部署可留空)';
        document.getElementById('settingsConfigStatus').style.color = '#f59e0b';
        return;
    }

    document.getElementById('settingsConfigStatus').textContent = '⏳ 保存中...';
    document.getElementById('settingsConfigStatus').style.color = 'var(--text-secondary)';

    const result = await saveConfigToBackend(apiKey, baseUrl, model);
    if (result.success) {
        currentApiKey = apiKey;
        currentBaseUrl = baseUrl;
        currentModel = model;
        populateModelSelector(model);
        document.getElementById('settingsConfigStatus').textContent = '✅ 配置已保存';
        document.getElementById('settingsConfigStatus').style.color = '#22c55e';
        setTimeout(() => {
            closeSettings();
            updateConnectionStatus(true);
        }, 800);
    } else {
        document.getElementById('settingsConfigStatus').textContent = '❌ 保存失败，请重试';
        document.getElementById('settingsConfigStatus').style.color = '#ef4444';
    }
}

function selectProvider(el, provider) {
    document.querySelectorAll('.settings-provider-card').forEach(c => c.classList.remove('active'));
    el.classList.add('active');

    const baseUrl = el.dataset.baseUrl;
    const model = el.dataset.model;

    const settingsModal = document.getElementById('settingsModal');
    const setupWizard = document.getElementById('setupWizard');

    if (settingsModal.classList.contains('show')) {
        document.getElementById('settingsBaseUrl').value = baseUrl;
        document.getElementById('settingsModel').value = model;
    }
    if (setupWizard.classList.contains('show')) {
        document.getElementById('wizardBaseUrl').value = baseUrl;
        document.getElementById('wizardModel').value = model;
    }
}

function toggleApiKeyVisibility() {
    const input = document.getElementById('settingsApiKey');
    input.type = input.type === 'password' ? 'text' : 'password';
}

function toggleWizardApiKeyVisibility() {
    const input = document.getElementById('wizardApiKey');
    input.type = input.type === 'password' ? 'text' : 'password';
}

function dismissSetupWizard() {
    document.getElementById('setupWizard').classList.remove('show');
}

async function saveSetupWizard() {
    const apiKey = document.getElementById('wizardApiKey').value.trim();
    const baseUrl = document.getElementById('wizardBaseUrl').value.trim();
    const model = document.getElementById('wizardModel').value.trim();

    if (!apiKey && !baseUrl.match(/^http:\/\/localhost/)) {
        document.getElementById('wizardConfigStatus').textContent = '⚠️ 请输入 API Key';
        document.getElementById('wizardConfigStatus').style.color = '#f59e0b';
        return;
    }

    document.getElementById('wizardConfigStatus').textContent = '⏳ 保存中...';
    document.getElementById('wizardConfigStatus').style.color = 'var(--text-secondary)';

    const result = await saveConfigToBackend(apiKey, baseUrl, model);
    if (result.success) {
        currentApiKey = apiKey;
        currentBaseUrl = baseUrl;
        currentModel = model;
        populateModelSelector(model);
        document.getElementById('wizardConfigStatus').textContent = '✅ 配置已保存，准备就绪！';
        document.getElementById('wizardConfigStatus').style.color = '#22c55e';
        setTimeout(() => {
            document.getElementById('setupWizard').classList.remove('show');
            updateConnectionStatus(true);
        }, 600);
    } else {
        document.getElementById('wizardConfigStatus').textContent = '❌ 保存失败，请重试';
        document.getElementById('wizardConfigStatus').style.color = '#ef4444';
    }
}

document.addEventListener('click', (e) => {
    if (e.target === document.getElementById('settingsModal')) closeSettings();
    if (e.target === document.getElementById('setupWizard')) dismissSetupWizard();
});