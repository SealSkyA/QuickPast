//! S1 剪贴板历史：轮询系统剪贴板（文本/图片/文件），持久化 JSON，事件推送。
//! 命令：history_load / history_remove / history_clear / history_set_fav / history_pin
//!       / history_edit / clipboard_set_text / clipboard_set_files / clipboard_set_img
//!       / clip_img_b64

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use chrono::Local;
use clipboard_rs::{Clipboard, common::RustImage};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::settings;

pub const LIMIT: usize = 600;

/// 各类别数量上限（设置可调；默认 文字 200 / 图片 200 / 文件 100）
fn cat_limit(app: &AppHandle, cat: &str) -> usize {
    let key = match cat {
        "text" => "history.limitText",
        "img" => "history.limitImg",
        "file" => "history.limitFile",
        _ => return 0,
    };
    let dft: usize = match cat { "text" => 200, "img" => 200, "file" => 100, _ => 0 };
    settings::get_key(app, key)
        .ok()
        .flatten()
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(dft)
        .max(1)
}

/// 按类别上限裁剪：某类数量超过上限时，从最老的一条开始删除（连同内容文件），
/// 直到该类数量 ≤ 上限。返回被删条目的 id。
pub fn prune_by_limits(app: &AppHandle) -> Vec<u64> {
    let st = app.state::<History>();
    let mut items = st.items.lock().unwrap();
    let mut removed: Vec<(u64, String)> = Vec::new();
    for cat in ["text", "img", "file"] {
        let limit = cat_limit(app, cat);
        let mut over = items.iter().filter(|i| i.cat == cat).count().saturating_sub(limit);
        let mut idx = items.len();
        while over > 0 && idx > 0 {
            idx -= 1;
            if items[idx].cat == cat {
                let (id, c) = (items[idx].id, items[idx].cat.clone());
                items.remove(idx);
                removed.push((id, c));
                over -= 1;
            }
        }
    }
    if removed.is_empty() {
        return Vec::new();
    }
    drop(items);
    for (id, cat) in &removed {
        match cat.as_str() {
            "text" => {
                let _ = std::fs::remove_file(text_dir(&app).join(format!("{id}.txt")));
                let _ = std::fs::remove_file(text_dir(&app).join(format!("{id}.html")));
            }
            "img" => {
                let _ = std::fs::remove_file(img_dir(&app).join(format!("{id}.png")));
            }
            "file" => {
                let _ = std::fs::remove_dir_all(file_dir(&app).join(format!("{id}")));
            }
            _ => {}
        }
    }
    save_items(&app, &st.items.lock().unwrap());
    removed.iter().map(|(id, _)| *id).collect()
}

/// 设置页保存上限后立即应用（超出部分删最早）
#[tauri::command]
pub fn history_apply_limits(app: AppHandle) -> Vec<u64> {
    prune_by_limits(&app)
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Item {
    pub id: u64,
    pub cat: String, // text | img | file | note
    pub fav: bool,
    pub dt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>, // 文本正文（索引不落盘，存于 text/{id}.txt）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub res: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>, // 文件原路径（| 分隔）或本地图片存储路径
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_rich: Option<bool>, // 剪贴板带 HTML 格式（复制为格式时用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_copy: Option<bool>, // 最近一次复制的条目（列表图标标记为应用图标黄色）
}

#[derive(Clone, PartialEq, Debug)]
pub enum ClipKey {
    Text(String),
    Img(Vec<u8>), // 原始图片字节指纹
    Files(Vec<String>),
}

pub struct History {
    pub items: Mutex<Vec<Item>>,
}

impl Default for History {
    fn default() -> Self {
        History {
            items: Mutex::new(Vec::new()),
        }
    }
}

/* ────────────── 存储（按类别分文件夹）──────────────
   历史数据根：<app_data>/history/
     index.json   条目索引（id/cat/fav/dt/name/size/res/path/pin）
     text/{id}.txt  文本内容
     img/{id}.png   图片快照
     file/{id}/…    文件快照（复制副本）
──────────────────────────────────────────────────── */

fn data_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("history")
}

fn index_path(app: &AppHandle) -> PathBuf {
    data_dir(app).join("index.json")
}

fn text_dir(app: &AppHandle) -> PathBuf {
    data_dir(app).join("text")
}

