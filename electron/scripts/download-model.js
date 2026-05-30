const fs = require('fs');
const path = require('path');
const https = require('https');
const { createGunzip } = require('zlib');
const { Extract } = require('tar-stream');
const { pipeline } = require('stream/promises');

const MODEL_CACHE_DIR = path.join(__dirname, '..', '..', 'model_cache');
const HF_HOME = MODEL_CACHE_DIR;
const CACHE_DIR = path.join(HF_HOME, 'hub', 'models--Qdrant--all-MiniLM-L6-v2-onnx');

// Model files to download (from HuggingFace)
const FILES = [
    'model.onnx',
    'config.json',
    'tokenizer.json',
    'tokenizer_config.json',
    'special_tokens_map.json',
];

const BASE_URL = (process.env.HF_ENDPOINT || 'https://huggingface.co') + '/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main';

function downloadFile(url, dest) {
    return new Promise((resolve, reject) => {
        console.log(`  Downloading: ${path.basename(dest)}`);
        const file = fs.createWriteStream(dest);
        https.get(url, { timeout: 60000 }, (response) => {
            if (response.statusCode !== 200) {
                reject(new Error(`HTTP ${response.statusCode} for ${url}`));
                return;
            }
            const total = parseInt(response.headers['content-length'] || '0', 10);
            let downloaded = 0;
            response.on('data', (chunk) => {
                downloaded += chunk.length;
                if (total > 0) {
                    const pct = (downloaded / total * 100).toFixed(1);
                    process.stdout.write(`\r    ${(downloaded/1024/1024).toFixed(1)}MB / ${(total/1024/1024).toFixed(1)}MB (${pct}%)`);
                }
            });
            pipeline(response, file).then(() => {
                process.stdout.write('\n');
                resolve();
            }).catch(reject);
        }).on('error', reject);
    });
}

async function ensureModelDir() {
    const snapshotsDir = path.join(CACHE_DIR, 'snapshots');
    const refsDir = path.join(CACHE_DIR, 'refs');

    // Create directory structure
    fs.mkdirSync(snapshotsDir, { recursive: true });
    fs.mkdirSync(refsDir, { recursive: true });

    // Get the hash from the snapshot directory
    // We'll use a fixed commit hash that corresponds to the model version
    const commitHash = 'bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5';
    const modelDir = path.join(snapshotsDir, commitHash);
    fs.mkdirSync(modelDir, { recursive: true });

    // Write refs/main pointing to the commit hash
    fs.writeFileSync(path.join(refsDir, 'main'), commitHash);
    fs.writeFileSync(path.join(refsDir, 'HEAD'), `ref: refs/heads/main\n`);

    // Download model files
    for (const file of FILES) {
        const filePath = path.join(modelDir, file);
        if (fs.existsSync(filePath)) {
            const stats = fs.statSync(filePath);
            console.log(`  ${file} already exists (${(stats.size/1024/1024).toFixed(1)} MB)`);
            continue;
        }
        const url = `${BASE_URL}/${file}`;
        try {
            await downloadFile(url, filePath);
        } catch (err) {
            console.error(`  Failed to download ${file}: ${err.message}`);
        }
    }

    // Verify
    console.log('\nDownload complete!');
    for (const file of FILES) {
        const filePath = path.join(modelDir, file);
        if (fs.existsSync(filePath)) {
            const stats = fs.statSync(filePath);
            console.log(`  ✓ ${file} (${(stats.size/1024/1024).toFixed(1)} MB)`);
        } else {
            console.log(`  ✗ ${file} not found`);
        }
    }
}

console.log('=== Downloading Embedding Model ===');
console.log(`Model cache dir: ${MODEL_CACHE_DIR}`);
ensureModelDir().catch(err => {
    console.error('Download failed:', err.message);
    process.exit(1);
});