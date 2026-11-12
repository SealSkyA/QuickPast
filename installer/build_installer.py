# -*- coding: utf-8 -*-
"""
QuickCopy Windows 安装包构建脚本（替代 tauri nsis 原生打包）
1. cargo build --release --features custom-protocol（生产构建，UI 内嵌）
2. 读取 src-tauri/tauri.conf.json package.version（三端统一版本）
3. 调 NSIS makensis 编译 installer/QuickCopy.nsi
4. 产物：dist-release/QuickCopy_<version>_x64-setup.exe
"""
import json, os, shutil, subprocess, sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC_T = os.path.join(REPO, "src-tauri")
INST = os.path.join(REPO, "installer")
DIST = os.path.join(REPO, "dist-release")

def find_makensis():
    """探测 makensis：环境变量 NSIS_PATH / PATH / tauri CLI 便携缓存"""
    cands = []
    nsis_path = os.environ.get("NSIS_PATH")
    if nsis_path:
        cands.append(os.path.join(nsis_path, "makensis.exe"))
    for p in os.environ.get("PATH", "").split(";"):
        p = p.strip()
        if p and p.lower().endswith("nsis"):
            cands.append(os.path.join(p, "makensis.exe"))
    tauri_cache = os.path.join(os.environ.get("LOCALAPPDATA", ""), "tauri", "NSIS", "makensis.exe")
    cands.append(tauri_cache)
    cands.append(r"C:\Program Files (x86)\NSIS\makensis.exe")
    for c in cands:
        if os.path.exists(c):
            return c
    return None

def version():
    with open(os.path.join(SRC_T, "tauri.conf.json"), encoding="utf-8") as f:
        return json.load(f)["package"]["version"]

def main():
    makensis = find_makensis()
    if not makensis:
        print("NO MAKENSIS: 未找到 NSIS 编译器（可安装 NSIS 或用 Tauri 缓存）。退出")
        sys.exit(1)
    print("makensis:", makensis)

    print("==> npx tauri build --features custom-protocol (UI 资源嵌入)")
    # 用 tauri CLI 而非 cargo build：tauri-build 的增量缓存不会把 ui/ 内容变化
    # 反映到复用产物里（cargo build 增量可能带上旧 UI），CLI 保证同步嵌入最新 index.html。
    # 产物 target/release/QuickCopy.exe 供下方 makensis 打自定义安装包。
    r = subprocess.run(
        ["npx", "tauri", "build", "--features", "custom-protocol"],
        cwd=REPO,
    )
    if r.returncode != 0:
        print("cargo build 失败"); sys.exit(1)

    ver = version()
    os.makedirs(DIST, exist_ok=True)
    out = os.path.join(DIST, "QuickCopy_%s_x64-setup.exe" % ver)
    print("==> makensis -DVERSION=%s QuickCopy.nsi" % ver)
    r = subprocess.run(
        [makensis, "-DVERSION=%s" % ver, os.path.join(INST, "QuickCopy.nsi")],
        cwd=INST,
    )
    if r.returncode != 0:
        print("makensis 失败"); sys.exit(1)
    exe = os.path.join(REPO, "src-tauri", "target", "release", "QuickCopy.exe")
    print("==> exe:", os.path.getsize(exe), "bytes")
    if os.path.exists(out):
        print("==> 安装包:", out, os.path.getsize(out), "bytes")
    else:
        print("==> 安装包未生成！", out); sys.exit(1)

if __name__ == "__main__":
    main()