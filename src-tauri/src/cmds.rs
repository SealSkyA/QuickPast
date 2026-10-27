//! 前端可调用的自定义命令（IPC）。
//! 全局对象（Tauri 1）：window.__TAURI__.invoke('命令名', {参数})

use std::thread;
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Position};

use crate::settings;

#[tauri::command]
pub fn get_setting(app: AppHandle, key: String) -> Value {
    settings::get_key(&app, &key).ok().flatten().unwrap_or(Value::Null)
}

#[tauri::command]
pub fn set_setting(app: AppHandle, key: String, value: Value) -> Result<(), String> {
    settings::set_key(&app, &key, value).map_err(|e| e.to_string())
}

/// 打开系统资源管理器（缓存目录、发送目录等）
#[tauri::command]
pub fn open_path(_app: AppHandle, path: String) -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 应用缓存目录（高级设置：查看/清空）
#[tauri::command]
pub fn qp_cache_dir(_app: AppHandle) -> Result<String, String> {
    let dir = crate::paths::data_root().join("cache");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().to_string())
}

/// 清空应用缓存目录（图片缩略等），返回已清理的字节数
#[tauri::command]
pub fn qp_clear_cache(_app: AppHandle) -> Result<u64, String> {
    let dir = crate::paths::data_root().join("cache");
    let mut freed = 0u64;
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if let Ok(md) = p.metadata() {
                if md.is_dir() {
                    if let Ok(n) = std::fs::remove_dir_all(&p) {
                        let _ = n;
                    }
                } else if let Ok(n) = std::fs::remove_file(&p) {
                    let _ = n;
                }
                freed += md.len();
            }
        }
    }
    Ok(freed)
}

#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 应用缓存目录（高级设置：查看/清空）

/// 动态修改呼出快捷键（设置页录制后调用）
#[tauri::command]
pub fn qp_set_hotkey(app: AppHandle, combo: String) -> Result<(), String> {
    crate::apply_hotkey(&app, &combo);
    settings::set_key(&app, "hotkey", Value::String(combo)).map_err(|e| e.to_string())
}

/// 主窗口隐藏到系统托盘（点 × 不退出应用，任务栏不留；托盘/热键可再呼出）
#[tauri::command]
pub fn qp_window_hide(app: AppHandle) -> Result<(), String> {
    let win = app
        .get_window(crate::MAIN_LABEL)
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let _ = win.set_skip_taskbar(true); // 隐藏进托盘 → 任务栏不保留图标
    win.hide().map_err(|e| e.to_string())
}

/// 主窗口最小化（标题栏按钮）
#[tauri::command]
pub fn qp_window_minimize(app: AppHandle) -> Result<(), String> {
    let w = app
        .get_window(crate::MAIN_LABEL)
        .ok_or_else(|| "主窗口不存在".to_string())?;
    w.minimize().map_err(|e| e.to_string())
}