fn img_dir(app: &AppHandle) -> PathBuf {
    data_dir(app).join("img")
}

fn file_dir(app: &AppHandle) -> PathBuf {
    data_dir(app).join("file")
}

fn ensure_dirs(app: &AppHandle) {
    for d in [data_dir(app), text_dir(app), img_dir(app), file_dir(app)] {
        let _ = std::fs::create_dir_all(&d);
    }
}

pub fn load_items(app: &AppHandle) -> Vec<Item> {
    let items: Vec<Item> = std::fs::read_to_string(index_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    // 内存态补回正文（索引文件保持精简；正文用于历史查重/显示）
    items
        .into_iter()
        .map(|mut i| {
            if i.cat == "text" || i.cat == "note" {
                i.body = std::fs::read_to_string(text_dir(app).join(format!("{}.txt", i.id))).ok();
            }
            i
        })
        .collect()
}

/// 启动时清理存量重复条目（同内容保留最新一条），并删除其内容文件
pub fn prune_dups(app: &AppHandle) {
    let st = app.state::<History>();
    let mut items = st.items.lock().unwrap();
    if items.len() < 2 {
        return;
    }
    let mut removed = Vec::new();
    let mut kept: Vec<Item> = Vec::with_capacity(items.len());
    for x in items.iter() {
        let dup = kept.iter().any(|k| {
            match (k.cat.as_str(), x.cat.as_str()) {
                ("text", "text") => x.body.as_deref().map(|b| b.trim()) == k.body.as_deref().map(|b| b.trim()),
                ("file", "file") => x.path == k.path,
                _ => false,
            }
        });
        if dup {
            removed.push(x.id);
        } else {
            kept.push(x.clone());
        }
    }
    if removed.is_empty() {
        return;
    }
    *items = kept;
    drop(items);
    for rid in &removed {
        let _ = std::fs::remove_file(text_dir(&app).join(format!("{rid}.txt")));
        let _ = std::fs::remove_file(text_dir(&app).join(format!("{rid}.html")));
        let _ = std::fs::remove_file(img_dir(&app).join(format!("{rid}.png")));
        let _ = std::fs::remove_dir_all(file_dir(&app).join(format!("{rid}")));
    }
    save_items(&app, &st.items.lock().unwrap());
}

fn save_items(app: &AppHandle, items: &[Item]) {
    // 索引保持精简：正文只存 text/{id}.txt，序列化前剥离 body
    let slim: Vec<Item> = items
        .iter()
        .map(|i| {
            let mut c = i.clone();
            c.body = None;
            c
        })
        .collect();
    if let Ok(s) = serde_json::to_string(&slim) {
        ensure_dirs(app);
        let _ = std::fs::write(index_path(app), s);
    }
}

/// 把"上次复制"标记设到指定条目（其余条目清空标记），持久化并通知前端渲染
pub fn mark_last_copy(app: &AppHandle, id: u64) {
    let st = app.state::<History>();
    let mut items = st.items.lock().unwrap();
    let mut changed = false;
    for i in items.iter_mut() {
        let want = Some(i.id == id);
        if i.last_copy != want {
            i.last_copy = want;
            changed = true;
        }
    }
    drop(items);
    if changed {
        save_items(app, &st.items.lock().unwrap());
    }
    let _ = app.emit("qp:clip", serde_json::json!({ "markId": id }));
}

/// 标记"上次复制"的条目（点击历史条目复制成功后由前端调用）
#[tauri::command]
pub fn history_mark_last(app: AppHandle, id: u64) {
    mark_last_copy(&app, id);
}

/// 把文本内容写入 text/{id}.txt（body 不再存于索引，按类分文件夹）
fn write_text(app: &AppHandle, id: u64, body: &str) {
    let p = text_dir(app).join(format!("{id}.txt"));
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
    let _ = std::fs::write(p, body);
}

/// 文件快照：把剪贴板中的文件复制到 file/{id}/，返回快照路径（| 分隔）
fn snapshot_files(app: &AppHandle, id: u64, paths: &[String]) -> String {
    let d = file_dir(app).join(format!("{id}"));
    let _ = std::fs::create_dir_all(&d);
    let mut snaps = Vec::new();
    for p in paths {
        let name = PathBuf::from(p)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let dest = d.join(&name);
        match std::fs::copy(p, &dest) {
            Ok(_) => snaps.push(dest.to_string_lossy().to_string()),
            Err(_) => snaps.push(p.clone()), // 复制失败退化为原路径引用
        }
    }
    snaps.join("|")
}

fn now_iso() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn fmt_size(n: u64) -> String {
    if n >= 1 << 30 {
        format!("{:.1} GB", n as f64 / (1 << 30) as f64)
    } else if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KB", n as f64 / (1 << 10) as f64)
    } else {
        format!("{n} B")
    }
}

