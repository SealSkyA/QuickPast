//! 设置持久化：统一走 tauri-plugin-store 落盘到 settings.json（app_config_dir）。
//! 前端通过自定义命令 get_setting/set_setting 访问，密钥语义见 UI 设置页。

use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub const SETTINGS_FILE: &str = "settings.json";

pub fn get_key(app: &AppHandle, key: &str) -> Result<Option<Value>, tauri_plugin_store::Error> {
    let store = app.store(SETTINGS_FILE)?;
    Ok(store.get(key))
}

pub fn set_key(
    app: &AppHandle,
    key: &str,
    value: Value,
) -> Result<(), tauri_plugin_store::Error> {
    let store = app.store(SETTINGS_FILE)?;
    store.set(key.to_string(), value);
    store.save()?;
    Ok(())
}