const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const projectRoot = path.join(__dirname, '..');
const backendRustDir = path.join(projectRoot, '..', 'backend-rust');
const cargoPath = path.join(process.env.USERPROFILE, '.cargo', 'bin', 'cargo.exe');
const rustDistDir = path.join(projectRoot, 'rust-dist');

console.log('=== Building Rust backend ===');
console.log(`cargo: ${cargoPath}`);
console.log(`backend-rust dir: ${backendRustDir}`);

if (!fs.existsSync(cargoPath)) {
    console.error('cargo not found at:', cargoPath);
    console.error('Please install Rust first');
    process.exit(1);
}

console.log('Running cargo build --release...');
execSync(`"${cargoPath}" build --release --manifest-path "${path.join(backendRustDir, 'Cargo.toml')}"`, {
    stdio: 'inherit',
    cwd: backendRustDir,
});

console.log('Copying to rust-dist...');

const destDir = path.join(rustDistDir, 'backend');
fs.mkdirSync(destDir, { recursive: true });

// Copy binary
const srcExe = path.join(backendRustDir, 'target', 'release', 'backend-server.exe');
const destExe = path.join(destDir, 'backend.exe');
if (fs.existsSync(srcExe)) {
    fs.copyFileSync(srcExe, destExe);
    const stats = fs.statSync(destExe);
    console.log(`  backend.exe (${(stats.size / 1024 / 1024).toFixed(1)} MB)`);
} else {
    console.error('ERROR: backend-server.exe not found at', srcExe);
    process.exit(1);
}

// Copy .env
const srcEnv = path.join(backendRustDir, '.env');
const destEnv = path.join(rustDistDir, '.env');
if (fs.existsSync(srcEnv)) {
    fs.copyFileSync(srcEnv, destEnv);
    console.log('  .env');
} else {
    console.warn('Warning: .env not found, backend may not have API key');
}

// Copy memory_db
const srcMemory = path.join(projectRoot, 'memory_db');
const destMemory = path.join(rustDistDir, 'memory_db');
if (fs.existsSync(srcMemory)) {
    function copyDir(src, dest) {
        fs.mkdirSync(dest, { recursive: true });
        const items = fs.readdirSync(src);
        for (const item of items) {
            const srcPath = path.join(src, item);
            const destPath = path.join(dest, item);
            if (fs.statSync(srcPath).isDirectory()) {
                copyDir(srcPath, destPath);
            } else {
                fs.copyFileSync(srcPath, destPath);
            }
        }
    }
    copyDir(srcMemory, destMemory);
    console.log('  memory_db/');
} else {
    console.warn('Warning: memory_db not found');
}

// Copy model cache (pre-downloaded embedding model)
const srcModelCache = path.join(__dirname, '..', 'rust-dist', 'model_cache');
const destModelCache = path.join(rustDistDir, 'model_cache');
if (fs.existsSync(srcModelCache)) {
    function copyDir(src, dest) {
        fs.mkdirSync(dest, { recursive: true });
        const items = fs.readdirSync(src);
        for (const item of items) {
            const srcPath = path.join(src, item);
            const destPath = path.join(dest, item);
            if (fs.statSync(srcPath).isDirectory()) {
                copyDir(srcPath, destPath);
            } else {
                fs.copyFileSync(srcPath, destPath);
            }
        }
    }
    copyDir(srcModelCache, destModelCache);
    console.log('  model_cache/');
} else {
    console.warn('Warning: model_cache not found at', srcModelCache);
}

console.log('=== Rust backend build complete ===');