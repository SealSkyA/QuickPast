//! 设置持久化：读写 <安装路径>/data/settings.json（JSON 文件）。
//! 前端通过自定义命令 get_setting/set_setting 访问，密钥语义见 UI 设置页。

use std::fs;
use std::sync::Mutex;

use serde_json::{Map, Value};
use tauri::AppHandle;

use crate::paths;

pub const SETTINGS_FILE: &str = "settings.json";

// 进程内串行化文件读写（多线程同时写会互相覆盖）
static LOCK: Mutex<()> = Mutex::new(());

fn settings_path() -> std::path::PathBuf {
    paths::data_root().join(SETTINGS_FILE)
}

fn read_map() -> Map<String, Value> {
    let _g = LOCK.lock().unwrap();
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Map<String, Value>>(&s).ok())
        .unwrap_or_default()
}

fn write_map(map: &Map<String, Value>) -> Result<(), String> {
    let _g = LOCK.lock().unwrap();
    let dir = settings_path()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
    fs::write(settings_path(), json).map_err(|e| e.to_string())
}

pub fn get_key(_app: &AppHandle, key: &str) -> Result<Option<Value>, String> {
    Ok(read_map().get(key).cloned())
}

pub fn set_key(_app: &AppHandle, key: &str, value: Value) -> Result<(), String> {
    let mut map = read_map();
    map.insert(key.to_string(), value);
    write_map(&map)
}
