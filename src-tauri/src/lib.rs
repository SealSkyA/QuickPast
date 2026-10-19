//! QuickPaste 桌面骨架（S0）：全局热键、置顶、窗口几何记忆、设置持久化。
//! 扩展点：`modules/` 模块注册表（见 docs/实施/02-全局产品约束与扩展架构.md 4.2）。

pub mod cmds;
pub mod history;
pub mod net;
pub mod paths;
pub mod settings;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, Size, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub const MAIN_LABEL: &str = "main";
const GEOM_KEY: &str = "window.geometry";
const GEOM_DEBOUNCE_MS: u64 = 900;

struct AppState {
    geom_saved_at: Mutex<Instant>,
    /// 用户从历史列表点击复制的内容指纹：watcher 检测到相同内容时
    /// 不新增、不移动、不去重（保持列表与序号不变），只保留"上次复制"标记
    last_copied: Mutex<Option<history::ClipKey>>,
    /// 当前预览窗口要显示的图片 id（viewer 页面轮询兜底，防事件时序丢失）
    viewer_id: Mutex<Option<u64>>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            // 初始为“很久以前”，保证启动后的首次窗口事件立即触发保存
            geom_saved_at: Mutex::new(Instant::now() - Duration::from_secs(3600)),
            last_copied: Mutex::new(None),
            viewer_id: Mutex::new(None),
        }
    }
}

/// 当前注册的呼出快捷键（支持设置页动态修改）
struct HotkeyState {
    cur: Mutex<Option<String>>,
}

impl Default for HotkeyState {
    fn default() -> Self {
        HotkeyState {
            // 初始 None：表示尚未注册任何热键，首次 apply_hotkey 必须真正注册
            cur: Mutex::new(None),
        }
    }
}