fn next_id(items: &[Item]) -> u64 {
    items.iter().map(|i| i.id).max().unwrap_or(0) + 1
}

/* ────────────── 剪贴板监听线程 ────────────── */

pub fn spawn_watcher(app: AppHandle) {
    let mut last: Option<ClipKey> = None;
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(800));
        let ctx = match clipboard_rs::ClipboardContext::new() {
            Ok(c) => c,
            Err(_) => continue,
        };
        let found = poll_once(&ctx);
        let Some((key, mut it, img, html)) = found else { continue };
        if last.as_ref() == Some(&key) {
            continue; // 内容未变化，去重
        }
        last = Some(key.clone());
        // 历史去重：已存在相同内容 → 不新增，仅更新时间并移到顶部，
        // 同时清理同内容的其他重复项（只保留最新这条）
        {
            let st = app.state::<History>();
            let mut items = st.items.lock().unwrap();
            let same = |x: &Item| match &key {
                ClipKey::Text(t) => x.cat == "text" && x.body.as_deref().map(|b| b.trim()) == Some(t.trim()),
                ClipKey::Files(fs) => x.cat == "file" && x.path.as_deref() == Some(&fs.join("|")),
                ClipKey::Img(_) => false,
            };
            if let Some(pos) = items.iter().position(&same) {
                let mut dup = items.remove(pos);
                dup.dt = now_iso();
                let mut removed = Vec::new();
                items.retain(|x| {
                    if same(x) && x.id != dup.id {
                        removed.push(x.id);
                        return false;
                    }
                    true
                });
                // 保持原位置：复制已存在的条目不再置顶，仅更新时间并打上"上次复制"标记
                let ins = pos.min(items.len());
                items.insert(ins, dup.clone());
                drop(items);
                for rid in &removed {
                    let _ = std::fs::remove_file(text_dir(&app).join(format!("{rid}.txt")));
                    let _ = std::fs::remove_file(text_dir(&app).join(format!("{rid}.html")));
                }
                save_items(&app, &st.items.lock().unwrap());
                mark_last_copy(&app, dup.id);
                let _ = app.emit(
                    "qp:clip",
                    serde_json::json!({ "item": dup, "removed": removed }),
                );
                continue;
            }
        }
        it.dt = now_iso();
        let st = app.state::<History>();
        let mut items = st.items.lock().unwrap();
        it.id = next_id(&items);
        items.insert(0, it);
        items.truncate(LIMIT);
        // 内容按类别落盘到各自文件夹
        match items[0].cat.as_str() {
            "text" | "note" => {
                if let Some(b) = items[0].body.clone() {
                    write_text(&app, items[0].id, &b);
                }
                // 富文本（HTML）：同目录另存 .html，供"复制格式"回放
                if let Some(h) = html {
                    let _ = std::fs::create_dir_all(text_dir(&app));
                    let _ = std::fs::write(text_dir(&app).join(format!("{}.html", items[0].id)), h);
                    items[0].has_rich = Some(true);
                }
            }
            "file" => {
                let srcs: Vec<String> = items[0]
                    .path
                    .clone()
                    .unwrap_or_default()
                    .split('|')
                    .map(|s| s.to_string())
                    .collect();
                items[0].path = Some(snapshot_files(&app, items[0].id, &srcs));
            }
            "img" => {
                if let Some(img) = img {
                    let _ = persist_img(&app, items[0].id, &img);
                }
            }
            _ => {}
        }
        let mut emit_item = items[0].clone();
        emit_item.last_copy = Some(true); // 系统新复制的条目 = 上次复制
        drop(items);
        save_items(&app, &st.items.lock().unwrap());
        // 类别数量上限：超出时删除最早的一条，并通知前端同步移除
        let pruned = prune_by_limits(&app);
        mark_last_copy(&app, emit_item.id);
        let _ = app.emit(
            "qp:clip",
            serde_json::json!({ "item": emit_item, "removed": pruned }),
        );
    });
}

