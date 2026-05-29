const { app, BrowserWindow, ipcMain, screen, Tray, Menu, nativeImage } = require('electron');
const path = require('path');
const fs = require('fs');
const { spawn } = require('child_process');
const http = require('http');

let mainWindow = null;
let floatWindow = null;
let tray = null;
let backendProcess = null;
let backendProcessExited = false;

/* ==================== 文件日志系统 ==================== */
const LOG_FILE = path.join(app.getPath('userData'), 'ai-agent-debug.log');
const LOG_SESSION = Date.now();

function log(...args) {
    const time = new Date().toISOString().slice(11, 23);
    const msg = `[${time}][${LOG_SESSION}] ${args.join(' ')}`;
    console.log(msg);
    try {
        fs.appendFileSync(LOG_FILE, msg + '\n');
    } catch(e) {
        console.error('Log write error:', e);
    }
}

function logError(...args) {
    const time = new Date().toISOString().slice(11, 23);
    const msg = `[${time}][${LOG_SESSION}][ERROR] ${args.join(' ')}`;
    console.error(msg);
    try {
        fs.appendFileSync(LOG_FILE, msg + '\n');
    } catch(e) {
        console.error('Log write error:', e);
    }
}

log('=== AI Agent 启动 ===');
log('app.isPackaged:', app.isPackaged);
log('__dirname:', __dirname);
log('userData:', app.getPath('userData'));
log('LOG_FILE:', LOG_FILE);

// 轮询后端健康检查（不超过 maxRetries 次，interval 毫秒间隔）
function waitForBackend(url, maxRetries = 60, interval = 3000) {
    return new Promise((resolve) => {
        let attempts = 0;
        const check = () => {
            attempts++;
            // 先检查后端进程是否还在运行
            if (backendProcessExited) {
                logError('[后端检查] 后端进程已退出');
                resolve(false);
                return;
            }
            const start = Date.now();
            const req = http.get(url, (res) => {
                const elapsed = Date.now() - start;
                log(`[后端检查] 尝试 ${attempts}/${maxRetries} status=${res.statusCode} 耗时=${elapsed}ms`);
                if (res.statusCode === 200) {
                    log('[后端检查] 后端就绪！');
                    resolve(true);
                } else {
                    retry();
                }
            });
            req.on('error', (err) => {
                log(`[后端检查] 尝试 ${attempts}/${maxRetries} 错误: ${err.code || err.message}`);
                retry();
            });
            req.setTimeout(2000, () => {
                log(`[后端检查] 尝试 ${attempts}/${maxRetries} 超时`);
                req.destroy();
                retry();
            });
            
            function retry() {
                if (attempts >= maxRetries) {
                    logError(`[后端检查] 已用尽 ${maxRetries} 次尝试，后端启动失败`);
                    resolve(false);
                } else {
                    setTimeout(check, interval);
                }
            }
        };
        log('[后端检查] 将在 1500ms 后开始首次检查');
        setTimeout(check, 1500);
    });
}

