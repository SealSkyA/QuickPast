//! 单实例（Tauri 1 无官方插件发布，自实现）：
//! 独占绑定 localhost 固定端口：已有实例 → 唤醒它并退出自身；运行期循环 accept，
//! 收到第二个实例的"敲门"连接时呼出主窗口（双击图标/再次启动 → 显示主界面）。
//! 跨平台（std::net），无第三方依赖。

use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use tauri::AppHandle;

/// 独占/唤醒端口（冷门端口，避免与应用自身协议端口冲突）
const POKE_PORT: u16 = 29371;

/// 尝试独占端口。
/// Some(listener)：本进程获得单实例许可（后续交给 spawn_wake_listener 维持独占）；
/// None：已有实例在运行。
pub fn acquire() -> Option<TcpListener> {
    TcpListener::bind(("127.0.0.1", POKE_PORT)).ok()
}

/// 唤醒已运行实例：向其端口发起一次连接（由监听侧呼出主窗口）。
pub fn poke_existing() {
    let _ = TcpStream::connect(("127.0.0.1", POKE_PORT));
}

/// 后台线程维持独占并等待唤醒连接：收到连接 → 呼出主窗口。
pub fn spawn_wake_listener(app: AppHandle, listener: TcpListener) {
    let _ = listener.set_nonblocking(true);
    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(_) => {
                    // 第二个实例启动：呼出主窗口（与旧插件行为一致：不重复开进程）
                    if let Some(w) = app.get_window(crate::MAIN_LABEL) {
                        let _ = w.unminimize();
                        let _ = w.show();
                        let _ = w.set_skip_taskbar(false);
                        let _ = w.set_focus();
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(200));
                }
                Err(_) => break,
            }
        }
    });
}