fn poll_once(
    ctx: &clipboard_rs::ClipboardContext,
) -> Option<(ClipKey, Item, Option<clipboard_rs::RustImageData>, Option<String>)> {
    // 文件优先（复制文件时剪贴板也可能带文本路径）
    if let Ok(files) = ctx.get_files() {
        if !files.is_empty() {
            let total = files
                .iter()
                .map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0))
                .sum();
            let name = PathBuf::from(&files[0])
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            return Some((
                ClipKey::Files(files.clone()),
                Item {
                    cat: "file".into(),
                    name: Some(name),
                    size: Some(fmt_size(total)),
                    path: Some(files.join("|")),
                    ..Default::default()
                },
                None,
                None,
            ));
        }
    }
    // 图片
    if let Ok(img) = ctx.get_image() {
        let (w, h) = img.get_size();
        let raw = img
            .to_rgba8()
            .map(|r| r.as_raw().clone())
            .unwrap_or_default();
        let key = ClipKey::Img(raw);
        let size = (w * h * 4) as u64;
        let name = format!("截图_{}.png", Local::now().format("%Y%m%d_%H%M%S"));
        return Some((
            key,
            Item {
                cat: "img".into(),
                name: Some(name),
                res: Some(format!("{w} × {h}")),
                size: Some(fmt_size(size)),
                ..Default::default()
            },
            Some(img),
            None,
        ));
    }
    // 富文本（HTML 格式优先：复制网页/带样式文本时保留格式）
    if let Ok(h) = ctx.get_html() {
        let t = ctx.get_text().unwrap_or_default();
        if !h.trim().is_empty() && !t.trim().is_empty() {
            return Some((
                ClipKey::Text(t.clone()),
                Item {
                    cat: "text".into(),
                    body: Some(t.clone()),
                    has_rich: Some(true),
                    ..Default::default()
                },
                None,
                Some(h),
            ));
        }
    }
    // 文本
    if let Ok(t) = ctx.get_text() {
        if !t.trim().is_empty() {
            return Some((
                ClipKey::Text(t.clone()),
                Item {
                    cat: "text".into(),
                    body: Some(t),
                    ..Default::default()
                },
                None,
                None,
            ));
        }
    }
    None
}