// 启动 Rust 后端服务
function startRustBackend() {
    const isPackaged = app.isPackaged;
    let backendExePath, cwd;
    if (isPackaged) {
        backendExePath = path.join(process.resourcesPath, 'backend-dist', 'backend', 'backend.exe');
        cwd = path.join(process.resourcesPath, 'backend-dist');
    } else {
        backendExePath = path.join(__dirname, 'rust-dist', 'backend', 'backend.exe');
        cwd = path.join(__dirname, 'rust-dist');
    }

    log(`[后端启动] 模式: ${isPackaged ? 'production' : 'development'}`);
    log(`[后端启动] 路径: ${backendExePath}`);
    log(`[后端启动] cwd: ${cwd}`);
    log(`[后端启动] 文件存在: ${fs.existsSync(backendExePath)}`);

    if (isPackaged) {
        const resDir = process.resourcesPath;
        log(`[后端启动] resourcesPath: ${resDir}`);
        try {
            const listing = fs.readdirSync(resDir);
            log(`[后端启动] resources 内容: ${listing.join(', ')}`);
            const backendDir = path.join(resDir, 'backend-dist');
            if (fs.existsSync(backendDir)) {
                log(`[后端启动] backend-dist 内容: ${fs.readdirSync(backendDir).join(', ')}`);
                const backendExeDir = path.join(backendDir, 'backend');
                if (fs.existsSync(backendExeDir)) {
                    log(`[后端启动] backend/ 内容: ${fs.readdirSync(backendExeDir).join(', ')}`);
                }
            }
        } catch(e) {
            logError(`[后端启动] 无法读取 resources: ${e.message}`);
        }
    }

    log('[后端启动] 使用 spawn 启动 Rust 后端');
    backendProcess = spawn(backendExePath, [], {
        env: {
            ...process.env,
            MEMORY_PATH: path.join(cwd, 'memory_db'),
            WORKSPACE: path.join(cwd, 'workspace'),
            HF_HOME: path.join(cwd, 'model_cache'),
            FASTEMBED_CACHE_DIR: path.join(cwd, 'model_cache', 'models--Qdrant--all-MiniLM-L6-v2-onnx'),
        },
        cwd: cwd,
        detached: false,
        stdio: ['ignore', 'pipe', 'pipe']
    });

    log('[后端启动] 进程 PID:', backendProcess.pid);

    let stdoutBuffer = '';
    let stderrBuffer = '';

    backendProcess.stdout.on('data', (data) => {
        const text = data.toString();
        stdoutBuffer += text;
        log(`[backend stdout] ${text.trim()}`);
    });

    backendProcess.stderr.on('data', (data) => {
        const text = data.toString();
        stderrBuffer += text;
        log(`[backend stderr] ${text.trim()}`);
    });

    backendProcess.on('error', (err) => {
        logError(`[后端启动] 进程错误: ${err.code} ${err.message}`);
    });

    backendProcess.on('exit', (code, signal) => {
        backendProcessExited = true;
        log(`[后端启动] 进程退出 code=${code} signal=${signal}`);
        log(`[后端启动] stdout 共 ${stdoutBuffer.length} 字符, stderr 共 ${stderrBuffer.length} 字符`);
        if (stderrBuffer.length > 0) {
            logError(`[后端启动] stderr 完整输出:\n${stderrBuffer}`);
        }
    });

    log('[后端启动] 开始等待后端健康检查...');
    return waitForBackend('http://127.0.0.1:8000/health');
}

// 创建主窗口
function createMainWindow() {
    log('[主窗口] 开始创建...');
    
    mainWindow = new BrowserWindow({
        width: 1400,
        height: 900,
        minWidth: 1000,
        minHeight: 700,
        show: false,
        backgroundColor: '#0c1624',
        titleBarStyle: 'hiddenInset',
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            preload: path.join(__dirname, 'preload.js'),
        },
        icon: path.join(__dirname, '../resources/icons/icon.png')
    });

    log('[主窗口] BrowserWindow 实例创建完成');

    const isDev = !app.isPackaged;
    if (isDev) {
        log('[主窗口] 开发模式: loadURL');
        mainWindow.loadURL('http://localhost:5173/main_window/index.html');
        mainWindow.webContents.openDevTools({ mode: 'right' });
    } else {
        log('[主窗口] 生产模式: 使用 extraResources 路径');
        const indexPath = path.join(process.resourcesPath, 'frontend-dist', 'main_window', 'index.html');
        log(`[主窗口] loadFile = ${indexPath}`);
        log(`[主窗口] 文件存在: ${require('fs').existsSync(indexPath)}`);
        try {
            mainWindow.loadFile(indexPath);
            log('[主窗口] loadFile 调用成功');
        } catch(e) {
            logError(`[主窗口] loadFile 失败: ${e.message}`);
        }
    }

    // 捕获渲染进程的控制台日志（写入主进程日志文件）
    mainWindow.webContents.on('console-message', (event, level, message, line, sourceId) => {
        const levelMap = { 0: 'verbose', 1: 'info', 2: 'warn', 3: 'error' };
        const levelName = levelMap[level] || 'unknown';
        if (level === 3) {
            logError(`[renderer console] ${message}`);
        } else {
            log(`[renderer console] ${message}`);
        }
    });

    mainWindow.once('ready-to-show', () => {
        log('[主窗口] ready-to-show 事件触发');
        mainWindow.show();
        mainWindow.focus();
        // 延迟注入 JavaScript 来检查页面状态
        mainWindow.webContents.executeJavaScript(`
            console.log('[App] 页面加载完成检查:');
            console.log('[App] document.body.innerHTML.length:', document.body.innerHTML.length);
            console.log('[App] document.title:', document.title);
            console.log('[App] loadingScreen exists:', !!document.getElementById('loadingScreen'));
            console.log('[App] appContainer exists:', !!document.getElementById('appContainer'));
            console.log('[App] connectingOverlay exists:', !!document.getElementById('connectingOverlay'));
        `).catch(err => {
            logError(`[主窗口] executeJavaScript 失败: ${err.message}`);
        });
    });

    // 兜底：5秒后强行显示
    setTimeout(() => {
        if (mainWindow && !mainWindow.isVisible()) {
            log('[主窗口] 5秒兜底触发: 强行显示窗口');
            mainWindow.show();
        }
    }, 5000);

    mainWindow.webContents.on('did-finish-load', () => {
        log('[主窗口] did-finish-load 事件触发');
    });

    mainWindow.webContents.on('did-fail-load', (event, errorCode, errorDesc, validatedURL) => {
        logError(`[主窗口] did-fail-load: code=${errorCode} desc=${errorDesc} url=${validatedURL}`);
    });

    mainWindow.on('closed', () => {
        log('[主窗口] closed 事件触发');
        mainWindow = null;
    });
}

