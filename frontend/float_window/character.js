class VirtualCharacter {
    constructor(canvas, spriteAnimation) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        this.sprite = spriteAnimation;
        this.width = spriteAnimation.width;
        this.height = spriteAnimation.height;

        this.state = 'idle';
        this.prevState = 'idle';
        this.breathPhase = 0;
        this.clickCount = 0;
        this.lastClickTime = 0;

        this.onStateChange = null;
        this.onDoubleClick = null;

        this._returnToIdleTimer = null;
    }

    handleClick() {
        const now = Date.now();
        if (now - this.lastClickTime < 300) {
            this.clickCount++;
            if (this.clickCount >= 2) {
                this.setState('wave');
                this.clickCount = 0;
                if (this.onDoubleClick) this.onDoubleClick();
                this._scheduleReturnToIdle();
            }
        } else {
            this.clickCount = 1;
            this.setState('click');
            this._scheduleReturnToIdle();
        }
        this.lastClickTime = now;
    }

    _scheduleReturnToIdle() {
        if (this._returnToIdleTimer) {
            clearTimeout(this._returnToIdleTimer);
        }
        this._returnToIdleTimer = setTimeout(() => {
            this._returnToIdleTimer = null;
            if (this.state !== 'idle') {
                this.state = 'idle';
                this.sprite.play('idle', true);
                if (this.onStateChange) this.onStateChange('idle');
            }
        }, 5000);
    }

    setState(newState) {
        if (newState === this.state) return;

        if (this._returnToIdleTimer) {
            clearTimeout(this._returnToIdleTimer);
            this._returnToIdleTimer = null;
        }

        this.prevState = this.state;
        this.state = newState;

        const loopStates = { idle: true, thinking: true };
        const isLoop = loopStates[newState] || false;

        if (!isLoop) {
            this.sprite.play(newState, false, () => {
                if (this.state === newState) {
                    if (newState === 'talking') {
                        this.setState('click');
                    } else if (newState === 'click') {
                        this.state = 'idle';
                        this.sprite.play('idle', true);
                        if (this.onStateChange) this.onStateChange('idle');
                    } else {
                        this.state = 'idle';
                        this.sprite.play('idle', true);
                        if (this.onStateChange) this.onStateChange('idle');
                    }
                }
            });
        } else {
            this.sprite.play(newState, true);
        }
        if (this.onStateChange) this.onStateChange(newState);
    }

    setMousePosition(x, y) {
        // 保留用于后续眼球跟随功能
    }
}
