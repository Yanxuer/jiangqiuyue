const fs = require('fs');
const path = require('path');

const projectRoot = path.join(__dirname, '..');
const src = path.join(projectRoot, '..', 'frontend', 'dist');
const dest = path.join(projectRoot, 'frontend-dist');

console.log(`src: ${src}`);
console.log(`dest: ${dest}`);
console.log(`src exists: ${fs.existsSync(src)}`);

console.log('Copying frontend dist...');

if (fs.existsSync(dest)) {
    fs.rmSync(dest, { recursive: true });
}

function copyDir(srcDir, destDir) {
    fs.mkdirSync(destDir, { recursive: true });
    const items = fs.readdirSync(srcDir);
    for (const item of items) {
        const srcPath = path.join(srcDir, item);
        const destPath = path.join(destDir, item);
        if (fs.statSync(srcPath).isDirectory()) {
            copyDir(srcPath, destPath);
        } else {
            fs.copyFileSync(srcPath, destPath);
            console.log(`  ${item}`);
        }
    }
}

copyDir(src, dest);

// 复制非模块 JS 文件（Vite 不处理这些直接 <script src="..."> 引用的文件）
const jsFiles = {
    main: ['app.js'],
    float: ['sprite.js', 'character.js'],
};

for (const [window, files] of Object.entries(jsFiles)) {
    const windowSrc = path.join(projectRoot, '..', 'frontend', `${window}_window`);
    const windowDest = path.join(dest, `${window}_window`);
    for (const file of files) {
        const srcFile = path.join(windowSrc, file);
        const destFile = path.join(windowDest, file);
        if (fs.existsSync(srcFile)) {
            fs.copyFileSync(srcFile, destFile);
            console.log(`  ${window}/${file}`);
        } else {
            console.warn(`  Warning: ${window}/${file} not found at ${srcFile}`);
        }
    }
}

console.log('Done!');