/// 开机自启开关：写入设置并立即生效。Windows 写注册表 Run 键；Linux（麒麟/UOS）写
/// XDG autostart 的 .desktop（~/.config/autostart/），两个发行版都遵循标准。
/// 自启启动附带 --hidden 参数 → 开机时静默运行（托盘常驻）；手动双击启动仍正常显示窗口。
#[tauri::command]
pub fn qp_set_autostart(app: AppHandle, on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        // 清理历史遗留的旧名称自启项（QuickPaste 品牌改名前的残留）：
        // 改名前写入的值名是 QuickPaste，指向的 exe 路径已失效/被覆盖，
        // 不清理会造成开机尝试启动失效路径、多实例并存等“时好时坏”现象。
        let _ = std::process::Command::new("reg")
            .args([
                "delete",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                "/v",
                "QuickPaste",
                "/f",
            ])
            .output();
        if on {
            // 优先使用安装目录的正式程序（NSIS currentUser：%LOCALAPPDATA%\QuickCopy\QuickCopy.exe），
            // 该路径稳定、不会被 cargo 构建清理；只有在未安装时才回退到 current_exe
            // （如开发模式直接跑 target/release 产物）。否则自启项指向构建产物目录，
            // 一旦重新构建/清理就失效，造成开机自启“时好时坏”。
            let exe = {
                let installed = std::env::var("LOCALAPPDATA").ok().map(|d| {
                    std::path::PathBuf::from(d).join("QuickCopy").join("QuickCopy.exe")
                }).filter(|p| p.exists());
                match installed {
                    Some(p) => p,
                    None => std::env::current_exe().map_err(|e| e.to_string())?,
                }
            };
            let cmd = format!("\"{}\" --hidden", exe.to_string_lossy());
            let out = std::process::Command::new("reg")
                .args([
                    "add",
                    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                    "/v",
                    "QuickCopy",
                    "/d",
                    &cmd,
                    "/f",
                ])
                .output()
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err("写入开机自启注册表失败".to_string());
            }
        } else {
            let _ = std::process::Command::new("reg")
                .args([
                    "delete",
                    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                    "/v",
                    "QuickCopy",
                    "/f",
                ])
                .output();
        }
    }
    #[cfg(not(windows))]
    {
        let dir = std::path::PathBuf::from(
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
        )
        .join(".config/autostart");
        let file = dir.join("QuickCopy.desktop");
        if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&dir).map_err(|e| format!("创建自启目录失败：{e}"))?;
            std::fs::write(
                &file,
                format!(
                    "[Desktop Entry]\nType=Application\nName=QuickCopy\nExec=\"{}\" --hidden\nComment=局域网剪贴板历史与文件互传\n",
                    exe.to_string_lossy()
                ),
            )
            .map_err(|e| format!("写自启文件失败：{e}"))?;
        } else {
            let _ = std::fs::remove_file(&file);
        }
    }
    settings::set_key(&app, "autostart", Value::Bool(on)).map_err(|e| e.to_string())
}

/// 接收任务到达时确保主窗口可见：
/// 窗口在托盘/隐藏态 → 呼出并定位到主显示器右下角（像系统通知一样弹出接收请求）；
/// 窗口本就可见 → 保持原位不动，只还原最小化并聚焦。
#[tauri::command]
pub fn qp_show_recv(app: AppHandle) -> Result<(), String> {
    let win = app
        .get_window(crate::MAIN_LABEL)
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let _ = win.unminimize();
    let was_visible = win.is_visible().unwrap_or(false);
    let _ = win.show();
    let _ = win.set_skip_taskbar(false); // 接收任务呼出 → 任务栏图标正常显示
    let _ = win.set_focus();
    if !was_visible {
        // 托盘/隐藏态：定位到主显示器右下角（距右缘 24px、距底缘 64px 近似任务栏高度）
        if let Ok(Some(mon)) = win.primary_monitor() {
            let mpos = mon.position();
            let msz = mon.size();
            if let Ok(psz) = win.outer_size() {
                let w = psz.width as i32;
                let h = psz.height as i32;
                let x = mpos.x + msz.width as i32 - w - 24;
                let y = mpos.y + msz.height as i32 - h - 64;
                let _ = win.set_position(Position::Physical(PhysicalPosition::new(x, y)));
            }
        }
    }
    Ok(())
}

/* ────────────── 关于与升级（S6） ────────────── */

