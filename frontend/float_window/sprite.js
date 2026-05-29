class SpriteAnimation {
    constructor(canvas, options = {}) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        this.width = options.width || 120;
        this.height = options.height || 160;
        this.defaultFps = options.fps || 12;
        this.resourcePath = options.resourcePath || '';

        canvas.width = this.width * (window.devicePixelRatio || 1);
        canvas.height = this.height * (window.devicePixelRatio || 1);
        canvas.style.width = this.width + 'px';
        canvas.style.height = this.height + 'px';
        this.ctx.scale(window.devicePixelRatio || 1, window.devicePixelRatio || 1);

        this.states = new Map();
        this.currentState = null;
        this.currentFrame = 0;
        this.frameCount = 0;
        this.isPlaying = false;
        this.isLooping = false;
        this.frameInterval = 1000 / this.defaultFps;
        this.lastFrameTime = 0;
        this.animId = null;
        this.onComplete = null;
        this.loadedImages = new Map();
        this.pendingLoads = 0;
        this.onAllLoaded = null;

        this.mainLoop = this.mainLoop.bind(this);
    }

    addState(name, config) {
        this.states.set(name, {
            frames: config.frames || [],
            images: config.images || [],
            fps: config.fps || this.defaultFps,
            loop: config.loop || false,
            totalFrames: config.totalFrames || 0,
            offsetX: config.offsetX || 0,
            offsetY: config.offsetY || 0,
        });
    }

    _normalizePath(p) {
        if (p.startsWith('file://') || p.startsWith('http://') || p.startsWith('https://')) return p;
        if (/^[A-Za-z]:[\\/]/.test(p)) {
            const normalized = p.replace(/\\/g, '/');
            return 'file:///' + normalized;
        }
        if (p.startsWith('/')) return 'file://' + p;
        return p;
    }

    addStateWithFrames(name, frameDir, frameCount, config = {}) {
        const frames = [];
        const images = [];
        for (let i = 0; i < frameCount; i++) {
            const num = String(i + 1).padStart(4, '0');
            const rawPath = `${this.resourcePath}${frameDir}/${num}.png`;
            const path = this._normalizePath(rawPath);
            frames.push(path);
            images.push(null);
        }
        this.states.set(name, {
            frames,
            images,
            fps: config.fps || this.defaultFps,
            loop: config.loop || false,
            totalFrames: config.totalFrames || 0,
            offsetX: config.offsetX || 0,
            offsetY: config.offsetY || 0,
        });
    }

    loadAll(onComplete) {
        this.onAllLoaded = onComplete;
        this.pendingLoads = 0;

        for (const [name, state] of this.states) {
            for (let i = 0; i < state.frames.length; i++) {
                const path = state.frames[i];
                if (this.loadedImages.has(path)) {
                    state.images[i] = this.loadedImages.get(path);
                    continue;
                }
                this.pendingLoads++;
                const img = new Image();
                img.onload = () => {
                    this.loadedImages.set(path, img);
                    state.images[i] = img;
                    this.pendingLoads--;
                    if (this.pendingLoads <= 0 && this.onAllLoaded) {
                        this.onAllLoaded();
                    }
                };
                img.onerror = () => {
                    console.warn(`[SpriteAnimation] 加载失败: ${path} (单帧模式将使用现有帧)`);
                    this.pendingLoads--;
                    if (this.pendingLoads <= 0 && this.onAllLoaded) {
                        this.onAllLoaded();
                    }
                };
                img.src = path;
            }
        }

        if (this.pendingLoads === 0 && this.onAllLoaded) {
            this.onAllLoaded();
        }
    }

    play(stateName, loop = false, onComplete = null) {
        if (!this.states.has(stateName)) {
            console.warn(`[SpriteAnimation] 未知状态: ${stateName}`);
            return;
        }

        const state = this.states.get(stateName);
        
        // 检查是否有可用的图片帧
        const availableImages = state.images.filter(img => img !== null);
        if (availableImages.length === 0) {
            console.warn(`[SpriteAnimation] 状态 ${stateName} 没有可用图片帧`);
            return;
        }

        // 如果只有部分帧加载成功，调整帧计数
        this.frameCount = availableImages.length;
        
        this.currentState = stateName;
        this.currentFrame = 0;
        this.isLooping = loop || this.frameCount <= 1; // 单帧自动循环
        this.onComplete = onComplete || null;

        // 每张图片停留2秒后切换
        this.frameInterval = 2000;

        // 单帧模式：添加呼吸微动效果
        this._enableBreathingEffect = this.frameCount <= 1;

        if (!this.isPlaying) {
            this.isPlaying = true;
            this.lastFrameTime = performance.now();
            this.animId = requestAnimationFrame(this.mainLoop);
        }
    }

    stop() {
        this.isPlaying = false;
        if (this.animId) {
            cancelAnimationFrame(this.animId);
            this.animId = null;
        }
    }

    mainLoop(timestamp) {
        if (!this.isPlaying) return;

        const elapsed = timestamp - this.lastFrameTime;

        if (elapsed >= this.frameInterval) {
            this.lastFrameTime = timestamp - (elapsed % this.frameInterval);

            const state = this.states.get(this.currentState);
            this.ctx.clearRect(0, 0, this.width, this.height);

            // 获取当前帧的图片，如果缺失则使用第一帧
            let currentImg = state.images[this.currentFrame];
            if (!currentImg && state.images.length > 0) {
                currentImg = state.images[0];
            }

            if (currentImg) {
                const ox = state.offsetX || 0;
                const oy = state.offsetY || 0;
                
                // 单帧模式：添加呼吸微动效果
                if (this._enableBreathingEffect && currentImg) {
                    const breatheTime = timestamp * 0.002;
                    const breatheOffset = Math.sin(breatheTime) * 1.5;
                    const breatheScale = 1 + Math.sin(breatheTime * 0.7) * 0.008;
                    
                    this.ctx.save();
                    this.ctx.translate(this.width / 2, this.height / 2 + breatheOffset);
                    this.ctx.scale(breatheScale, breatheScale);
                    this.ctx.translate(-this.width / 2, -this.height / 2);
                    this.ctx.drawImage(currentImg, ox, oy, this.width, this.height);
                    this.ctx.restore();
                } else {
                    this.ctx.drawImage(currentImg, ox, oy, this.width, this.height);
                }
            }

            this.currentFrame++;

            if (this.currentFrame >= this.frameCount) {
                if (this.isLooping) {
                    this.currentFrame = 0;
                } else {
                    this.currentFrame = this.frameCount - 1;
                    this.isPlaying = false;
                    if (this.onComplete) {
                        this.onComplete();
                    }
                    return;
                }
            }
        }

        this.animId = requestAnimationFrame(this.mainLoop);
    }

    getCurrentState() {
        return this.currentState;
    }
}
