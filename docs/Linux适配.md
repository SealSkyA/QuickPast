# Linux (.deb) 适配文档

QuickCopy（原名 QuickPaste）桌面端目前正式发布平台为 Windows。本文件说明
银河麒麟 / UOS 等国产 Linux 发行版的适配现状、构建方法与已知差异。

## 1. 目标系统支持矩阵

| 系统 | 架构 | 能否构建/运行 | 说明 |
|---|---|---|---|
| 银河麒麟 V10 **SP1** | arm64 | ⚠️ 看源 | 若源内无 `libwebkit2gtk-4.1-dev` 则无法编译（见 §6） |
| 银河麒麟 V10 **SP2 及以上** | arm64 / x86_64 | ✅ | 源含 webkit2gtk-4.1，Tauri 2 可编译 |
| UOS 统信 UOS 20（1060 及以上） | arm64 / x86_64 | ✅ | 源含 webkit2gtk-4.1 |
| 银河麒麟 V10 SP1 | **loongarch64（龙芯）** | ❌ | 仅 webkit2gtk-4.0、glibc 2.28，Tauri 2 无法编译（见 §6） |

> 注意架构：龙芯 = `loongarch64`，与 ARM（`aarch64`）不同，产物互不兼容。

## 2. 一键构建（GitHub Actions，推荐）

仓库已配置 `.github/workflows/build-deb.yml`：

- 触发方式：
  - **打 tag**（`v*`）自动构建并把 `.deb` 挂到该 Release；
  - **手动**：Actions → build-deb-linux → Run workflow（ref 选 main）。
- Runner：`ubuntu-22.04-arm`（GitHub 免费公共 arm64 真机，glibc 2.35 + webkit2gtk-4.1）。
- 产物：`QuickCopy_<version>_arm64.deb`，以 artifact / Release 资产形式提供。
- **glibc 注意**：产物面向 glibc ≥ 2.35 的系统（麒麟 V10 SP2+ / UOS 1060+）。
  在更老的基础版本（如 V10 SP1）安装会报依赖缺失或运行段错误。

## 3. 目标机手动构建

```bash
# 1) 系统依赖
sudo apt update && sudo apt install -y build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev \
  libxdo-dev libssl-dev patchelf

# 2) Rust + Node
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt install -y nodejs npm

# 3) 拉代码并构建
git clone https://github.com/SealSkyA/QuickPast.git && cd QuickPast
npm install
npx tauri build --bundles deb
# 产物：src-tauri/target/release/bundle/deb/*.deb
```

## 4. 已完成的平台适配（代码层）

| 适配点 | 实现 |
|---|---|
| 数据目录 | Linux 走 XDG：`~/.local/share/quickcopy/`（deb 安装目录只读，不能放 exe 旁） |
| 开机自启 | Linux 写 `~/.config/autostart/QuickCopy.desktop`（XDG 标准，麒麟/UOS 通用） |
| 在线更新 | Linux 自动匹配 `.deb` 资产；`curl` 调用平台隔离（Windows 静默窗口参数仅 Windows） |
| user32 链接 | 无边框窗口鼠标缩放（`GetCursorPos`/`GetAsyncKeyState`）仅 Windows 编译，Linux 上该命令返回不支持 |
| 桌面路径 | 当前代码不依赖「桌面」路径；将来用到时读 `$HOME/.config/user-dirs.dirs` 的 `XDG_DESKTOP_DIR`（麒麟 `Desktop`、UOS `桌面` 自动识别） |

## 5. 与 Windows exe 的功能对照（目标：丝毫不差）

- 剪贴板历史 / 文本 · 图片 · 文件 · 便签四类条目
- 局域网发现（LocalSend v2 协议，UDP 53318 / HTTP 53318）与收发
- 设备收藏 / 在线状态 / 批量选择发送
- 托盘常驻、接收弹窗、自动保存 / 信任设备静默接收
- 设置项（主题、自启、端口、缓存清理等）与「关于 / 在线更新」
- 拖放文件/文件夹到窗口发送

**已知差异（Linux）**：
1. **无边框窗口边缘拖拽缩放**（Win32 鼠标跟踪）暂不支持，报「当前系统暂不支持」；
   可临时用系统窗口快捷键（Alt+F8/系统菜单）调整，或未来接入 Linux X11 自适应。
2. 图片预览独立窗口、文件夹选择对话框随系统（GTK 对话框）外观不同，功能一致。
3. 后台无「全局快捷键呼出」（Windows 也已移除该功能，仅托盘呼出）。

## 6. 龙芯 loongarch64 评估结论（记录在案）

实测环境：银河麒麟 V10 SP1（2403），`loongarch64`，gcc 8.3 / glibc 2.28，
源内 `libwebkit2gtk` 仅 4.0(2.38)，无 4.1。

结论：**Tauri 2 在 loongarch64 麒麟 V10 SP1 上无法构建**。
- Tauri 2 / wry 硬性依赖 `libwebkit2gtk-4.1`，麒麟源缺失，且 glibc 2.28 过老、
  debian-ports 的 4.1 无法回移（依赖撕裂）。
- 若必须支持龙芯：唯一出路是更换 UI 引擎（Tauri v1 或 GTK3+webkit2gtk-4.0 自研壳），
  属重写级工程，未列入当前计划。

## 7. 发布流程

1. 推送 tag `v*` → Actions 自动构建 arm64 deb 并挂 Release；
2. 从 Release 下载 `.deb`，在目标机 `sudo dpkg -i`（依赖缺失时 `sudo apt -f install`）；
3. 首次运行核对：
   - `~/.local/share/quickcopy/` 自动创建；
   - 托盘图标常驻；与 Windows 端互通收发；
   - 「设置 → 开机自启」生成 `~/.config/autostart/QuickCopy.desktop`。