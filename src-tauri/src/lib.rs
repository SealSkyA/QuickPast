//! QuickCopy 桌面骨架（S0）：全局热键、置顶、窗口几何记忆、设置持久化。
//! 扩展点：`modules/` 模块注册表（见 docs/实施/02-全局产品约束与扩展架构.md 4.2）。

pub mod cmds;
pub mod history;
pub mod net;
pub mod paths;
pub mod settings;
pub mod single;

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{App, AppHandle, CustomMenuItem, Manager, PhysicalPosition, PhysicalSize, Position, RunEvent, Size, SystemTray, SystemTrayEvent, SystemTrayMenu, WindowEvent};

pub const MAIN_LABEL: &str = "main";
const GEOM_KEY: &str = "window.geometry";
const GEOM_DEBOUNCE_MS: u64 = 900;

struct AppState {
    geom_saved_at: Mutex<Instant>,
    /// 退出/销毁中：屏蔽后续几何保存（退出过程的事件会把位置/尺寸写坏，导致下次启动偏移）
    exiting: Mutex<bool>,
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
            exiting: Mutex::new(false),
            last_copied: Mutex::new(None),
            viewer_id: Mutex::new(None),
        }
    }
}

/// 在线更新安装包下载状态（前端轮询进度）
#[derive(Default)]
pub struct DownloadState {
    pub inner: Mutex<DownloadStateInner>,
}

#[derive(Default)]
pub struct DownloadStateInner {
    pub phase: String,          // idle | downloading | done | error
    pub size: u64,              // 安装包总大小（字节，0=未知）
    pub path: Option<String>,   // 下载完成后的完整路径
    pub error: Option<String>,  // 失败原因
    pub tmp: Option<std::path::PathBuf>, // 下载中临时文件路径（用于统计进度）
}