// 创建悬浮窗（虚拟形象窗口）
function createFloatWindow() {
    log('[悬浮窗] 开始创建...');
    const { width, height } = screen.getPrimaryDisplay().workAreaSize;

    const isDev = !app.isPackaged;

    floatWindow = new BrowserWindow({
        width: 130,
        height: 180,
        x: width - 150,
        y: height - 200,
        frame: false,
        transparent: true,
        alwaysOnTop: true,
        skipTaskbar: true,
        resizable: false,
        movable: true,
        hasShadow: false,
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            preload: path.join(__dirname, 'preload_float.js'),
            offscreen: false,
            webSecurity: isDev ? false : true,
        }
    });

    if (isDev) {
        floatWindow.loadURL('http://localhost:5173/float_window/index.html');
    } else {
        const indexPath = path.join(process.resourcesPath, 'frontend-dist', 'float_window', 'index.html');
        log(`[悬浮窗] loadFile = ${indexPath}`);
        log(`[悬浮窗] 文件存在: ${fs.existsSync(indexPath)}`);
        floatWindow.loadFile(indexPath);
    }

    floatWindow.webContents.on('did-finish-load', () => {
        log('[悬浮窗] did-finish-load');
        floatWindow.webContents.send('float-window-ready');
    });
    
    floatWindow.hide();
    log('[悬浮窗] 初始已隐藏');

    floatWindow.on('closed', () => {
        log('[悬浮窗] closed');
        floatWindow = null;
    });
}

// 系统托盘
function createTray() {
    log('[托盘] 开始创建...');
    const iconPath = path.join(__dirname, '../resources/icons/tray.png');
    log(`[托盘] 图标路径: ${iconPath}`);
    const icon = nativeImage.createFromPath(iconPath).resize({ width: 16, height: 16 });
    
    tray = new Tray(icon);
    const contextMenu = Menu.buildFromTemplate([
        { label: '显示主窗口', click: () => { log('[托盘] 显示主窗口'); mainWindow?.show(); } },
        { label: '显示/隐藏悬浮窗', click: () => {
            if (floatWindow?.isVisible()) {
                log('[托盘] 隐藏悬浮窗');
                floatWindow.hide();
            } else {
                log('[托盘] 显示悬浮窗');
                floatWindow?.show();
            }
        }},
        { type: 'separator' },
        { label: '退出', click: () => { log('[托盘] 退出应用'); app.quit(); }}
    ]);
    
    tray.setToolTip('AI Agent');
    tray.setContextMenu(contextMenu);
    
    tray.on('click', () => {
        if (mainWindow?.isVisible()) {
            mainWindow.hide();
        } else {
            mainWindow?.show();
        }
    });
    log('[托盘] 创建完成');
}