#[derive(serde::Deserialize, Clone, Copy)]
struct GeomValue {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

pub fn run() {
    tauri::Builder::default()
        // 单实例：已运行时再次启动（双击图标/快捷方式）→ 呼出主窗口，不重复开进程
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window(MAIN_LABEL) {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(HotkeyState::default())
        .manage(history::History::default())
        .manage(net::NetState::default())
        .invoke_handler(tauri::generate_handler![
            cmds::get_setting,
            cmds::set_setting,
            cmds::open_path,
            cmds::app_version,
            cmds::qp_cache_dir,
            cmds::qp_clear_cache,
            cmds::qp_resize_begin,
            cmds::qp_debug_resize,
            cmds::qp_set_hotkey,
            cmds::qp_window_hide,
            cmds::qp_window_toggle_max,
            cmds::about_info,
            cmds::qp_check_update,
            cmds::qp_download_update,
            cmds::qp_install_update,
            cmds::qp_open_url,
            cmds::qp_show_about,
            cmds::qp_viewer_show,
            cmds::qp_viewer_current_id,
            history::history_load,
            history::history_remove,
            history::history_clear,
            history::history_set_fav,
            history::history_pin,
            history::history_edit,
            history::history_add_note,
            history::history_export,
            history::history_apply_limits,
            history::history_mark_last,
            history::clipboard_set_text,
            history::clipboard_set_files,
            history::clipboard_set_img,
            history::history_copy_rich,
            history::clip_img_b64,
            history::clip_img_path,
            history::clip_img_full,
            history::qp_debug_clip_set_text,
            history::qp_debug_clip_set_html,
            history::qp_debug_clip_get_html,
            net::net_devices,
            net::net_refresh,
            net::net_save_dir,
            net::net_accept,
            net::net_reject,
            net::net_send,
            net::net_send_text,
            net::net_share,
            net::net_set_alias,
            net::net_my_info
        ])
        .setup(|app| {
            // 数据迁移：旧版本数据在 C 盘 AppData，新版本统一存安装路径 → 一次性复制
            migrate_data(app.handle());
            // S1 剪贴板历史：载入持久化数据（含存量去重）并启动监听
            let loaded = history::load_items(app.handle());
            {
                let st = app.state::<history::History>();
                *st.items.lock().unwrap() = loaded;
            }
            history::prune_dups(app.handle());
            // 类别数量上限：启动时把已超限的旧数据裁剪到上限内
            history::prune_by_limits(app.handle());
            history::spawn_watcher(app.handle().clone());
            // S2 传输核心：组播发现 + HTTP 服务器
            net::spawn_discovery(app.handle().clone());
            net::spawn_server(app.handle().clone());
            // 发送侧端到端自测钩子（QP_SEND_TEST="ip:port|path1;path2"）
            if let Ok(v) = std::env::var("QP_SEND_TEST") {
                net::test_send(app.handle().clone(), &v);
            }
            // 呼出快捷键（支持设置页修改；默认 Ctrl+Alt+Z）
            let hotkey = settings::get_key(app.handle(), "hotkey")
                .ok()
                .flatten()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "Ctrl+Alt+Z".into());
            // 旧默认 Alt+Z 迁移为新默认 Ctrl+Alt+Z（用户未改过快捷键时保持默认一致）
            let hotkey = if hotkey == "Alt+Z" {
                "Ctrl+Alt+Z".to_string()
            } else {
                hotkey.clone()
            };
            let _ = settings::set_key(app.handle(), "hotkey", serde_json::json!(hotkey));
            apply_hotkey(app.handle(), &hotkey);
            setup_tray(app.handle())?;
            apply_autostart(app)?;
            restore_geometry(app)?;
            // 开机自启：静默启动（不显示主窗口，托盘/热键常驻）
            let auto_start = settings::get_key(app.handle(), "autostart")
                .ok()
                .flatten()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if auto_start {
                if let Some(w) = app.get_webview_window(MAIN_LABEL) {
                    let _ = w.hide();
                }
            }
            restore_geometry(app)?;
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Moved(_) | WindowEvent::Resized(_) if window.label() == MAIN_LABEL => {
                schedule_geom_save(window.app_handle().clone());
            }
            // ×/系统关闭请求：不退出应用，隐藏到系统托盘（进程常驻）
            WindowEvent::CloseRequested { api, .. } if window.label() == MAIN_LABEL => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("QuickPaste 启动失败");
}

/* ────────────── 热键 Alt+Z（支持动态修改） ────────────── */

/// 注册/切换呼出快捷键：先移除旧组合，再注册新组合
pub fn apply_hotkey(app: &AppHandle, combo: &str) {
    let st = app.state::<HotkeyState>();
    let prev = st.cur.lock().unwrap().clone();
    if let Some(ref p) = prev {
        if p == combo {
            return;
        }
        let _ = app.global_shortcut().unregister(p.as_str());
    }
    let ok = app
        .global_shortcut()
        .on_shortcut(combo, |app, _sc, ev| {
            if ev.state() == ShortcutState::Pressed {
                toggle_main(app);
            }
        });
    match ok {
        Ok(_) => {
            *st.cur.lock().unwrap() = Some(combo.to_string());
        }
        Err(_e) => {
            // 新组合注册失败：回退旧组合
            let _ = app.global_shortcut().unregister(combo);
            let _ = app
                .global_shortcut()
                .on_shortcut(prev.as_deref().unwrap_or("Ctrl+Alt+Z"), |app, _sc, ev| {
                    if ev.state() == ShortcutState::Pressed {
                        toggle_main(app);
                    }
                });
            *st.cur.lock().unwrap() = prev;
            let _ = settings::set_key(app, "hotkey", serde_json::json!(st.cur.lock().unwrap().clone().unwrap_or_default()));
        }
    }
}

/* ────────────── 系统托盘：图标 + 右击菜单 ────────────── */

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", "显示主界面", true, None::<&str>)?,
            &MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?,
            &MenuItem::with_id(app, "about", "关于与升级", true, None::<&str>)?,
            &MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?,
        ],
    )?;
    TrayIconBuilder::with_id("qp-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("QuickPaste · 快捷粘贴")
        .menu(&menu)
        // 左键不弹菜单（左键仅呼出/隐藏；菜单只在右键显示）
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => toggle_main(app),
            "settings" => {
                toggle_main(app);
                let _ = app.emit("qp:open-settings", ());
            }
            "about" => {
                let _ = cmds::qp_show_about(app.clone());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 左键单击：呼出/隐藏主界面（右键由系统弹出菜单）
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/* ────────────── 窗口辅助 ────────────── */

fn toggle_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.unminimize(); // 从任务栏/最小化状态还原
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}

/* ────────────── 几何记忆 ────────────── */

fn restore_geometry(app: &mut App) -> tauri::Result<()> {
    let Some(w) = app.get_webview_window(MAIN_LABEL) else {
        return Ok(());
    };
    let geo = settings::get_key(app.handle(), GEOM_KEY).ok().flatten();
    match geo.and_then(|v| serde_json::from_value::<GeomValue>(v).ok()) {
        // 最小化/隐藏期间可能存到异常几何（-32000 偏移、极小尺寸），视为无效 → 居中
        Some(g) if g.w >= 200 && g.h >= 200 && g.x > -20000 && g.y > -20000 => {
            let _ = w.set_position(Position::Physical(PhysicalPosition::new(g.x, g.y)));
            let _ = w.set_size(Size::Physical(PhysicalSize::new(g.w, g.h)));
        }
        _ => {
            let _ = w.center();
        }
    }
    let _ = w.show();
    let _ = w.set_focus();
    Ok(())
}

fn schedule_geom_save(app: AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut last = state.geom_saved_at.lock().unwrap();
        if last.elapsed() < Duration::from_millis(GEOM_DEBOUNCE_MS) {
            return;
        }
        *last = Instant::now();
    }
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        // 最小化/隐藏期间窗口几何不代表用户布局，不保存
        if w.is_minimized().unwrap_or(true) || !w.is_visible().unwrap_or(false) {
            return;
        }
        if let (Ok(pos), Ok(size)) = (w.outer_position(), w.outer_size()) {
            let v = serde_json::json!({ "x": pos.x, "y": pos.y, "w": size.width, "h": size.height });
            let _ = settings::set_key(&app, GEOM_KEY, v);
        }
    }
}