/// 关于信息：应用名 / 版本 / 作者 / 升级记录
#[tauri::command]
pub fn about_info() -> serde_json::Value {
    serde_json::json!({
        "app": "快捷拷贝 QuickCopy",
        "version": env!("CARGO_PKG_VERSION"),
        "author": "zhangaiguo",
        "changelog": [
            {"v":"0.2.18","log":"快捷粘贴右键「发送给设备/发送所选项」直达文件传输-发送页；与手机端 0.2.18 合并发布"},
            {"v":"0.2.17","log":"关于页更名「快捷拷贝」、升级记录补齐；手机端：后台恢复自动重连发现、设备收藏、打开目录直达文件管理"},
            {"v":"0.2.16","log":"手机端：在线检查更新、下拉菜单不再被遮挡、界面主题可切换"},
            {"v":"0.2.15","log":"品牌更名 QuickCopy、图标改色；修复 Android 存储权限"},
            {"v":"0.2.14","log":"手机端 Flutter App 首次发布（收发/发现 + 新拟物 UI）"},
            {"v":"0.2.13","log":"文本发送改为临时 txt 文件通道，提升对端兼容性"},
            {"v":"0.2.12","log":"文本与文件支持混合发送，多条文本自动合并"},
            {"v":"0.2.11","log":"批量选择后「发送给设备」发送所有已选项"},
            {"v":"0.2.10","log":"发送给设备支持全类型 + 图片按内容去重"},
            {"v":"0.2.9","log":"默认监听/广播端口改为 53318"},
            {"v":"0.2.8","log":"自动保存/信任设备静默接收，移除顶栏置顶徽标"},
            {"v":"0.2.7","log":"接收重名文件/文件夹自动追加序号，不再覆盖"},
            {"v":"0.2.6","log":"发送命令改后台线程执行，彻底解决点击设备后窗口未响应"},
            {"v":"0.2.5","log":"发送成功后清空选择列表；大文件/大文件夹传输优化"},
            {"v":"0.2.4","log":"支持文件夹选择/拖放发送，接收端还原目录结构"},
            {"v":"0.2.3","log":"设备收藏功能；发送页探查反馈与按钮整理；修复窗口位置累积偏移"},
            {"v":"0.2.2","log":"移除全局快捷键只留托盘呼出；在线更新加进度条与镜像加速"},
            {"v":"0.2.1","log":"修复拖放识别/开机自启/在线状态；引入打包发布流程"},
            {"v":"0.2.0","log":"图片预览独立窗口（屏幕居中/缩放/关闭）；数据改存安装路径；覆盖式安装；单实例；默认快捷键 Ctrl+Alt+Z"},
            {"v":"0.1.0","log":"初始版本：剪贴板历史、文件传输（LocalSend 协议）、窗口记忆、全局热键"},
        ],
    })
}

/// 语义版本比较：a > b → Greater；返回 -1/0/1
fn cmp_version(a: &str, b: &str) -> i32 {
    let num = |s: &str| -> u64 { s.trim_matches(|c: char| !c.is_ascii_digit()).parse().unwrap_or(0) };
    let av: Vec<u64> = a.split('.').map(num).collect();
    let bv: Vec<u64> = b.split('.').map(num).collect();
    for i in 0..av.len().max(bv.len()) {
        let x = av.get(i).copied().unwrap_or(0);
        let y = bv.get(i).copied().unwrap_or(0);
        if x != y { return if x > y { 1 } else { -1 }; }
    }
    0
}

// 静默子进程标志：不弹终端/控制台窗口（CREATE_NO_WINDOW；仅 Windows）
#[cfg(windows)]
const NO_WINDOW: u32 = 0x08000000;

/// 构造不弹终端窗口的子进程命令（Windows 加 creation_flags；Linux 无需处理）。
#[cfg(windows)]
fn silent_cmd(prog: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(prog);
    c.creation_flags(NO_WINDOW);
    c
}
#[cfg(not(windows))]
fn silent_cmd(prog: &str) -> std::process::Command {
    std::process::Command::new(prog)
}

