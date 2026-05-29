const fs = require('fs');
const path = require('path');

const distDir = path.join(__dirname, '..', 'dist');
const srcDir = path.join(__dirname, '..');

function copyStaticFiles(htmlDir, files) {
    const targetDir = path.join(distDir, htmlDir);
    if (!fs.existsSync(targetDir)) {
        fs.mkdirSync(targetDir, { recursive: true });
    }
    files.forEach(file => {
        const src = path.join(srcDir, htmlDir, file);
        const dest = path.join(targetDir, file);
        if (fs.existsSync(src)) {
            fs.copyFileSync(src, dest);
            console.log(`  ✓ ${htmlDir}${file}`);
        }
    });
}

function copyDirectory(src, dest) {
    if (!fs.existsSync(src)) return;
    if (!fs.existsSync(dest)) {
        fs.mkdirSync(dest, { recursive: true });
    }
    const items = fs.readdirSync(src);
    items.forEach(item => {
        const srcPath = path.join(src, item);
        const destPath = path.join(dest, item);
        if (fs.statSync(srcPath).isDirectory()) {
            copyDirectory(srcPath, destPath);
        } else {
            fs.copyFileSync(srcPath, destPath);
            console.log(`  ✓ resources/${item}`);
        }
    });
}

console.log('Copying static JS files...');
copyStaticFiles('main_window', ['app.js', 'style.css']);
copyStaticFiles('float_window', ['sprite.js', 'character.js', 'style.css']);

console.log('Copying character resources...');
const resourcesSrc = path.join(srcDir, 'resources', 'character');
const resourcesDest = path.join(distDir, 'resources', 'character');
copyDirectory(resourcesSrc, resourcesDest);

console.log('Done!');