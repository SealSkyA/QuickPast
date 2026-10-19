//! 数据根目录策略：所有数据存放在安装路径（exe 所在目录）下的 data/ 目录，
//! 不再使用 C 盘 AppData（用户明确要求：数据不放 C 盘）。
//! 安装版：<安装目录>/data；开发模式：target/debug/data（与源码目录同盘）。

use std::path::PathBuf;

/// 应用数据根目录：`<exe所在目录>/data`
pub fn data_root() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.parent()
        .map(|p| p.join("data"))
        .unwrap_or_else(|| PathBuf::from("data"))
}
