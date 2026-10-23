//! 应用数据根目录策略：
//! - Windows：所有数据存放在安装路径（exe 所在目录）下的 data/ 目录，
//!   不再使用 C 盘 AppData（用户明确要求：数据不放 C 盘）。
//!   安装版：<安装目录>/data；开发模式：target/debug/data（与源码目录同盘）。
//! - Linux（麒麟/UOS）：deb 安装到 /usr（只读），数据必须放用户目录
//!   → $HOME/.local/share/quickcopy（XDG 数据目录，跨发行版稳定）。

use std::path::PathBuf;

/// 应用数据根目录
pub fn data_root() -> PathBuf {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().unwrap_or_default();
        return exe
            .parent()
            .map(|p| p.join("data"))
            .unwrap_or_else(|| PathBuf::from("data"));
    }
    #[cfg(not(windows))]
    {
        if let Ok(h) = std::env::var("HOME") {
            if !h.is_empty() {
                return PathBuf::from(&h).join(".local/share/quickcopy");
            }
        }
        std::env::temp_dir().join("quickcopy-data")
    }
}