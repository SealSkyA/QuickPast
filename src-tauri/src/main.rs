// Windows GUI 子系统（不弹控制台窗口；日志走文件/调试器）
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    quickpaste_lib::run();
}