/// 在线更新：读取 GitHub Releases 最新版本（系统 curl 静默请求，不弹终端框）。
/// 顺带取回安装包资产地址，供 qp_download_update 在线下载。
#[tauri::command]
pub fn qp_check_update() -> Result<serde_json::Value, String> {
    let out = silent_cmd("curl")
        .args(["-fs", "--max-time", "12", "-H", "Accept: application/vnd.github+json", "-H", "User-Agent: QuickCopy"])
        .arg("https://api.github.com/repos/SealSkyA/QuickPast/releases/latest")
        .output()
        .map_err(|e| format!("无法启动 curl：{e}"))?;
    if !out.status.success() {
        let code = out.status.code().unwrap_or(-1);
        return Err(if code == 22 { "仓库暂未发布任何版本".into() } else { format!("检查更新失败（curl exit {code}）") });
    }
    let body = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("解析更新信息失败：{e}"))?;
    let tag = v.get("tag_name").and_then(|t| t.as_str()).unwrap_or("").to_string();
    let latest = tag.trim_start_matches('v').to_string();
    // 优先选安装包资产：Windows 找 .exe/.msi；Linux（麒麟/UOS）找 .deb
    let mut asset_name = String::new();
    let mut asset_url = String::new();
    let mut asset_size: u64 = 0;
    if let Some(arr) = v.get("assets").and_then(|a| a.as_array()) {
        for a in arr {
            let name = a.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let url = a.get("browser_download_url").and_then(|u| u.as_str()).unwrap_or("");
            let hit;
            #[cfg(windows)]
            {
                hit = (name.ends_with(".exe") || name.ends_with(".msi")) && !url.is_empty();
            }
            #[cfg(not(windows))]
            {
                hit = name.ends_with(".deb") && !url.is_empty();
            }
            if hit {
                asset_name = name.to_string();
                asset_url = url.to_string();
                asset_size = a.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
                break;
            }
        }
    }
    let current = env!("CARGO_PKG_VERSION").to_string();
    serde_json::to_value(serde_json::json!({
        "current": current,
        "latest": latest,
        "hasUpdate": !latest.is_empty() && cmp_version(&latest, &current) > 0,
        "url": "https://github.com/SealSkyA/QuickPast/releases",
        "assetName": asset_name,
        "assetUrl": asset_url,
        "assetSize": asset_size,
    })).map_err(|e| e.to_string())
}

/// 在线下载新版本安装包（GitHub Release 资产），保存到应用缓存 downloads 目录。
/// 异步执行（后台线程 + 官方地址/加速镜像依次尝试），前端轮询 qp_download_progress 获取进度。
/// 已下载过（或正在下载）时直接返回。
#[tauri::command]
pub fn qp_download_update(
    app: AppHandle,
    url: String,
    name: String,
    size: Option<u64>,
) -> Result<String, String> {
    let dir = crate::paths::data_root().join("cache").join("downloads");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // 清洗文件名（避免非法字符）
    let safe: String = name
        .chars()
        .filter(|c| !c.is_ascii_control() && !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let safe = if safe.trim().is_empty() { "quickcopy_setup.exe".to_string() } else { safe };
    let path = dir.join(&safe);
    if path.exists() {
        return Ok(path.to_string_lossy().to_string()); // 已下载过
    }
    let st = app.state::<crate::DownloadState>();
    {
        let mut d = st.inner.lock().unwrap();
        if d.phase == "downloading" {
            return Err("已在下载中，请稍候".into());
        }
        d.phase = "downloading".into();
        d.size = size.unwrap_or(0);
        d.path = None;
        d.error = None;
        d.tmp = None;
    }
    let ret = path.to_string_lossy().to_string();
    std::thread::spawn(move || {
        // 后台线程内通过 app 取状态（AppHandle 自身随闭包存活）
        {
            let st = app.state::<crate::DownloadState>();
            let mut d = st.inner.lock().unwrap();
            d.tmp = Some(path.with_extension("part"));
        }
        let res = do_download(&url, &path);
        let st = app.state::<crate::DownloadState>();
        let mut d = st.inner.lock().unwrap();
        match res {
            Ok(p) => {
                d.phase = "done".into();
                d.path = Some(p);
                d.tmp = None;
            }
            Err(e) => {
                d.phase = "error".into();
                d.error = Some(e);
                d.tmp = None;
            }
        }
    });
    Ok(ret)
}

/// 查询安装包下载进度（前端每 400ms 轮询）
#[tauri::command]
pub fn qp_download_progress(app: AppHandle) -> serde_json::Value {
    let st = app.state::<crate::DownloadState>();
    let d = st.inner.lock().unwrap();
    // 下载中：以临时文件当前大小作为已下载字节数（近似进度）
    let written = if d.phase == "downloading" {
        d.tmp
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0)
    } else {
        d.path.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0)
    };
    serde_json::json!({
        "phase": d.phase,
        "written": written,
        "size": d.size,
        "path": d.path,
        "error": d.error,
    })
}

