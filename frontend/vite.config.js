import { defineConfig } from 'vite';
import { resolve } from 'path';

export default defineConfig({
    root: '.',
    base: './',
    build: {
        outDir: 'dist',
        rollupOptions: {
            input: {
                main: resolve(__dirname, 'main_window/index.html'),
                float: resolve(__dirname, 'float_window/index.html')
            }
        },
        emptyOutDir: true
    },
    server: {
        port: 5173,
        fs: {
            allow: ['..']
        },
        proxy: {
            '/api': 'http://localhost:8000',
            '/ws': {
                target: 'ws://localhost:8000',
                ws: true
            }
        }
    }
});