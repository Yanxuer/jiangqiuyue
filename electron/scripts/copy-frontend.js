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
console.log('Done!');