/// GitHub 下载加速镜像前缀（依次尝试：官方地址 → 镜像，提高国内可达性）
const GH_MIRRORS: [&str; 4] = [
    "https://ghfast.top/",
    "https://ghproxy.net/",
    "https://gh-proxy.com/",
    "https://mirror.ghproxy.com/",
];

fn do_download(url: &str, path: &std::path::Path) -> Result<String, String> {
    let tmp = path.with_extension("part");
    let mut candidates: Vec<String> = Vec::with_capacity(1 + GH_MIRRORS.len());
    candidates.push(url.to_string());
    for m in GH_MIRRORS {
        candidates.push(format!("{m}{url}"));
    }
    for cand in &candidates {
        let _ = std::fs::remove_file(&tmp);
        let out = silent_cmd("curl")
            .args(["-Lfs", "--connect-timeout", "8", "--max-time", "300", "-o"])
            .arg(&tmp)
            .arg(cand)
            .output();
        match out {
            Ok(o) if o.status.success() && tmp.exists() => {
                let len = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
                // 镜像返回错误页/极小响应视为失败，继续下一候选
                if len < 4096 {
                    continue;
                }
                let _ = std::fs::rename(&tmp, path);
                return Ok(path.to_string_lossy().to_string());
            }
            _ => continue,
        }
    }
    Err("下载失败：无法连接下载服务器，请检查网络后重试".into())
}

