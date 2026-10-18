//! 前端可调用的自定义命令（IPC）。
//! 全局对象：window.__TAURI__.core.invoke('命令名', {参数})

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
pub fn qp_cache_dir(app: AppHandle) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("cache");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().to_string())
}

/// 清空应用缓存目录（图片缩略等），返回已清理的字节数
#[tauri::command]
pub fn qp_clear_cache(app: AppHandle) -> Result<u64, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("cache");
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

/// 动态修改呼出快捷键（设置页录制后调用）
#[tauri::command]
pub fn qp_set_hotkey(app: AppHandle, combo: String) -> Result<(), String> {
    crate::apply_hotkey(&app, &combo);
    settings::set_key(&app, "hotkey", Value::String(combo)).map_err(|e| e.to_string())
}

/// 主窗口隐藏到系统托盘（点 × 不退出应用，任务栏不留；托盘/热键可再呼出）
#[tauri::command]
pub fn qp_window_hide(app: AppHandle) -> Result<(), String> {
    app.get_webview_window(crate::MAIN_LABEL)
        .ok_or_else(|| "主窗口不存在".to_string())?
        .hide()
        .map_err(|e| e.to_string())
}

/* ────────────── 关于与升级（S6） ────────────── */

/// 关于信息：应用名 / 版本 / 作者 / 升级记录
#[tauri::command]
pub fn about_info() -> serde_json::Value {
    serde_json::json!({
        "app": "快捷粘贴 QuickPaste",
        "version": env!("CARGO_PKG_VERSION"),
        "author": "zhangaiguo",
        "changelog": [
            {"v":"0.1.0","log":"初始版本：剪贴板历史、文件传输（LocalSend 协议）、窗口记忆、全局热键"},
        ],
    })
}

/// 找 git 可执行文件（完整路径优先，避免继承 PATH 缺失）
fn git_exe() -> Result<String, String> {
    for c in [
        r"C:\Program Files\Git\bin\git.exe",
        r"C:\Program Files\Git\cmd\git.exe",
        "git",
    ] {
        if std::process::Command::new(c).arg("--version").output().is_ok() {
            return Ok(c.to_string());
        }
    }
    Err("未找到 git 命令".into())
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

/// 在线更新：git ls-remote 读取远程标签版本，与本地版本比较
#[tauri::command]
pub fn qp_check_update() -> Result<serde_json::Value, String> {
    let remote = "https://github.com/SealSkyA/QuickPast.git";
    let git = git_exe()?;
    // 限时执行：10 秒内无响应视为网络异常
    let (tx, rx) = std::sync::mpsc::channel();
    let (git2, remote2) = (git.clone(), remote.to_string());
    std::thread::spawn(move || {
        let out = std::process::Command::new(&git2)
            .args(["ls-remote", "--tags", &remote2])
            .output();
        let _ = tx.send(out);
    });
    let res = rx.recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|_| "检查更新超时（网络不通或无法访问 git）".to_string())?;
    let out = res.map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    // 解析 vX.Y.Z 标签，取最大版本
    let mut latest: Option<String> = None;
    for line in text.lines() {
        if let Some(tag) = line.split("refs/tags/").nth(1) {
            let t = tag.trim();
            if let Some(v) = t.strip_prefix('v') {
                if v.split('.').count() >= 2 {
                    if latest.as_deref().map(|l| cmp_version(v, l) > 0).unwrap_or(true) {
                        latest = Some(v.to_string());
                    }
                }
            }
        }
    }
    let current = env!("CARGO_PKG_VERSION").to_string();
    let latest = latest.unwrap_or_else(|| current.clone());
    serde_json::to_value(serde_json::json!({
        "current": current,
        "latest": latest,
        "hasUpdate": cmp_version(&latest, &current) > 0,
        "url": "https://github.com/SealSkyA/QuickPast/releases",
    })).map_err(|e| e.to_string())
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

/// 最大化 / 还原（标题栏按钮）
#[tauri::command]
pub fn qp_window_toggle_max(app: AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window(crate::MAIN_LABEL)
        .ok_or_else(|| "主窗口不存在".to_string())?;
    if w.is_maximized().unwrap_or(false) {
        w.unmaximize().map_err(|e| e.to_string())
    } else {
        w.maximize().map_err(|e| e.to_string())
    }
}

#[repr(C)]
struct POINT {
    x: i32,
    y: i32,
}

#[link(name = "user32")]
extern "system" {
    fn GetCursorPos(p: *mut POINT) -> i32;
    fn GetAsyncKeyState(vk: i32) -> i16;
}

const VK_LBUTTON: i32 = 0x01;
const MIN_W_LOGICAL: f64 = 700.0; // 与 tauri.conf.json minWidth 一致
const MIN_H_LOGICAL: f64 = 460.0;

/// 临时调试：不依赖鼠标，直接模拟 20 步缩放，验证 Rust 线程 + set_size 链路
#[tauri::command]
pub fn qp_debug_resize(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(crate::MAIN_LABEL)
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
        .get_webview_window(crate::MAIN_LABEL)
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let dir = direction.to_ascii_lowercase();
    if !["n", "s", "e", "w", "ne", "nw", "se", "sw"].contains(&dir.as_str()) {
        return Err(format!("未知缩放方向：{direction}"));
    }
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