/// 当前注册的呼出快捷键（支持设置页动态修改）
/// 内部记录「组合字符串 + 已注册的 HotKey」：比对用字符串，退订用 HotKey 对象
struct HotkeyState {
    cur: Mutex<Option<(String, global_hotkey::hotkey::HotKey)>>,
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
    // 单实例：独占端口失败说明已有实例 → 唤醒它（呼出主窗口）后本进程退出
    let listener = match single::acquire() {
        Some(l) => l,
        None => {
            single::poke_existing();
            std::process::exit(0);
        }
    };
    let builder = tauri::Builder::default()
        .manage(AppState::default())
        .manage(HotkeyState::default())
        .manage(history::History::default())
        .manage(DownloadState::default())
        .manage(net::NetState::default())
        .system_tray(build_tray())
        .on_menu_event(|event| {
            // v1：WindowMenuEvent 仅 1 参，经 event.window() 取 AppHandle
            let app = event.window().app_handle();
            match event.menu_item_id().as_ref() {
                "show" => toggle_main(&app, true),
                "settings" => {
                    toggle_main(&app, true);
                    let _ = app.emit_all("qp:open-settings", ());
                }
                "about" => {
                    let _ = cmds::qp_show_about(app.clone());
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_system_tray_event(|app, event| {
            // 左键单击：呼出/隐藏主界面（右键由系统弹出菜单）
            if let SystemTrayEvent::LeftClick { .. } = event {
                toggle_main(app, true);
            }
        })
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
            cmds::qp_show_recv,
            cmds::qp_set_autostart,
            cmds::qp_window_toggle_max,
            cmds::about_info,
            cmds::qp_check_update,
            cmds::qp_download_update,
            cmds::qp_download_progress,
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
            history::clip_current,
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
            net::net_my_info,
            cmds::qp_save_drag_files,
            cmds::qp_pick_files,
            cmds::qp_pick_dir,
            cmds::qp_pick_save
        ])
        .setup(move |app| {
            // 数据迁移：旧版本数据在 C 盘 AppData，新版本统一存安装路径 → 一次性复制
            migrate_data(&app.handle());
            // 单实例：维持独占监听，后续启动实例在此处呼出主窗口
            single::spawn_wake_listener(app.handle().clone(), listener);
            // S1 剪贴板历史：载入持久化数据（含存量去重）并启动监听
            let loaded = history::load_items(&app.handle());
            {
                let st = app.state::<history::History>();
                *st.items.lock().unwrap() = loaded;
            }
            history::prune_dups(&app.handle());
            // 类别数量上限：启动时把已超限的旧数据裁剪到上限内
            history::prune_by_limits(&app.handle());
            history::spawn_watcher(app.handle().clone());
            // S2 传输核心：组播发现 + HTTP 服务器
            net::spawn_discovery(app.handle().clone());
            net::spawn_server(app.handle().clone());
            // 发送侧端到端自测钩子（QP_SEND_TEST="ip:port|path1;path2"）
            if let Ok(v) = std::env::var("QP_SEND_TEST") {
                net::test_send(app.handle().clone(), &v);
            }
            // 呼出快捷键（支持设置页修改；默认 Ctrl+Alt+Z）
            let hotkey = settings::get_key(&app.handle(), "hotkey")
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
            let _ = settings::set_key(&app.handle(), "hotkey", serde_json::json!(hotkey));
            apply_hotkey(&app.handle(), &hotkey);
            restore_geometry(app)?;
            // 开机自启：静默启动（不显示主窗口，托盘/热键常驻）。
            // 仅在带 --hidden 参数启动时静默（开机自启注册表写入该参数）；手动双击启动 → 正常显示窗口。
            if std::env::args().any(|a| a == "--hidden") {
                if let Some(w) = app.get_window(MAIN_LABEL) {
                    let _ = w.set_skip_taskbar(true);
                    let _ = w.hide();
                }
            }
            restore_geometry(app)?;
            Ok(())
        })
        .on_window_event(|ev| match ev.event() {
            WindowEvent::Moved(_) | WindowEvent::Resized(_) if ev.window().label() == MAIN_LABEL => {
                schedule_geom_save(ev.window().app_handle());
            }
            // 窗口销毁：此后所有几何保存一律跳过（退出销毁过程中读到的位置/尺寸不可靠）
            WindowEvent::Destroyed if ev.window().label() == MAIN_LABEL => {
                let h = ev.window().app_handle();
                let state = h.state::<AppState>();
                *state.exiting.lock().unwrap() = true;
            }
            // ×/系统关闭请求：不退出应用，隐藏到系统托盘（进程常驻）
            WindowEvent::CloseRequested { api, .. } if ev.window().label() == MAIN_LABEL => {
                api.prevent_close();
                let _ = ev.window().set_skip_taskbar(true);
                let _ = ev.window().hide();
            }
            _ => {}
        });
    let app = builder.build(tauri::generate_context!()).expect("QuickCopy 启动失败");
    app.run(|handle, event| {
        // 真正退出前：保存一次干净几何（窗口此刻仍是用户布局），并屏蔽退出过程的后续保存
        if let RunEvent::ExitRequested { .. } = event {
            let state = handle.state::<AppState>();
            let mut ex = state.exiting.lock().unwrap();
            if !*ex {
                *ex = true;
                save_geom_now(handle);
            }
        }
    });
}

/* ────────────── 热键呼出（支持动态修改） ────────────── */
// Tauri 1 无官方 global-shortcut 插件发布 → 改用独立 global-hotkey crate（Win API / Linux X11，Wayland 不支持）。
// 全局事件回调只能注册一次：经 OnceLock 的 AppHandle 调用 toggle_main。

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

/// 事件回调用的 AppHandle（OnceLock：首次 apply_hotkey 时写入）
static HK_APP: OnceLock<AppHandle> = OnceLock::new();
/// 全局快捷键管理器（长驻保活）。
/// Windows 平台 GlobalHotKeyManager 含裸指针（非 Send/Sync）→ 包一层并显式标记，
/// 且所有调用都在 Mutex 独占下进行（apply_hotkey 本身只会串行出现）。
struct HotkeyMgr(GlobalHotKeyManager);
unsafe impl Send for HotkeyMgr {}
unsafe impl Sync for HotkeyMgr {}
static HK_MGR: OnceLock<Mutex<HotkeyMgr>> = OnceLock::new();

/// 热键调试日志（%TEMP%\qp-hotkey.log）
fn hlog(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("qp-hotkey.log")) {
        let _ = f.write_all(format!("{msg}\n").as_bytes());
    }
}

/// 解析 "Ctrl+Alt+Z" / "Alt+F9" 等组合（大小写、分隔符容忍）
fn parse_combo(s: &str) -> Option<HotKey> {
    let mut mods = Modifiers::empty();
    let mut key: Option<Code> = None;
    for p in s.split('+') {
        let up = p.trim().to_uppercase();
        match up.as_str() {
            "CTRL" | "CONTROL" | "CMD" => mods |= Modifiers::CONTROL,
            "ALT" | "OPT" => mods |= Modifiers::ALT,
            "SHIFT" => mods |= Modifiers::SHIFT,
            "META" | "SUPER" | "WIN" | "LOGO" => mods |= Modifiers::META,
            "F1" => key = Some(Code::F1),
            "F2" => key = Some(Code::F2),
            "F3" => key = Some(Code::F3),
            "F4" => key = Some(Code::F4),
            "F5" => key = Some(Code::F5),
            "F6" => key = Some(Code::F6),
            "F7" => key = Some(Code::F7),
            "F8" => key = Some(Code::F8),
            "F9" => key = Some(Code::F9),
            "F10" => key = Some(Code::F10),
            "F11" => key = Some(Code::F11),
            "F12" => key = Some(Code::F12),
            "F13" => key = Some(Code::F13),
            "F14" => key = Some(Code::F14),
            "F15" => key = Some(Code::F15),
            "F16" => key = Some(Code::F16),
            "F17" => key = Some(Code::F17),
            "F18" => key = Some(Code::F18),
            "F19" => key = Some(Code::F19),
            "F20" => key = Some(Code::F20),
            "F21" => key = Some(Code::F21),
            "F22" => key = Some(Code::F22),
            "F23" => key = Some(Code::F23),
            "F24" => key = Some(Code::F24),
            "SPACE" | "SPACEBAR" => key = Some(Code::Space),
            // keyboard-types Code：字母仅 KeyA..KeyZ（无 from_char），显式映射
            _ if up.len() == 1 => key = Some(match up.chars().next()? {
                'A' => Code::KeyA,
                'B' => Code::KeyB,
                'C' => Code::KeyC,
                'D' => Code::KeyD,
                'E' => Code::KeyE,
                'F' => Code::KeyF,
                'G' => Code::KeyG,
                'H' => Code::KeyH,
                'I' => Code::KeyI,
                'J' => Code::KeyJ,
                'K' => Code::KeyK,
                'L' => Code::KeyL,
                'M' => Code::KeyM,
                'N' => Code::KeyN,
                'O' => Code::KeyO,
                'P' => Code::KeyP,
                'Q' => Code::KeyQ,
                'R' => Code::KeyR,
                'S' => Code::KeyS,
                'T' => Code::KeyT,
                'U' => Code::KeyU,
                'V' => Code::KeyV,
                'W' => Code::KeyW,
                'X' => Code::KeyX,
                'Y' => Code::KeyY,
                'Z' => Code::KeyZ,
                '0' => Code::Digit0,
                '1' => Code::Digit1,
                '2' => Code::Digit2,
                '3' => Code::Digit3,
                '4' => Code::Digit4,
                '5' => Code::Digit5,
                '6' => Code::Digit6,
                '7' => Code::Digit7,
                '8' => Code::Digit8,
                '9' => Code::Digit9,
                _ => return None,
            }),
            _ => return None,
        }
    }
    Some(HotKey::new(Some(mods), key?))
}

/// 注册/切换呼出快捷键：先移除旧组合，再注册新组合
pub fn apply_hotkey(app: &AppHandle, combo: &str) {
    hlog(&format!("apply_hotkey: combo={combo}"));
    let _ = HK_APP.set(app.clone());
    let manager = HK_MGR.get_or_init(|| {
        let m = GlobalHotKeyManager::new().unwrap();
        // 事件回调只注册一次：任何已注册热键按下都呼出主窗口（state 为公开字段）
        GlobalHotKeyEvent::set_event_handler(Some(|ev: GlobalHotKeyEvent| {
            if ev.state == HotKeyState::Pressed {
                hlog("hotkey PRESSED → toggle_main");
                if let Some(a) = HK_APP.get() {
                    toggle_main(a, false);
                }
            }
        }));
        Mutex::new(HotkeyMgr(m))
    });
    let Some(new) = parse_combo(combo) else {
        hlog(&format!("parse_combo 失败: {combo}"));
        return;
    };
    // 管理器调用全部在 Mutex 独占下进行（HotkeyMgr 含裸指针，禁止并发触碰）
    let mgr = manager.lock().unwrap();
    let st = app.state::<HotkeyState>();
    let mut cur = st.cur.lock().unwrap();
    if let Some((old_s, old_h)) = cur.as_ref() {
        if old_s == combo {
            hlog("same, skip");
            return;
        }
        hlog(&format!("unregister old: {old_s}"));
        // HotKey 为 Copy：按值传递（global-hotkey 0.9 register/unregister 均接收 owned）
        let _ = mgr.0.unregister(*old_h);
    }
    // Copy 语义：register 按值后 new 仍可用
    match mgr.0.register(new) {
        Ok(_) => {
            hlog("register ok");
            *cur = Some((combo.to_string(), new));
        }
        Err(e) => {
            hlog(&format!("register FAILED: {e}"));
            let _ = mgr.0.unregister(new);
            // 回滚到旧热键（如有）并回写设置
            if let Some((old_s, old_h)) = cur.take() {
                if mgr.0.register(old_h).is_ok() {
                    *cur = Some((old_s.clone(), old_h));
                }
                let _ = settings::set_key(app, "hotkey", serde_json::json!(old_s));
            } else if let Some(fallback) = parse_combo("Ctrl+Alt+Z") {
                if mgr.0.register(fallback).is_ok() {
                    *cur = Some(("Ctrl+Alt+Z".to_string(), fallback));
                    let _ = settings::set_key(app, "hotkey", serde_json::json!("Ctrl+Alt+Z"));
                }
            }
        }
    }
}

/* ────────────── 系统托盘：图标 + 右击菜单 ────────────── */
// Tauri 1（Linux webkit2gtk-4.0）：托盘用 SystemTray + SystemTrayMenu，
// 由 Builder 在构建时注册（.system_tray/.on_menu_event/.on_system_tray_event）。

fn build_tray() -> SystemTray {
    let show = CustomMenuItem::new("show".to_string(), "显示主界面");
    let settings = CustomMenuItem::new("settings".to_string(), "设置");
    let about = CustomMenuItem::new("about".to_string(), "关于与升级");
    let quit = CustomMenuItem::new("quit".to_string(), "退出");
    let tray_menu = SystemTrayMenu::new()
        .add_item(show)
        .add_item(settings)
        .add_item(about)
        .add_item(quit);
    SystemTray::new()
        .with_id("qp-tray")
        .with_tooltip("QuickCopy · 快捷拷贝")
        .with_menu(tray_menu)
}

/* ────────────── 窗口辅助 ────────────── */

fn toggle_main(app: &AppHandle, show_taskbar: bool) {
    if let Some(w) = app.get_window(MAIN_LABEL) {
        hlog(&format!("toggle_main(show_taskbar={show_taskbar}): visible={:?}", w.is_visible()));
        if w.is_visible().unwrap_or(false) {
            let _ = w.set_skip_taskbar(true); // 隐藏 → 任务栏图标一并移除（只留托盘图标）
            let r = w.hide();
            hlog(&format!("hide → {:?}", r));
        } else {
            let _ = w.unminimize(); // 从任务栏/最小化状态还原
            let _ = w.show();
            // 快捷键呼出：任务栏不显示图标（只留托盘）；单击托盘呼出：任务栏图标正常显示
            let _ = w.set_skip_taskbar(!show_taskbar);
            let _ = w.set_focus();
            hlog("show+focus");
        }
    } else {
        hlog("toggle_main: main window NOT FOUND");
    }
}

/* ────────────── 几何记忆 ────────────── */

fn restore_geometry(app: &mut App) -> tauri::Result<()> {
    let Some(w) = app.get_window(MAIN_LABEL) else {
        return Ok(());
    };
    let geo = settings::get_key(&app.handle(), GEOM_KEY).ok().flatten();
    match geo.and_then(|v| serde_json::from_value::<GeomValue>(v).ok()) {
        // 最小化/隐藏期间可能存到异常几何（-32000 偏移、极小尺寸），视为无效 → 居中
        Some(g) if g.w >= 200 && g.h >= 200 && g.x > -20000 && g.y > -20000 => {
            // 先定尺寸再定位置：避免设置尺寸时 Windows 调整窗口原点造成漂移
            let _ = w.set_size(Size::Physical(PhysicalSize::new(g.w, g.h)));
            let _ = w.set_position(Position::Physical(PhysicalPosition::new(g.x, g.y)));
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
    if *state.exiting.lock().unwrap() {
        return;
    }
    {
        let mut last = state.geom_saved_at.lock().unwrap();
        if last.elapsed() < Duration::from_millis(GEOM_DEBOUNCE_MS) {
            return;
        }
        *last = Instant::now();
    }
    save_geom_now(&app);
}

/// 立即保存主窗口几何：位置取 outer（与 set_position 同语义），尺寸取 inner（与 set_size 同语义）
fn save_geom_now(app: &AppHandle) {
    if let Some(w) = app.get_window(MAIN_LABEL) {
        // 最小化/隐藏期间窗口几何不代表用户布局，不保存
        if w.is_minimized().unwrap_or(true) || !w.is_visible().unwrap_or(false) {
            return;
        }
        if let (Ok(pos), Ok(size)) = (w.outer_position(), w.inner_size()) {
            let v = serde_json::json!({ "x": pos.x, "y": pos.y, "w": size.width, "h": size.height });
            let _ = settings::set_key(app, GEOM_KEY, v);
        }
    }
}

/* ────────────── 数据迁移：AppData(C盘) → 安装路径 ────────────── */

/// 旧版本数据在 C 盘 AppData（com.qpaste.quickpaste）；新版本统一存安装路径。
/// 逐项补齐：新位置缺失的项才复制，已有数据保留（可重复调用，天然幂等）。
fn migrate_data(app: &AppHandle) {
    let new_root = paths::data_root();
    // Tauri 1：路径解析器返回 Option（v2 为 Result）
    let old = match app.path_resolver().app_data_dir() {
        Some(o) if o != new_root && o.exists() => o,
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