// IPC 通信
ipcMain.handle('renderer-log', (event, level, ...args) => {
    const prefix = level === 'error' ? '[渲染进程错误]' : '[渲染进程]';
    if (level === 'error') {
        logError(prefix, ...args);
    } else {
        log(prefix, ...args);
    }
});

ipcMain.handle('window-control', (event, action) => {
    log(`[IPC] window-control: ${action}`);
    switch(action) {
        case 'show-main':
            mainWindow?.show();
            mainWindow?.focus();
            return true;
        case 'hide-main':
            mainWindow?.hide();
            return true;
        case 'hide-float':
            floatWindow?.hide();
            return true;
        case 'show-float':
            if (floatWindow) {
                floatWindow.show();
                floatWindow.setAlwaysOnTop(true);
                floatWindow.focus();
            }
            return true;
        case 'set-ignore-mouse':
            if (floatWindow) {
                floatWindow.setIgnoreMouseEvents(true, { forward: true });
            }
            return true;
        case 'set-mouse-normal':
            if (floatWindow) {
                floatWindow.setIgnoreMouseEvents(false);
            }
            return true;
    }
});

ipcMain.on('window-drag-start', () => {
    if (floatWindow) {
        const bounds = floatWindow.getBounds();
        floatWindow.setBounds(bounds);
    }
});

ipcMain.handle('window-drag-move', (event, dx, dy) => {
    if (floatWindow) {
        const bounds = floatWindow.getBounds();
        floatWindow.setBounds({
            x: bounds.x + Math.round(dx),
            y: bounds.y + Math.round(dy),
            width: bounds.width,
            height: bounds.height
        });
    }
});

ipcMain.handle('window-toggle-pin', (event, pinned) => {
    if (floatWindow) {
        floatWindow.setAlwaysOnTop(pinned);
    }
});

ipcMain.handle('get-resource-path', (event, relativePath) => {
    const basePath = app.isPackaged
        ? path.join(process.resourcesPath, 'resources')
        : path.join(__dirname, '../resources');
    const fullPath = path.join(basePath, relativePath);
    log(`[IPC] get-resource-path: ${relativePath} -> ${fullPath} (exists: ${fs.existsSync(fullPath)})`);
    return fullPath;
});

// 获取日志文件路径（供渲染进程读取）
ipcMain.handle('get-log-path', () => {
    return LOG_FILE;
});

// 选择文件夹对话框
ipcMain.handle('select-directory', async () => {
    const { dialog } = require('electron');
    const result = await dialog.showOpenDialog(mainWindow, {
        properties: ['openDirectory'],
        title: '选择文件夹'
    });
    if (!result.canceled && result.filePaths.length > 0) {
        return result.filePaths[0];
    }
    return null;
});

/* ==================== 应用主入口 ==================== */

app.whenReady().then(async () => {
    log('[App] app.whenReady() 触发');
    
    // ★★★ 关键修复：并行启动后端，不阻塞窗口创建！★★★
    log('[App] 并行启动后端 + 创建窗口');
    const backendPromise = startRustBackend().then(ready => {
        log(`[App] 后端启动结果: ${ready ? '就绪' : '超时/失败'}`);
        // 通知渲染进程
        if (mainWindow && !mainWindow.isDestroyed()) {
            mainWindow.webContents.send('backend-status', ready);
        }
    }).catch(err => {
        logError('[App] 后端启动异常:', err.message);
        if (mainWindow && !mainWindow.isDestroyed()) {
            mainWindow.webContents.send('backend-status', false);
        }
    });
    
    // 立即创建窗口（不等待后端！）
    createMainWindow();
    createFloatWindow();
    createTray();
    
    log('[App] 窗口创建完毕，后端正在后台启动...');
    
    app.on('activate', () => {
        if (BrowserWindow.getAllWindows().length === 0) {
            createMainWindow();
        }
    });
});

app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') {
        app.quit();
    }
});

app.on('before-quit', () => {
    log('[App] before-quit');
    if (backendProcess) {
        log('[App] 终止后端进程 PID:', backendProcess.pid);
        backendProcess.kill();
    }
});