/// 运行已下载的安装包（NSIS / MSI 图形安装向导；应用本体不阻塞）
#[tauri::command]
pub fn qp_install_update(path: String) -> Result<(), String> {
    std::process::Command::new("cmd")
        .args(["/c", "start", "", &path])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 用系统默认浏览器打开网址
#[tauri::command]
pub fn qp_open_url(url: String) -> Result<(), String> {
    std::process::Command::new("cmd")
        .args(["/c", "start", "", &url])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 显示"关于与升级"独立窗口（tauri.conf 静态声明，启动时已创建，屏幕居中）。
#[tauri::command]
pub fn qp_show_about(app: AppHandle) -> Result<(), String> {
    let w = app
        .get_window(crate::ABOUT_LABEL)
        .ok_or_else(|| "about 窗口不存在".to_string())?;
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.center(); // 每次呼出保持在屏幕中间
    let _ = w.set_focus();
    Ok(())
}

/// 显示"图片预览"独立窗口：屏幕居中，顶栏支持最小化/最大化/关闭。
/// 主窗口点击缩略图时调用：先记录 id（供预览页轮询兜底）再 emit 事件，然后居中显示。
#[tauri::command]
pub fn qp_viewer_show(app: AppHandle, id: u64, name: Option<String>) -> Result<(), String> {
    const VIEWER_LABEL: &str = "viewer";
    let w = app
        .get_window(VIEWER_LABEL)
        .ok_or_else(|| "viewer 窗口不存在".to_string())?;
    // 记录当前预览图片 id（viewer 页面轮询兜底，防事件时序丢失）
    *app.state::<crate::AppState>().viewer_id.lock().unwrap() = Some(id);
    let _ = w.emit_all("viewer:show", serde_json::json!({ "id": id, "name": name }));
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.center();
    let _ = w.set_focus();
    Ok(())
}

/// viewer 页面轮询：返回当前要预览的图片 id（无则 None）
#[tauri::command]
pub fn qp_viewer_current_id(app: AppHandle) -> Option<u64> {
    *app.state::<crate::AppState>().viewer_id.lock().unwrap()
}

/// 最大化 / 还原（标题栏按钮）
#[tauri::command]
pub fn qp_window_toggle_max(app: AppHandle) -> Result<(), String> {
    let w = app
        .get_window(crate::MAIN_LABEL)
        .ok_or_else(|| "主窗口不存在".to_string())?;
    if w.is_maximized().unwrap_or(false) {
        w.unmaximize().map_err(|e| e.to_string())
    } else {
        w.maximize().map_err(|e| e.to_string())
    }
}

/// 关于窗口：最小化（标题栏按钮，走 Rust 命令而非前端 window API）
#[tauri::command]
pub fn qp_about_minimize(app: AppHandle) -> Result<(), String> {
    let w = app
        .get_window(crate::ABOUT_LABEL)
        .ok_or_else(|| "关于窗口不存在".to_string())?;
    w.minimize().map_err(|e| e.to_string())
}

/// 关于窗口：最大化 / 还原
#[tauri::command]
pub fn qp_about_toggle_max(app: AppHandle) -> Result<(), String> {
    let w = app
        .get_window(crate::ABOUT_LABEL)
        .ok_or_else(|| "关于窗口不存在".to_string())?;
    if w.is_maximized().unwrap_or(false) {
        w.unmaximize().map_err(|e| e.to_string())
    } else {
        w.maximize().map_err(|e| e.to_string())
    }
}

/// 关于窗口：隐藏（关闭按钮语义 = 隐藏到托盘，进程常驻）
#[tauri::command]
pub fn qp_about_hide(app: AppHandle) -> Result<(), String> {
    let w = app
        .get_window(crate::ABOUT_LABEL)
        .ok_or_else(|| "关于窗口不存在".to_string())?;
    w.hide().map_err(|e| e.to_string())
}

#[cfg(windows)]
#[repr(C)]
struct POINT {
    x: i32,
    y: i32,
}

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn GetCursorPos(p: *mut POINT) -> i32;
    fn GetAsyncKeyState(vk: i32) -> i16;
}

#[cfg(windows)]
const VK_LBUTTON: i32 = 0x01;
const MIN_W_LOGICAL: f64 = 600.0; // 与 tauri.conf.json minWidth 一致
const MIN_H_LOGICAL: f64 = 400.0;

/// 临时调试：不依赖鼠标，直接模拟 20 步缩放，验证 Rust 线程 + set_size 链路
#[tauri::command]
pub fn qp_debug_resize(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_window(crate::MAIN_LABEL)
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let win = window.clone();
    let base = window.outer_size().map_err(|e| e.to_string())?;
    thread::spawn(move || {
        for i in 1..=20 {
            let _ = win.set_size(PhysicalSize::new(base.width + i * 10, base.height + i * 7));
            thread::sleep(Duration::from_millis(50));
        }
    });
    Ok(())
}

/// 无边框窗口自定义缩放。direction ∈ {n,s,e,w,ne,nw,se,sw}（小写）。
/// 边：单维缩放；四角：按窗口当前宽高比**等比**缩放，并同步修正窗口位置
/// （n/w 方向的边跟随鼠标）。命令在本线程内跟踪鼠标直到左键释放，
/// 每 ~16ms 更新一次窗口大小/位置。
#[tauri::command]
pub fn qp_resize_begin(app: AppHandle, direction: String) -> Result<(), String> {
    let window = app
        .get_window(crate::MAIN_LABEL)
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let dir = direction.to_ascii_lowercase();
    if !["n", "s", "e", "w", "ne", "nw", "se", "sw"].contains(&dir.as_str()) {
        return Err(format!("未知缩放方向：{direction}"));
    }
    #[cfg(windows)]
    {
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    // 最小物理尺寸（配置为逻辑像素）
    let min_w = (MIN_W_LOGICAL * scale).round().max(1.0) as u32;
    let min_h = (MIN_H_LOGICAL * scale).round().max(1.0) as u32;
    let base_w = size.width as f64;
    let base_h = size.height as f64;
    let base_x = pos.x as f64;
    let base_y = pos.y as f64;
    let ratio = (base_w / base_h).max(0.01);
    let mut start = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut start) };
    let (sx, sy) = (start.x as f64, start.y as f64);

    let win = window.clone();
    thread::spawn(move || {
        loop {
            // 左键已释放 → 拖拽结束
            unsafe {
                if (GetAsyncKeyState(VK_LBUTTON) as u16) & 0x8000 == 0 {
                    break;
                }
            }
            let mut cur = POINT { x: 0, y: 0 };
            unsafe { GetCursorPos(&mut cur) };
            let dx = cur.x as f64 - sx;
            let dy = cur.y as f64 - sy;
            let has_e = dir.contains('e');
            let has_w = dir.contains('w');
            let has_s = dir.contains('s');
            let has_n = dir.contains('n');
            // 对应方向的位移增量（正 = 变大）
            let dxs = if has_e { dx } else if has_w { -dx } else { 0.0 };
            let dys = if has_s { dy } else if has_n { -dy } else { 0.0 };
            let (mut nw, mut nh) = (base_w, base_h);
            if has_e || has_w || has_n || has_s {
                if (has_e || has_w) && (has_s || has_n) {
                    // 角：以移动幅度大的维度驱动，等比缩放
                    let delta = if dxs.abs() >= dys.abs() { dxs } else { dys };
                    nw = (base_w + delta).max(min_w as f64);
                    nh = (nw / ratio).max(min_h as f64);
                } else {
                    if has_e || has_w { nw = (base_w + dxs).max(min_w as f64); }
                    if has_s || has_n { nh = (base_h + dys).max(min_h as f64); }
                }
            }
            // 从 n/w 方向缩放时，对应边跟随鼠标（修正窗口位置）
            if has_w || has_n {
                let nx = if has_w { base_x + (base_w - nw) } else { base_x };
                let ny = if has_n { base_y + (base_h - nh) } else { base_y };
                let _ = win.set_position(Position::Physical(PhysicalPosition::new(nx as i32, ny as i32)));
            }
            let _ = win.set_size(PhysicalSize::new(nw as u32, nh as u32));
            thread::sleep(Duration::from_millis(16));
        }
    });
    Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (&window, &dir);
        Err("当前系统暂不支持鼠标拖拽缩放，请在系统窗口管理器中调整".into())
    }
}