/* 图片条目：将剪贴板图片落盘（供缩略图与回写剪贴板） */
pub fn persist_img(
    app: &AppHandle,
    id: u64,
    img: &clipboard_rs::RustImageData,
) -> Result<(), String> {
    let d = img_dir(app);
    let _ = std::fs::create_dir_all(&d);
    let p = d.join(format!("{id}.png"));
    img.save_to_path(p.to_str().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/* ────────────── IPC 命令 ────────────── */

#[tauri::command]
pub fn history_load(app: AppHandle) -> Vec<Item> {
    let mut items = app.state::<History>().items.lock().unwrap().clone();
    // 文本内容存于 text/{id}.txt，读取时填充（索引保持精简）
    for it in &mut items {
        if it.cat == "text" || it.cat == "note" {
            let p = text_dir(&app).join(format!("{}.txt", it.id));
            if let Ok(s) = std::fs::read_to_string(&p) {
                it.body = Some(s);
            }
        }
    }
    items
}

#[tauri::command]
pub fn history_remove(app: AppHandle, id: u64) {
    let st = app.state::<History>();
    let mut items = st.items.lock().unwrap();
    let it = items.iter().find(|i| i.id == id).cloned();
    items.retain(|i| i.id != id);
    save_items(&app, &items);
    drop(items);
    if let Some(it) = it {
        // 清理对应内容文件
        if it.cat == "text" || it.cat == "note" {
            let _ = std::fs::remove_file(text_dir(&app).join(format!("{id}.txt")));
            let _ = std::fs::remove_file(text_dir(&app).join(format!("{id}.html")));
        } else if it.cat == "img" {
            let _ = std::fs::remove_file(img_dir(&app).join(format!("{id}.png")));
        } else if it.cat == "file" {
            let _ = std::fs::remove_dir_all(file_dir(&app).join(format!("{id}")));
        }
    }
}

#[tauri::command]
pub fn history_clear(app: AppHandle) {
    let st = app.state::<History>();
    let mut items = st.items.lock().unwrap();
    items.clear();
    save_items(&app, &items);
    drop(items);
    for d in [text_dir(&app), img_dir(&app), file_dir(&app)] {
        let _ = std::fs::remove_dir_all(&d);
    }
    ensure_dirs(&app);
}

#[tauri::command]
pub fn history_set_fav(app: AppHandle, id: u64, fav: bool) {
    let st = app.state::<History>();
    let mut items = st.items.lock().unwrap();
    if let Some(i) = items.iter_mut().find(|i| i.id == id) {
        i.fav = fav;
    }
    save_items(&app, &items);
}

#[tauri::command]
pub fn history_pin(app: AppHandle, id: u64, set: Option<bool>) {
    let st = app.state::<History>();
    let mut items = st.items.lock().unwrap();
    let now = chrono::Utc::now().timestamp_millis() as u64;
    if let Some(i) = items.iter_mut().find(|i| i.id == id) {
        i.pin = match set {
            Some(false) => None,
            _ => Some(now),
        };
    }
    items.sort_by(|a, b| b.pin.unwrap_or(0).cmp(&a.pin.unwrap_or(0)).then(a.id.cmp(&b.id)));
    save_items(&app, &items);
}

#[tauri::command]
pub fn history_edit(app: AppHandle, id: u64, body: Option<String>, name: Option<String>) {
    let st = app.state::<History>();
    let mut items = st.items.lock().unwrap();
    if let Some(i) = items.iter_mut().find(|i| i.id == id) {
        if let Some(b) = body {
            if i.cat == "text" || i.cat == "note" {
                write_text(&app, id, &b);
            }
            i.body = Some(b);
        }
        if let Some(n) = name {
            i.name = Some(n);
        }
    }
    save_items(&app, &items);
}

/// 新建便签（手动创建）：插入顶部并落盘，返回新条目
#[tauri::command]
pub fn history_add_note(app: AppHandle, body: String) -> Result<serde_json::Value, String> {
    let body = body.trim().to_string();
    if body.is_empty() {
        return Err("便签内容不能为空".into());
    }
    let st = app.state::<History>();
    let mut items = st.items.lock().unwrap();
    let it = Item {
        id: next_id(&items),
        cat: "note".to_string(),
        fav: false,
        dt: now_iso(),
        body: Some(body.clone()),
        name: None,
        res: None,
        size: None,
        path: None,
        pin: None,
        has_rich: None,
        last_copy: None,
    };
    write_text(&app, it.id, &body);
    items.insert(0, it.clone());
    items.truncate(LIMIT);
    let emit_item = it.clone();
    drop(items);
    save_items(&app, &st.items.lock().unwrap());
    let _ = app.emit("qp:clip", emit_item);
    serde_json::to_value(it).map_err(|e| e.to_string())
}

/* 复制条目回系统剪贴板 */
#[tauri::command]
pub fn clipboard_set_text(text: String) -> Result<(), String> {
    let ctx = clipboard_rs::ClipboardContext::new().map_err(|e| e.to_string())?;
    ctx.set_text(text).map_err(|e| e.to_string())
}

/// 复制条目为富文本（HTML 格式回放；无 HTML 时回退纯文本）
#[tauri::command]
pub fn history_copy_rich(app: AppHandle, id: u64) -> Result<(), String> {
    let st = app.state::<History>();
    let items = st.items.lock().unwrap();
    let Some(it) = items.iter().find(|i| i.id == id) else {
        return Err("条目不存在".into());
    };
    let plain = it.body.clone().unwrap_or_default();
    drop(items);
    let html = std::fs::read_to_string(text_dir(&app).join(format!("{id}.html"))).ok();
    let ctx = clipboard_rs::ClipboardContext::new().map_err(|e| e.to_string())?;
    match html {
        Some(h) if !h.trim().is_empty() => {
            // 多格式一次写入（HTML + 纯文本），避免二次写入清空
            ctx.set(vec![
                clipboard_rs::ClipboardContent::Html(h),
                clipboard_rs::ClipboardContent::Text(plain),
            ])
            .map_err(|e| e.to_string())?;
        }
        _ => ctx.set_text(plain).map_err(|e| e.to_string())?,
    }
    Ok(())
}

#[tauri::command]
pub fn clipboard_set_files(app: AppHandle, id: u64) -> Result<(), String> {
    let st = app.state::<History>();
    let items = st.items.lock().unwrap();
    let it = items.iter().find(|i| i.id == id).ok_or("条目不存在")?;
    let paths: Vec<String> = it
        .path
        .clone()
        .unwrap_or_default()
        .split('|')
        .map(|s| s.to_string())
        .collect();
    let ctx = clipboard_rs::ClipboardContext::new().map_err(|e| e.to_string())?;
    ctx.set_files(paths).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clipboard_set_img(app: AppHandle, id: u64) -> Result<(), String> {
    let st = app.state::<History>();
    let items = st.items.lock().unwrap();
    if !items.iter().any(|i| i.id == id) {
        return Err("条目不存在".into());
    }
    let p = img_dir(&app).join(format!("{id}.png"));
    let img = clipboard_rs::RustImageData::from_path(p.to_str().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    let ctx = clipboard_rs::ClipboardContext::new().map_err(|e| e.to_string())?;
    ctx.set_image(img).map_err(|e| e.to_string())
}

/// 图片缩略图（宽 160）base64，供前端渲染
#[tauri::command]
pub fn clip_img_b64(app: AppHandle, id: u64) -> Result<String, String> {
    let p = img_dir(&app).join(format!("{id}.png"));
    let img = image::open(&p).map_err(|e| e.to_string())?;
    let thumb = img.thumbnail(160, 160);
    let mut buf = std::io::Cursor::new(Vec::new());
    thumb
        .write_to(&mut buf, image::ImageOutputFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(base64(buf.get_ref()))
}

/// 图片快照本地路径（发送/打开用）
#[tauri::command]
pub fn clip_img_path(app: AppHandle, id: u64) -> Result<String, String> {
    let p = img_dir(&app).join(format!("{id}.png"));
    if p.exists() {
        Ok(p.to_string_lossy().to_string())
    } else {
        Err("图片不存在".into())
    }
}

/// 图片大图（宽 ≤1200）base64，供预览
#[tauri::command]
pub fn clip_img_full(app: AppHandle, id: u64) -> Result<String, String> {
    let p = img_dir(&app).join(format!("{id}.png"));
    let img = image::open(&p).map_err(|e| e.to_string())?;
    let big = img.thumbnail(1200, 1200);
    let mut buf = std::io::Cursor::new(Vec::new());
    big.write_to(&mut buf, image::ImageOutputFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(base64(buf.get_ref()))
}

fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((b.len() + 2) / 3 * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (c.get(1).copied().unwrap_or(0) as u32) << 8
            | c.get(2).copied().unwrap_or(0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// 调试：向剪贴板写入文本（用于无人工测试验证监听链路）
#[tauri::command]
pub fn qp_debug_clip_set_text(text: String) -> Result<(), String> {
    let ctx = clipboard_rs::ClipboardContext::new().map_err(|e| e.to_string())?;
    ctx.set_text(text).map_err(|e| e.to_string())
}

/// 调试：读取剪贴板 HTML（验证富文本回放链路）
#[tauri::command]
pub fn qp_debug_clip_get_html() -> Result<String, String> {
    let ctx = clipboard_rs::ClipboardContext::new().map_err(|e| e.to_string())?;
    ctx.get_html().map_err(|e| e.to_string())
}

/// 调试：向剪贴板写入 HTML + 纯文本（模拟网页复制，验证富文本捕获/复制格式链路）
#[tauri::command]
pub fn qp_debug_clip_set_html(html: String) -> Result<(), String> {
    let plain = html.replace("<b>", "").replace("</b>", "")
        .replace("<i>", "").replace("</i>", "")
        .replace("<br>", "\n").replace("<br/>", "\n").replace("&amp;", "&");
    let ctx = clipboard_rs::ClipboardContext::new().map_err(|e| e.to_string())?;
    ctx.set(vec![
        clipboard_rs::ClipboardContent::Html(html),
        clipboard_rs::ClipboardContent::Text(plain),
    ])
    .map_err(|e| e.to_string())
}
