#!/usr/bin/env python3
"""Download the AllMiniLML6V2 ONNX model and set up the HF hub cache structure."""

import hashlib
import os
import sys
import shutil
import urllib.request
import time

BASE_URL = "https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/main"
FILES = [
    "model.onnx",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
]


def download_file(url, dest_path, timeout=120):
    """Download a file with retry logic."""
    max_retries = 3
    for attempt in range(max_retries):
        try:
            print(f"  Downloading {os.path.basename(dest_path)}...")
            req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
            with urllib.request.urlopen(req, timeout=timeout) as response:
                total_size = int(response.headers.get("Content-Length", 0))
                downloaded = 0
                chunk_size = 8192 * 64
                with open(dest_path + ".tmp", "wb") as f:
                    while True:
                        chunk = response.read(chunk_size)
                        if not chunk:
                            break
                        f.write(chunk)
                        downloaded += len(chunk)
                        if total_size > 0:
                            percent = int(downloaded * 100 / total_size)
                            if percent % 25 == 0 or downloaded == total_size:
                                print(f"    Progress: {downloaded / 1024 / 1024:.1f}/{total_size / 1024 / 1024:.1f} MB ({percent}%)")
                os.replace(dest_path + ".tmp", dest_path)
                file_size = os.path.getsize(dest_path)
                print(f"    Done ({file_size / 1024 / 1024:.1f} MB)")
                return True
        except Exception as e:
            print(f"    Attempt {attempt + 1}/{max_retries} failed: {e}")
            if os.path.exists(dest_path + ".tmp"):
                os.remove(dest_path + ".tmp")
            if attempt < max_retries - 1:
                time.sleep(2)
    return False


def main():
    if len(sys.argv) < 2:
        print("Usage: python download_model.py <output_cache_dir>")
        sys.exit(1)

    output_dir = sys.argv[1]
    os.makedirs(output_dir, exist_ok=True)

    model_cache_dir = os.path.join(output_dir, "models--Qdrant--all-MiniLM-L6-v2-onnx")
    blobs_dir = os.path.join(model_cache_dir, "blobs")
    refs_dir = os.path.join(model_cache_dir, "refs")
    os.makedirs(blobs_dir, exist_ok=True)
    os.makedirs(refs_dir, exist_ok=True)

    print(f"Downloading model files to {model_cache_dir}...\n")

    # Download all files
    downloaded = {}
    for filename in FILES:
        url = f"{BASE_URL}/{filename}"
        dest_path = os.path.join(blobs_dir, filename)
        print(f"  Target: {url}")
        if download_file(url, dest_path):
            downloaded[filename] = dest_path
        else:
            print(f"  FAILED to download {filename}")
            sys.exit(1)

    # Compute the commit SHA from model.onnx
    model_onnx_path = downloaded["model.onnx"]
    sha256_hash = hashlib.sha256()
    with open(model_onnx_path, "rb") as f:
        while True:
            chunk = f.read(4096)
            if not chunk:
                break
            sha256_hash.update(chunk)
    commit_sha = sha256_hash.hexdigest()
    print(f"\nComputed commit SHA: {commit_sha}")

    # Write refs/main
    refs_main_path = os.path.join(refs_dir, "main")
    with open(refs_main_path, "w") as f:
        f.write(commit_sha)
    print(f"Wrote {refs_main_path}")

    # Create snapshot directory
    snapshot_dir = os.path.join(model_cache_dir, "snapshots", commit_sha)
    os.makedirs(snapshot_dir, exist_ok=True)

    # Copy files to snapshot directory
    for filename, src_path in downloaded.items():
        dest_path = os.path.join(snapshot_dir, filename)
        if not os.path.exists(dest_path):
            shutil.copy2(src_path, dest_path)
            print(f"  Copied {filename} to snapshot")

    print(f"\nAll files downloaded and cached at: {model_cache_dir}")
    print(f"Snapshot directory: {snapshot_dir}")


if __name__ == "__main__":
    main()