/* ────────────── 拖放文件（HTML5 dataTransfer → 临时文件） ────────────── */

/// WebView2 的 HTML5 拖放拿不到磁盘路径（File.path 未注入），
/// 前端读文件内容后经 IPC 传来，由本命令写入 %TEMP%\qp-drag\ 临时文件，
/// 返回磁盘绝对路径列表供发送链路使用。
#[derive(serde::Deserialize)]
pub struct DragIn {
    name: String,
    data: Vec<u8>,
}

#[tauri::command]
pub fn qp_save_drag_files(files: Vec<DragIn>) -> Result<Vec<String>, String> {
    let dir = std::env::temp_dir().join("qp-drag");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (i, f) in files.iter().enumerate() {
        // 只用文件名部分，防路径穿越
        let base = std::path::Path::new(&f.name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("file{i}"));
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let p = dir.join(format!("{ts}_{i}_{base}"));
        std::fs::write(&p, &f.data).map_err(|e| e.to_string())?;
        out.push(p.to_string_lossy().to_string());
    }
    Ok(out)
}

/* ────────────── 磁盘对话框（Tauri 1 无 dialog 插件发布，用内核 tauri::api::dialog） ────────────── */

/// 系统「打开文件」对话框（多选），返回绝对路径列表（取消 → 空数组）
#[tauri::command]
pub fn qp_pick_files() -> Result<Vec<String>, String> {
    let b = tauri::api::dialog::blocking::FileDialogBuilder::new();
    Ok(b.pick_files()
        .map(|v| v.into_iter().map(|p| p.to_string_lossy().to_string()).collect())
        .unwrap_or_default())
}

/// 系统「选择文件夹」对话框（单选），返回绝对路径（取消 → null）
#[tauri::command]
pub fn qp_pick_dir() -> Result<Option<String>, String> {
    let b = tauri::api::dialog::blocking::FileDialogBuilder::new();
    Ok(b.pick_folder().map(|p| p.to_string_lossy().to_string()))
}

/// 系统「另存为」对话框，返回用户选择的保存路径（取消 → null）
#[tauri::command]
pub fn qp_pick_save() -> Result<Option<String>, String> {
    let b = tauri::api::dialog::blocking::FileDialogBuilder::new();
    Ok(b.save_file().map(|p| p.to_string_lossy().to_string()))
}