/* ────────────── 自启 ────────────── */

fn apply_autostart(app: &mut App) -> tauri::Result<()> {
    use tauri_plugin_autostart::ManagerExt;
    let enable = settings::get_key(app.handle(), "autostart")
        .ok()
        .flatten()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if enable {
        let _ = app.autolaunch().enable();
    }
    Ok(())
}

/* ────────────── 数据迁移：AppData(C盘) → 安装路径 ────────────── */

/// 旧版本数据在 C 盘 AppData（com.qpaste.quickpaste）；新版本统一存安装路径。
/// 逐项补齐：新位置缺失的项才复制，已有数据保留（可重复调用，天然幂等）。
fn migrate_data(app: &AppHandle) {
    let new_root = paths::data_root();
    let old = match app.path().app_data_dir() {
        Ok(o) if o != new_root && o.exists() => o,
        _ => return,
    };
    // settings.json
    if !new_root.join(settings::SETTINGS_FILE).exists() {
        let _ = std::fs::copy(old.join(settings::SETTINGS_FILE), new_root.join(settings::SETTINGS_FILE));
    }
    // history：逐项补齐（不覆盖已有文件）
    if let Ok(entries) = std::fs::read_dir(old.join("history")) {
        let hdst = new_root.join("history");
        for e in entries.flatten() {
            let dp = hdst.join(e.file_name());
            if !dp.exists() {
                if e.path().is_dir() {
                    copy_dir_all(&e.path(), &dp);
                } else if e.metadata().map(|m| m.is_file()).unwrap_or(false) {
                    let _ = std::fs::copy(e.path(), &dp);
                }
            }
        }
    }
    // cache / downloads：缺失才整体复制
    for name in ["cache", "downloads"] {
        if !new_root.join(name).exists() {
            copy_dir_all(&old.join(name), &new_root.join(name));
        }
    }
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(src) {
        let _ = std::fs::create_dir_all(dst);
        for e in entries.flatten() {
            let sp = e.path();
            let dp = dst.join(e.file_name());
            if sp.is_dir() {
                copy_dir_all(&sp, &dp);
            } else {
                let _ = std::fs::copy(&sp, &dp);
            }
        }
    }
}