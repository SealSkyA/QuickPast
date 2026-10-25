//! S2 传输核心：LocalSend v2 兼容（规格见 docs/实施/01-协议实现规格.md）。
//! UDP 组播发现（224.0.0.167:53318 JSON announce → HTTP register 单播回执）
//! + HTTP 服务器（0.0.0.0:53318，register/info/prepare-upload/upload/cancel）
//! + 接收会话（单槽、IP 绑定、每文件 token、sha256 校验）+ 发送客户端。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use clipboard_rs::Clipboard;

pub const PORT: u16 = 53318;
const MC_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);
const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(10);
const DEVICE_TTL: Duration = Duration::from_secs(25); // 2 轮 announce 未回应 → 离线
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(60); // 接收确认等待
const VERSION: &str = "2.2";

/// 监听/广播端口：优先级 QP_PORT 环境变量 > 设置 net.port > 默认 53318
fn port(app: Option<&AppHandle>) -> u16 {
    if let Ok(p) = std::env::var("QP_PORT") {
        if let Ok(n) = p.parse::<u16>() {
            return n;
        }
    }
    if let Some(a) = app {
        if let Ok(Some(v)) = settings_get(a, "net.port") {
            if let Some(n) = v.as_u64() {
                if (1..=65535).contains(&n) {
                    return n as u16;
                }
            }
        }
    }
    PORT
}

/// 局域网服务在线开关（离线 = 不广播/不回执，别人检测不到；仍可接收直接发送）
fn net_online(app: Option<&AppHandle>) -> bool {
    if let Some(a) = app {
        if let Ok(Some(v)) = settings_get(a, "net.online") {
            return v.as_bool().unwrap_or(true);
        }
    }
    true
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 调试日志（写 %TEMP%\qp-net.log，便于无控制台验证）
fn log(msg: &str) {
    let p = std::env::temp_dir().join("qp-net.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        use std::io::Write;
        let _ = f.write_all(format!("[net {}] {msg}\n", chrono::Local::now().format("%H:%M:%S")).as_bytes());
    }
}

/* ────────────── 设备表 ────────────── */

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Device {
    pub fingerprint: String,
    pub alias: String,
    pub device_type: String,
    pub version: String,
    pub ip: String,
    pub port: u16,
    pub protocol: String,
    pub download: bool,
    pub last_seen: u64,
}

pub struct NetState {
    pub devices: Mutex<Vec<Device>>,
    /// 接收会话：sessionId → 会话
    pub sessions: Mutex<HashMap<String, RcvSession>>,
    /// Web 分享会话（链接下载）：sessionId → 会话
    pub shares: Mutex<HashMap<String, ShareSession>>,
    /// 发送取消令牌：sendId → 取消标志（发送线程持 Arc，UI「取消发送」置 true）
    pub send_cancel: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Default for NetState {
    fn default() -> Self {
        NetState {
            devices: Mutex::new(Vec::new()),
            sessions: Mutex::new(HashMap::new()),
            shares: Mutex::new(HashMap::new()),
            send_cancel: Mutex::new(HashMap::new()),
        }
    }
}

/* ────────────── 本机设备信息 ────────────── */

fn fingerprint(app: Option<&AppHandle>) -> String {
    if let Some(a) = app {
        if let Ok(Some(v)) = settings_get(a, "net.fingerprint") {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
        let fp = Uuid::new_v4().to_string();
        let _ = settings_set(a, "net.fingerprint", serde_json::json!(fp));
        return fp;
    }
    // 无 AppHandle（集成测试）：每次生成临时指纹
    Uuid::new_v4().to_string()
}

fn alias(app: Option<&AppHandle>) -> String {
    if let Some(a) = app {
        if let Ok(Some(v)) = settings_get(a, "net.alias") {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "QuickCopy-PC".into())
}

use crate::settings;
fn settings_get(app: &AppHandle, key: &str) -> Result<Option<serde_json::Value>, String> {
    settings::get_key(app, key).map_err(|e| e.to_string())
}
fn settings_set(app: &AppHandle, key: &str, v: serde_json::Value) -> Result<(), String> {
    settings::set_key(app, key, v).map_err(|e| e.to_string())
}

fn my_info(app: Option<&AppHandle>) -> serde_json::Value {
    serde_json::json!({
        "alias": alias(app),
        "version": VERSION,
        "deviceModel": "QuickCopy",
        "deviceType": "desktop",
        "fingerprint": fingerprint(app),
        "port": port(app),
        "protocol": "http",
        "download": true,
    })
}

/* ────────────── 组播发现 ────────────── */

pub fn spawn_discovery(app: AppHandle) {
    thread::spawn(move || {
        let my_fp = fingerprint(Some(&app));
        let socket = match socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            None,
        ) {
            Ok(s) => s,
            Err(e) => {
                log(&format!("udp socket: {e}"));
                return;
            }
        };
        if socket.set_reuse_address(true).is_err() {
            log("set_reuse_address 失败");
            return;
        }
        let bind = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT);
        if socket.bind(&socket2::SockAddr::from(bind)).is_err() {
            log(&format!("udp bind {PORT} 失败（可能被占用）"));
            return;
        }
        // 加入组播组：先 0.0.0.0，失败则枚举本机 IPv4 接口逐个尝试
        let mut joined = socket
            .join_multicast_v4(&MC_ADDR, &Ipv4Addr::UNSPECIFIED)
            .is_ok();
        if !joined {
            for ip in local_ipv4s() {
                if socket.join_multicast_v4(&MC_ADDR, &ip).is_ok() {
                    log(&format!("组播已加入（接口 {ip}）"));
                    joined = true;
                    break;
                }
            }
        }
        if !joined {
            log("join multicast 失败（所有接口）");
            return;
        } else {
            log("join multicast OK");
        }
        let s: UdpSocket = socket.into();
        let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
        // 初始三次连发（@0/100/200ms），单包易丢；离线时不广播
        if net_online(Some(&app)) {
            announce(&s, &app);
            thread::sleep(Duration::from_millis(100));
            announce(&s, &app);
            thread::sleep(Duration::from_millis(100));
            announce(&s, &app);
            log("初始 announce 已发（3 次）");
        } else {
            log("局域网服务：离线（不广播，别人检测不到）");
        }
        let mut last_announce = Instant::now();
        let mut buf = [0u8; 8192];
        loop {
            match s.recv_from(&mut buf) {
                Ok((n, src)) => {
                    let text = String::from_utf8_lossy(&buf[..n]);
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        let fp = v["fingerprint"].as_str().unwrap_or("");
                        if fp.is_empty() || fp == my_fp {
                            continue; // 丢弃自己发出的报文
                        }
                        let alias = v["alias"].as_str().unwrap_or("?");
                        if alias.contains('\u{FFFD}') {
                            continue; // 组播报文损坏：丢弃，等 HTTP register 正常覆盖
                        }
                        let port = v["port"].as_u64().unwrap_or(0);
                        if !device_known(&app, fp) {
                            log(&format!("发现新设备 {alias} ({fp}) 端口 {port}"));
                        }
                        // 收到 announce → HTTP 单播回执 register（离线时不回执，别人列表无我）
                        if net_online(Some(&app)) {
                            let _ = send_register(&app, &v, &src);
                        }
                        upsert_device(&app, &v, &src);
                    }
                }
                Err(_) => {}
            }
            if last_announce.elapsed() >= ANNOUNCE_INTERVAL {
                if net_online(Some(&app)) {
                    announce(&s, &app);
                }
                last_announce = Instant::now();
            }
            prune_devices(&app);
        }
    });
}

/// 枚举本机 IPv4 非回环地址（组播接口候选）
fn local_ipv4s() -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    if let Ok(list) = local_ip_address::local_ip() {
        if let std::net::IpAddr::V4(ip) = list {
            out.push(ip);
        }
    }
    out
}

fn announce(s: &UdpSocket, app: &AppHandle) {
    let mut p = my_info(Some(app));
    p["announce"] = serde_json::json!(true);
    let body = p.to_string();
    let dst = SocketAddrV4::new(MC_ADDR, PORT);
    let _ = s.send_to(body.as_bytes(), dst);
}

/// 单播回执：HTTP POST /api/localsend/v2/register → 对端 ip:port（端口取 announce 报文的 port 字段）
fn send_register(app: &AppHandle, v: &serde_json::Value, src: &SocketAddr) -> Result<(), String> {
    let dport = v["port"].as_u64().unwrap_or(port(Some(app)) as u64) as u16;
    let url = format!("http://{}:{}/api/localsend/v2/register", src.ip(), dport);
    let body = my_info(Some(app)).to_string();
    ureq::post(&url)
        .timeout(Duration::from_millis(2000))
        .set("Content-Type", "application/json")
        .send_string(&body)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn device_known(app: &AppHandle, fp: &str) -> bool {
    app.state::<NetState>()
        .devices
        .lock()
        .unwrap()
        .iter()
        .any(|d| d.fingerprint == fp)
}

fn upsert_device(app: &AppHandle, v: &serde_json::Value, src: &SocketAddr) {
    let st = app.state::<NetState>();
    let mut devs = st.devices.lock().unwrap();
    let d = Device {
        fingerprint: v["fingerprint"].as_str().unwrap_or("").to_string(),
        alias: v["alias"].as_str().unwrap_or("?").to_string(),
        device_type: v["deviceType"].as_str().unwrap_or("desktop").to_string(),
        version: v["version"].as_str().unwrap_or("?").to_string(),
        ip: src.ip().to_string(),
        port: v["port"].as_u64().unwrap_or(port(None) as u64) as u16,
        protocol: v["protocol"].as_str().unwrap_or("http").to_string(),
        download: v["download"].as_bool().unwrap_or(false),
        last_seen: now_ms(),
    };
    if d.fingerprint.is_empty() {
        return;
    }
    if let Some(ex) = devs.iter_mut().find(|x| x.fingerprint == d.fingerprint) {
        *ex = d;
    } else {
        devs.push(d);
    }
    drop(devs);
    emit_devices(app);
}

/// 设备离线判定超时：设置 net.deviceTtl（秒）> 默认 25s（2 轮 announce 未回应 → 离线）
fn device_ttl(app: &AppHandle) -> Duration {
    if let Ok(Some(v)) = settings_get(app, "net.deviceTtl") {
        if let Some(n) = v.as_u64() {
            if (5..=600).contains(&n) {
                return Duration::from_secs(n);
            }
        }
    }
    DEVICE_TTL
}

fn prune_devices(app: &AppHandle) {
    let st = app.state::<NetState>();
    let mut devs = st.devices.lock().unwrap();
    let cut = now_ms().saturating_sub(device_ttl(app).as_millis() as u64);
    let before = devs.len();
    devs.retain(|d| d.last_seen >= cut);
    if devs.len() != before {
        drop(devs);
        emit_devices(app);
    }
}

fn emit_devices(app: &AppHandle) {
    let st = app.state::<NetState>();
    let devs = st.devices.lock().unwrap().clone();
    let _ = app.emit_all("qp:devices", devs);
}

/* ────────────── 接收会话 ────────────── */

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RcvFile {
    pub file_id: String,
    pub file_name: String,
    pub size: u64,
    pub file_type: String,
    pub sha256: Option<String>,
    pub token: String,
    pub state: String, // Pending|InProgress|Finished|Failed
    pub path: Option<String>,
}

pub struct RcvSession {
    pub session_id: String,
    pub sender_ip: String,
    pub sender_alias: String,
    pub sender_fingerprint: String,
    pub files: Vec<RcvFile>,
    pub state: String, // Pending|Accepted|Rejected|Finished|Canceled
    pub confirm: Arc<(Mutex<Option<bool>>, Condvar)>,
    pub accepted: Vec<String>, // 用户接受的 fileId
    pub renames: HashMap<String, String>, // 重名顶层（文件/文件夹）→ 自动追加（N）后的新名
}

/// Web 分享会话（S4：链接下载）
#[derive(Clone)]
pub struct ShareFile {
    pub file_id: String,
    pub file_name: String,
    pub size: u64,
    pub path: String,
}

#[derive(Clone)]
pub struct ShareSession {
    pub session_id: String,
    pub pin: String,
    pub created_at: u64,
    pub files: Vec<ShareFile>,
}

const SHARE_TTL: u64 = 30 * 60 * 1000; // 分享会话 30 分钟

fn rcv_dir(app: &AppHandle) -> PathBuf {
    // 设置页「保存目录」优先（文件传输设置）
    if let Ok(Some(v)) = settings_get(app, "net.saveDir") {
        if let Some(s) = v.as_str() {
            let p = PathBuf::from(s);
            if !s.is_empty() && p.is_absolute() {
                return p;
            }
        }
    }
    // 默认：桌面（用户可在设置中自选保存目录）。v1 PathResolver 无桌面/下载目录 → dirs crate
    if let Some(d) = dirs::desktop_dir() {
        return d;
    }
    if let Some(d) = dirs::download_dir() {
        return d;
    }
    // 数据存放于安装路径下（不占 C 盘 AppData）
    crate::paths::data_root().join("downloads")
}

/* ────────────── HTTP 服务器 ────────────── */

pub fn spawn_server(app: AppHandle) {
    thread::spawn(move || {
        let addr = format!("0.0.0.0:{}", port(Some(&app)));
        let server = match tiny_http::Server::http(&addr) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[net] http server {addr}: {e}");
                return;
            }
        };
        eprintln!("[net] 传输服务器已启动 http://{addr}");
        for req in server.incoming_requests() {
            let app2 = app.clone();
            thread::spawn(move || handle_http(app2, req));
        }
    });
}

fn handle_http(app: AppHandle, req: tiny_http::Request) {
    let method = req.method().as_str().to_string();
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();
    match (method.as_str(), path.as_str()) {
        ("POST", "/api/localsend/v2/register") => handle_register(app, req),
        ("GET", "/api/localsend/v2/info") => respond_json(&app, req, 200, &my_info(Some(&app))),
        ("POST", "/api/localsend/v2/prepare-upload") => handle_prepare_upload(app, req),
        ("POST", "/api/localsend/v2/upload") => handle_upload(app, req, &url),
        ("POST", "/api/localsend/v2/cancel") => handle_cancel(app, req, &url),
        ("POST", "/api/localsend/v2/prepare-download") => handle_prepare_download(app, req),
        ("GET", "/api/localsend/v2/download") => handle_download(app, req, &url),
        ("GET", "/") => handle_web_home(app, req),
        _ => respond(&app, req, 404, "text/plain", b"not found".to_vec()),
    }
}

fn read_body(req: &mut tiny_http::Request) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = req.as_reader().take(256 * 1024 * 1024).read_to_end(&mut buf);
    buf
}

fn read_body_str(req: &mut tiny_http::Request) -> String {
    String::from_utf8_lossy(&read_body(req)).to_string()
}

fn respond(
    _app: &AppHandle,
    req: tiny_http::Request,
    status: u16,
    ctype: &str,
    body: Vec<u8>,
) {
    let resp = tiny_http::Response::from_data(body)
        .with_status_code(status)
        .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()).unwrap());
    let _ = req.respond(resp);
}

fn respond_json(app: &AppHandle, req: tiny_http::Request, status: u16, v: &serde_json::Value) {
    respond(app, req, status, "application/json", v.to_string().into_bytes());
}

/// register / info 共用回执
fn handle_register(app: AppHandle, mut req: tiny_http::Request) {
    // 顺带把发起 register 的设备录入设备表（信息从 body + remote ip）
    let body = read_body_str(&mut req);
    let info = my_info(Some(&app));
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
        if let Some(src) = req.remote_addr() {
            let mut p = v.clone();
            p["port"] = serde_json::json!(port(Some(&app)));
            upsert_device(&app, &p, &src);
        }
    }
    respond_json(&app, req, 200, &info);
}

/// 接收：声明上传会话 → 弹确认 → 阻塞等待用户决定
fn handle_prepare_upload(app: AppHandle, mut req: tiny_http::Request) {
    let body = read_body_str(&mut req);
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
        respond_json(&app, req, 400, &serde_json::json!({"error":"bad json"}));
        return;
    };
    let info = v["info"].clone();
    let files = v["files"].clone();
    let files_map: HashMap<String, serde_json::Value> = match serde_json::from_value(files) {
        Ok(m) => m,
        Err(_) => {
            respond_json(&app, req, 400, &serde_json::json!({"error":"bad files"}));
            return;
        }
    };
    if files_map.is_empty() {
        respond_json(&app, req, 400, &serde_json::json!({"error":"no files"}));
        return;
    }
    let sender_ip = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let sender_alias = info["alias"].as_str().unwrap_or("?").to_string();
    let sender_fp = info["fingerprint"].as_str().unwrap_or("").to_string();

    // 生成会话（支持多人同时发送：每个会话独立排队，前端逐个确认）
    let session_id = Uuid::new_v4().to_string();
    let confirm: Arc<(Mutex<Option<bool>>, Condvar)> = Arc::new((Mutex::new(None), Condvar::new()));
    let mut rfiles = Vec::new();
    let mut total = 0u64;
    for (file_id, f) in &files_map {
        let size = f["size"].as_u64().unwrap_or(0);
        total += size;
        rfiles.push(RcvFile {
            file_id: file_id.clone(),
            file_name: f["fileName"].as_str().unwrap_or("unnamed").to_string(),
            size,
            file_type: f["fileType"].as_str().unwrap_or("application/octet-stream").to_string(),
            sha256: f["sha256"].as_str().map(|s| s.to_string()),
            token: Uuid::new_v4().to_string(),
            state: "Pending".into(),
            path: None,
        });
    }
    let renames = rename_map(&app, &rfiles);
    let session = RcvSession {
        session_id: session_id.clone(),
        sender_ip: sender_ip.clone(),
        sender_alias: sender_alias.clone(),
        sender_fingerprint: sender_fp.clone(),
        files: rfiles,
        state: "Pending".into(),
        confirm: confirm.clone(),
        accepted: Vec::new(),
        renames,
    };
    {
        let st = app.state::<NetState>();
        st.sessions.lock().unwrap().insert(session_id.clone(), session);
    }

    // 静默接收判断：QP_AUTO_ACCEPT=1（自测）、设置 net.autoSave（自动保存）或
    // 发送方在「信任设备」列表（高级设置）→ 不弹确认窗，直接接收
    let auto = std::env::var("QP_AUTO_ACCEPT").unwrap_or_default();
    let auto_setting = settings_get(&app, "net.autoSave")
        .ok()
        .flatten()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let trusted = settings_get(&app, "net.trusted")
        .ok()
        .flatten()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();
    let is_trusted = trusted.contains(&sender_fp);
    let auto_accept = auto == "1" || auto_setting || is_trusted;

    // 仅非静默模式通知前端弹确认窗
    let incoming = serde_json::json!({
        "sessionId": session_id,
        "senderAlias": sender_alias.clone(),
        "senderIp": sender_ip.clone(),
        "files": files_map.iter().map(|(id, f)| serde_json::json!({
            "fileId": id,
            "fileName": f["fileName"],
            "size": f["size"],
            "fileType": f["fileType"],
        })).collect::<Vec<_>>(),
        "totalSize": total,
    });
    if !auto_accept {
        let _ = app.emit_all("qp:incoming", incoming);
    }

    log(&format!(
        "prepare-upload from {sender_ip}: auto={auto:?} setting={auto_setting} trusted={is_trusted} silent={auto_accept}"
    ));
    if auto_accept {
        {
            let st = app.state::<NetState>();
            let mut sess = st.sessions.lock().unwrap();
            if let Some(s) = sess.get_mut(&session_id) {
                let (lock, cv) = &*s.confirm;
                *lock.lock().unwrap() = Some(true);
                cv.notify_all();
            }
        }
    }

    // 阻塞等待确认（CONFIRM_TIMEOUT 内无决定 → 拒绝）
    let (lock, cv) = &*confirm;
    let mut guard = lock.lock().unwrap();
    while guard.is_none() {
        let (g, to) = cv.wait_timeout(guard, CONFIRM_TIMEOUT).unwrap();
        guard = g;
        if to.timed_out() {
            break; // 超时无决定 → 按拒绝处理，释放单槽
        }
    }
    let accept = guard.unwrap_or(false);
    drop(guard);

    let st = app.state::<NetState>();
    let mut sess = st.sessions.lock().unwrap();
    let s = match sess.get_mut(&session_id) {
        Some(s) => s,
        None => {
            respond_json(&app, req, 404, &serde_json::json!({"error":"session gone"}));
            return;
        }
    };
    if !accept {
        s.state = "Rejected".into();
        respond_json(&app, req, 403, &serde_json::json!({"message":"rejected"}));
        return;
    }
    s.state = "Accepted".into();
    let tokens: serde_json::Map<String, serde_json::Value> = s
        .files
        .iter()
        .filter(|f| s.accepted.contains(&f.file_id) || s.accepted.is_empty())
        .map(|f| (f.file_id.clone(), serde_json::json!(f.token)))
        .collect();
    respond_json(
        &app,
        req,
        200,
        &serde_json::json!({"sessionId": session_id, "files": tokens}),
    );
}

/// 接收：单文件裸字节流上传
fn handle_upload(app: AppHandle, mut req: tiny_http::Request, url: &str) {
    let q = parse_query(url);
    let Some(session_id) = q.get("sessionId") else {
        respond(&app, req, 422, "text/plain", b"missing sessionId".to_vec());
        return;
    };
    let Some(file_id) = q.get("fileId") else {
        respond(&app, req, 422, "text/plain", b"missing fileId".to_vec());
        return;
    };
    let Some(token) = q.get("token") else {
        respond(&app, req, 422, "text/plain", b"missing token".to_vec());
        return;
    };
    let ip = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    log(&format!("upload {file_id} token={token} from {ip}"));

    let st = app.state::<NetState>();
    let mut sess = st.sessions.lock().unwrap();
    let s = match sess.get_mut(session_id) {
        Some(s) if s.state == "Accepted" || s.state == "InProgress" => s,
        _ => {
            respond(&app, req, 403, "text/plain", b"session not active".to_vec());
            return;
        }
    };
    // IP 绑定 + token 校验
    if s.sender_ip != ip {
        respond(&app, req, 403, "text/plain", b"ip mismatch".to_vec());
        return;
    }
    let Some(f) = s.files.iter_mut().find(|f| f.file_id == *file_id) else {
        respond(&app, req, 422, "text/plain", b"unknown fileId".to_vec());
        return;
    };
    if f.token != *token || f.state == "Finished" {
        respond(&app, req, 403, "text/plain", b"bad token".to_vec());
        return;
    }
    // 仅接受用户勾选的文件
    if !s.accepted.is_empty() && !s.accepted.contains(file_id) {
        respond(&app, req, 403, "text/plain", b"not accepted".to_vec());
        return;
    }
    f.state = "InProgress".into();
    s.state = "InProgress".into();

    // 文本消息（LocalSend v2 fileType=text）：不生成实体文件，直接写入系统剪贴板
    if f.file_type == "text" {
        let mut content: Vec<u8> = Vec::new();
        {
            let reader = req.as_reader();
            let mut buf = [0u8; 512 * 1024];
            loop {
                let n = reader.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                content.extend_from_slice(&buf[..n]);
            }
        }
        let text = String::from_utf8_lossy(&content).to_string();
        let _g = crate::history::CLIP_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        match clipboard_rs::ClipboardContext::new() {
            Ok(c) => match c.set_text(text.clone()) {
                Ok(()) => {}
                Err(e) => log(&format!("收到文本但写入剪贴板失败: {e}")),
            },
            Err(e) => log(&format!("收到文本但无法打开剪贴板: {e}")),
        }
        drop(_g);
        f.state = "Finished".into();
        f.path = None;
        respond(&app, req, 200, "text/plain", Vec::new());
        let done = s.files.iter().all(|f| f.state == "Finished");
        if done {
            s.state = "Finished".into();
            let snap = s.files.clone();
            let alias = s.sender_alias.clone();
            drop(sess);
            let _ = app.emit_all(
                "qp:rcv_done",
                serde_json::json!({"sessionId": session_id, "senderAlias": alias, "files": snap}),
            );
        }
        return;
    }

    // 流式落盘 + sha256（显示名含相对路径时还原子目录结构）
    let dir = rcv_dir(&app);
    let _ = std::fs::create_dir_all(&dir);
    let rel = safe_rel_path(&f.file_name);
    // 会话级顶层重命名：目标已存在同名文件/文件夹时整体改为「名称（N）」（不覆盖）
    let rel_str = rel.to_string_lossy().to_string();
    let mapped = match s.renames.get(&top_level(&rel_str)) {
        Some(r) => {
            let rest = match rel_str.find(['/', '\\']) {
                Some(i) => rel_str[i..].to_string(),
                None => String::new(),
            };
            format!("{r}{rest}")
        }
        None => rel_str,
    };
    // 文件级兜底（并发会话等边界）：叶子仍冲突则自动追加（N）
    let path = unique_path(&dir, &mapped);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut file = match std::fs::File::create(&path) {
        Ok(f) => f,
        Err(_) => {
            f.state = "Failed".into();
            respond(&app, req, 500, "text/plain", b"disk error".to_vec());
            return;
        }
    };
    let mut hasher = Sha256::new();
    let mut written = 0u64;
    {
        let reader = req.as_reader();
        let mut buf = [0u8; 512 * 1024];
        loop {
            let n = reader.read(&mut buf).unwrap_or(0);
            if n == 0 {
                break;
            }
            let _ = file.write_all(&buf[..n]);
            hasher.update(&buf[..n]);
            written += n as u64;
            let _ = app.emit_all(
                "qp:rcv_progress",
                serde_json::json!({"sessionId": session_id, "fileId": file_id, "written": written, "size": f.size}),
            );
        }
    }
    let _ = file.flush();
    // 校验
    if f.size > 0 && written != f.size {
        f.state = "Failed".into();
        respond(&app, req, 422, "text/plain", format!("size mismatch {written}/{}", f.size).into_bytes());
        return;
    }
    if let Some(dig) = &f.sha256 {
        let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !hex.is_empty() && !dig.is_empty() && hex.to_lowercase() != dig.to_lowercase() {
            f.state = "Failed".into();
            respond(&app, req, 422, "text/plain", b"sha256 mismatch".to_vec());
            return;
        }
    }
    f.state = "Finished".into();
    f.path = Some(path.to_string_lossy().to_string());
    respond(&app, req, 200, "text/plain", Vec::new());
    // 全部完成？
    let done = s.files.iter().all(|f| f.state == "Finished");
    if done {
        s.state = "Finished".into();
        let snap = s.files.clone();
        let alias = s.sender_alias.clone();
        drop(sess);
        let _ = app.emit_all(
            "qp:rcv_done",
            serde_json::json!({"sessionId": session_id, "senderAlias": alias, "files": snap}),
        );
    }
}

fn handle_cancel(app: AppHandle, req: tiny_http::Request, url: &str) {
    let q = parse_query(url);
    let ip = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let st = app.state::<NetState>();
    let mut sess = st.sessions.lock().unwrap();
    let key: Option<String> = q
        .get("sessionId")
        .cloned()
        .or_else(|| sess.iter().find(|(_, s)| s.sender_ip == ip && s.state == "Pending").map(|(k, _)| k.clone()));
    if let Some(k) = key {
        if let Some(s) = sess.get_mut(&k) {
            s.state = "Canceled".into();
            let (lock, cv) = &*s.confirm;
            *lock.lock().unwrap() = Some(false);
            cv.notify_all();
        }
    }
    drop(sess);
    respond(&app, req, 200, "text/plain", Vec::new());
}

/// Web 分享会话：prepare-download（对端 LocalSend 客户端拉清单）
fn handle_prepare_download(app: AppHandle, mut req: tiny_http::Request) {
    let body = read_body_str(&mut req);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::json!({}));
    let sid = v["sessionId"].as_str().unwrap_or("").to_string();
    prune_shares(&app);
    let st = app.state::<NetState>();
    let shares = st.shares.lock().unwrap();
    let Some(s) = shares.get(&sid) else {
        respond_json(&app, req, 404, &serde_json::json!({"error":"share not found"}));
        return;
    };
    let files: serde_json::Map<String, serde_json::Value> = s
        .files
        .iter()
        .map(|f| {
            (
                f.file_id.clone(),
                serde_json::json!({"id": f.file_id, "fileName": f.file_name, "size": f.size}),
            )
        })
        .collect();
    respond_json(
        &app,
        req,
        200,
        &serde_json::json!({"info": my_info(Some(&app)), "sessionId": s.session_id, "files": files}),
    );
}

/// Web 分享会话：download（浏览器 / LocalSend 客户端拉文件流）
fn handle_download(app: AppHandle, req: tiny_http::Request, url: &str) {
    let q = parse_query(url);
    let Some(sid) = q.get("sessionId") else {
        respond(&app, req, 422, "text/plain", b"missing sessionId".to_vec());
        return;
    };
    let Some(fid) = q.get("fileId") else {
        respond(&app, req, 422, "text/plain", b"missing fileId".to_vec());
        return;
    };
    let pin = q.get("pin").cloned().unwrap_or_default();
    prune_shares(&app);
    let st = app.state::<NetState>();
    let shares = st.shares.lock().unwrap();
    let Some(s) = shares.get(sid) else {
        respond(&app, req, 404, "text/plain", b"share not found".to_vec());
        return;
    };
    if s.pin != pin {
        respond(&app, req, 403, "text/plain", b"bad pin".to_vec());
        return;
    }
    let Some(f) = s.files.iter().find(|f| f.file_id == *fid) else {
        respond(&app, req, 422, "text/plain", b"unknown fileId".to_vec());
        return;
    };
    let meta = match std::fs::metadata(&f.path) {
        Ok(m) if m.is_file() => m,
        _ => {
            respond(&app, req, 500, "text/plain", b"file missing".to_vec());
            return;
        }
    };
    let file = match std::fs::File::open(&f.path) {
        Ok(f) => f,
        Err(_) => {
            respond(&app, req, 500, "text/plain", b"open failed".to_vec());
            return;
        }
    };
    // Content-Disposition: attachment; filename*=UTF-8''<encoded>
    let enc: String = f
        .file_name
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_' { (b as char).to_string() } else { format!("%{:02X}", b) })
        .collect();
    let cd = format!("attachment; filename*=UTF-8''{enc}");
    let mut headers = Vec::new();
    headers.push(tiny_http::Header::from_bytes(&b"Content-Disposition"[..], cd.as_bytes()).unwrap_or_else(|_| tiny_http::Header::from_bytes(&b"X-D"[..], b"1").unwrap()));
    let resp = tiny_http::Response::new(tiny_http::StatusCode::from(200), headers, file, Some(meta.len() as usize), None);
    let _ = req.respond(resp);
}

/// 分享会话懒清理（TTL 过期）
fn prune_shares(app: &AppHandle) {
    let st = app.state::<NetState>();
    let mut shares = st.shares.lock().unwrap();
    let cut = now_ms().saturating_sub(SHARE_TTL);
    shares.retain(|_, s| s.created_at >= cut);
}

/// Web 下载首页：无分享参数 → 占位页；带 ?s=&pin= → 分享下载页
fn handle_web_home(app: AppHandle, req: tiny_http::Request) {
    let q = parse_query(&req.url().to_string());
    let sid = q.get("s").cloned().unwrap_or_default();
    let pin = q.get("pin").cloned().unwrap_or_default();
    if !sid.is_empty() {
        prune_shares(&app);
        let st = app.state::<NetState>();
        let shares = st.shares.lock().unwrap();
        let Some(s) = shares.get(&sid) else {
            respond(&app, req, 404, "text/plain", "分享链接已失效（30 分钟过期）".as_bytes().to_vec());
            return;
        };
        let rows: String = s
            .files
            .iter()
            .map(|f| {
                let href = format!(
                    "/api/localsend/v2/download?sessionId={}&fileId={}&pin={}",
                    sid, f.file_id, pin
                );
                let size = fmt_bytes(f.size);
                format!(
                    "<a class=\"row\" href=\"{href}\"><svg viewBox=\"0 0 24 24\" width=\"18\" height=\"18\" stroke=\"#4d6fa4\" stroke-width=\"2\" fill=\"none\"><path d=\"M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z\"/><path d=\"M14 2v6h6\"/></svg><span class=\"n\">{}</span><span class=\"sz\">{}</span><span class=\"go\">下载</span></a>",
                    f.file_name, size
                )
            })
            .collect();
        let html = format!(
            "<!doctype html><html lang=zh><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\">\
             <title>QuickCopy 分享</title>\
             <style>\
             body{{margin:0;background:#e9edf2;font-family:system-ui,-apple-system,'Segoe UI',sans-serif;color:#3c4453;display:flex;align-items:center;justify-content:center;min-height:100vh}}\
             .card{{background:#e9edf2;border-radius:16px;box-shadow:8px 8px 16px #c0c7d2,-8px -8px 16px #ffffff;padding:28px 30px;width:min(520px,92vw)}}\
             h1{{font-size:17px;margin:0 0 4px;color:#3c4453}}\
             .sub{{font-size:12.5px;color:#7f8a9b;margin-bottom:18px}}\
             .row{{display:flex;align-items:center;gap:10px;padding:11px 13px;margin:9px 0;border-radius:13px;text-decoration:none;color:#3c4453;font-size:13.5px;background:#e9edf2;box-shadow:6px 6px 12px #c0c7d2,-6px -6px 12px #ffffff}}\
             .row:active{{box-shadow:inset 5px 5px 10px #c0c7d2,inset -5px -5px 10px #ffffff}}\
             .row .n{{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:600}}\
             .row .sz{{color:#7f8a9b;font-size:12px;font-variant-numeric:tabular-nums}}\
             .row .go{{color:#4d6fa4;font-weight:600}}\
             .none{{text-align:center;color:#7f8a9b;font-size:13px;padding:20px 0}}\
             </style>\
             <div class=\"card\"><h1>QuickCopy 文件分享</h1>\
             <div class=\"sub\">由 <b>{}</b> 分享 {} 个文件 · 点击下载</div>\
             {}{}</div>",
            alias(Some(&app)),
            s.files.len(),
            if s.files.is_empty() { "<div class=\"none\">没有可下载的文件</div>" } else { &rows },
            ""
        );
        respond(&app, req, 200, "text/html; charset=utf-8", html.into_bytes());
        return;
    }
    let html = format!(
        "<!doctype html><html lang=zh><meta charset=utf-8><title>QuickCopy</title>\
         <body style=\"font-family:system-ui;display:flex;align-items:center;justify-content:center;height:90vh;background:#e9edf2\">\
         <div style=\"text-align:center\"><h1>QuickCopy</h1>\
         <p>这台设备正在运行 QuickCopy 传输服务（{}）</p>\
         <p>设备名：<b>{}</b> · 端口 {} · LocalSend v2 兼容</p></div>",
        app.package_info().version.to_string(),
        alias(Some(&app)),
        port(Some(&app))
    );
    respond(&app, req, 200, "text/html; charset=utf-8", html.into_bytes());
}

fn fmt_bytes(n: u64) -> String {
    if n >= 1048576 {
        format!("{:.1} MB", n as f64 / 1048576.0)
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(h) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(h);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(url: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Some(q) = url.split('?').nth(1) {
        for kv in q.split('&') {
            if let Some((k, v)) = kv.split_once('=') {
                m.insert(percent_decode(k), percent_decode(v));
            }
        }
    }
    m
}

/// 目标目录下顶层（文件/文件夹）存在同名时，生成 原顶层 → 新顶层 的映射：
/// 新名为 名称（1）、名称（2）… 依次递增，绝不覆盖已有文件。
fn rename_map(app: &AppHandle, files: &[RcvFile]) -> HashMap<String, String> {
    let dir = rcv_dir(app);
    let _ = std::fs::create_dir_all(&dir);
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut renames: HashMap<String, String> = HashMap::new();
    for f in files {
        let top = top_level(&f.file_name);
        if seen.contains_key(&top) {
            continue;
        }
        if dir.join(&top).exists() {
            let (stem, ext) = split_name(&top);
            let mut new_top = None;
            for i in 1..=999 {
                let cand = format!("{stem}（{i}）{ext}");
                if !dir.join(&cand).exists() {
                    new_top = Some(cand);
                    break;
                }
            }
            let nt = new_top
                .unwrap_or_else(|| format!("{stem}_{}{ext}", Uuid::new_v4().to_string().split_at(8).0));
            seen.insert(top.clone(), nt.clone());
            renames.insert(top, nt);
        } else {
            seen.insert(top.clone(), top.clone());
        }
    }
    renames
}

/// 取显示名的顶层段（目录展开时首段为文件夹名；单文件即文件名本身）
fn top_level(name: &str) -> String {
    name.split(['/', '\\']).next().unwrap_or(name).to_string()
}

/// 拆分文件名：返回 (主名, 扩展名含点)；无扩展名时扩展名为空
fn split_name(name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && !ext.contains(['/', '\\']) => {
            (stem.to_string(), format!(".{ext}"))
        }
        _ => (name.to_string(), String::new()),
    }
}

/// 相对路径目标已存在时，叶子文件名自动追加全角括号序号（不覆盖）：
/// name（1）.txt、name（2）.txt …
fn unique_path(dir: &std::path::Path, rel: &str) -> std::path::PathBuf {
    let p = dir.join(rel);
    if !p.exists() {
        return p;
    }
    let leaf = rel.split(['/', '\\']).next_back().unwrap_or(rel);
    let (stem, ext) = split_name(leaf);
    for i in 1..=999 {
        let cand = format!("{stem}（{i}）{ext}");
        if !dir.join(&cand).exists() {
            return dir.join(&cand);
        }
    }
    dir.join(format!("{stem}_{}{ext}", Uuid::new_v4().to_string().split_at(8).0))
}

/// 将显示名（可含相对路径，来自目录展开）安全转为落盘相对路径：
/// 保留子目录层级，过滤非法字符与"."/".."，防路径穿越。
fn safe_rel_path(name: &str) -> PathBuf {
    let mut clean = PathBuf::new();
    for seg in name.split(['/', '\\']) {
        let mut part = String::new();
        for c in seg.chars() {
            part.push(if matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            });
        }
        if !part.is_empty() && part != "." && part != ".." {
            clean.push(part);
        }
    }
    if clean.as_os_str().is_empty() {
        clean.push("unnamed");
    }
    clean
}

/* ────────────── 发送客户端（S3 UI 复用） ────────────── */

/// 读取并检查发送取消标志：置位 → 返回 Err("已取消")
fn check_cancel(cancel: Option<&Arc<AtomicBool>>) -> Result<(), String> {
    if cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false) {
        Err("发送已取消".into())
    } else {
        Ok(())
    }
}

/// 登记发送取消令牌（UI 取消时置 true）
fn take_cancel_marker(app: &AppHandle, send_id: &str) -> Arc<AtomicBool> {
    let marker = Arc::new(AtomicBool::new(false));
    app.state::<NetState>()
        .send_cancel
        .lock()
        .unwrap()
        .insert(send_id.to_string(), marker.clone());
    marker
}

/// 发送完成后移除令牌
fn drop_cancel_marker(app: &AppHandle, send_id: &str) {
    app.state::<NetState>()
        .send_cancel
        .lock()
        .unwrap()
        .remove(send_id);
}

/// 分块读取包装：每次 read 前检查取消标志，置位则中断上传（ureq 收到 Interrupted → transport 错误）
struct CancelReader<R> {
    inner: R,
    cancel: Option<Arc<AtomicBool>>,
}

impl<R: Read> Read for CancelReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self
            .cancel
            .as_ref()
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "send canceled by user",
            ));
        }
        self.inner.read(buf)
    }
}

/// 尽力通知对端本会话已取消（对端清理半成品并唤醒等待）
fn notify_cancel(app: &AppHandle, device: &Device, session_id: &str) {
    let url = format!(
        "http://{}:{}/api/localsend/v2/cancel?sessionId={}",
        device.ip, device.port, session_id
    );
    let _ = ureq::post(&url)
        .timeout(Duration::from_secs(5))
        .send_string("")
        .is_ok();
    let _ = app.emit_all(
        "qp:send_canceled",
        serde_json::json!({"device": device.alias, "sessionId": session_id}),
    );
}

/// 把一批路径展开为「真实路径 + 显示名」列表：文件直接进入；
/// 目录递归展开，显示名为「目录名/相对路径」（接收端据此还原目录结构）。
fn expand_send_paths(
    paths: &[String],
    names: Option<&[String]>,
) -> Result<Vec<(PathBuf, String)>, String> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        let pb = PathBuf::from(p);
        let meta = std::fs::metadata(&pb).map_err(|e| format!("读取失败 {p}: {e}"))?;
        if meta.is_file() {
            let name = names
                .and_then(|ns| ns.get(i))
                .filter(|s| !s.is_empty())
                .cloned()
                .unwrap_or_else(|| {
                    pb.file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                });
            out.push((pb, name));
        } else if meta.is_dir() {
            let dir_name = pb
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "folder".into());
            collect_dir_entries(&pb, &dir_name, &mut out)?;
        } else {
            return Err(format!("不是文件：{p}"));
        }
    }
    Ok(out)
}

/// 递归收集目录内文件；显示名 = 相对路径（目录分隔符统一用 /，接收端还原结构）
fn collect_dir_entries(dir: &Path, rel: &str, out: &mut Vec<(PathBuf, String)>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败 {dir:?}: {e}"))?;
    for en in entries {
        let en = en.map_err(|e| format!("读取目录项失败：{e}"))?;
        let name = en.file_name().to_string_lossy().to_string();
        if name.is_empty() {
            continue;
        }
        let path = en.path();
        let ft = en.file_type().map_err(|e| format!("读取目录项失败：{e}"))?;
        let rel_path = if rel.is_empty() { name } else { format!("{rel}/{name}") };
        if ft.is_dir() {
            collect_dir_entries(&path, &rel_path, out)?;
        } else if ft.is_file() {
            out.push((path, rel_path));
        }
    }
    Ok(())
}

/// 向对端设备发送一批文件（path 为本地文件或目录路径，目录会递归展开并保留目录结构）。
/// app 传 None 时跳过事件推送（供集成测试调用）。
/// names：与 paths 对应的显示名（可选；拖放临时文件路径带时间戳前缀，用显示名覆盖文件名）
/// text：可选文本，写入临时 .txt 后与文件同一会话发送（文件通道兼容性最好），发送后删除临时文件
pub fn send_files(
    app: Option<&AppHandle>,
    device: &Device,
    paths: &[String],
    names: Option<&[String]>,
    text: Option<String>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<(), String> {
    let text = text.filter(|t| !t.trim().is_empty());
    let mut items = expand_send_paths(paths, names)?;
    // 文本 → 临时 .txt 文件，随文件一起走文件通道上传（对端统一按文件接收，最稳妥）
    let mut tmp_txt: Option<std::path::PathBuf> = None;
    if let Some(t) = &text {
        let p = std::env::temp_dir().join(format!("qp_text_{}.txt", Uuid::new_v4()));
        std::fs::write(&p, t).map_err(|e| format!("写入临时文本文件失败：{e}"))?;
        items.push((p.clone(), "剪贴板.txt".to_string()));
        tmp_txt = Some(p);
    }
    if items.is_empty() {
        if let Some(p) = &tmp_txt {
            let _ = std::fs::remove_file(p);
        }
        return Err("所选内容里没有可发送的内容".into());
    }
    let result = (|| -> Result<(), String> {
        check_cancel(cancel)?;
        let mut files_map: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        let mut ids: Vec<String> = Vec::new();
        for (path, name) in &items {
            let meta = std::fs::metadata(path).map_err(|e| format!("读取文件失败 {path:?}: {e}"))?;
            let file_id = Uuid::new_v4().to_string();
            let mime = mime_of(name);
            files_map.insert(
                file_id.clone(),
                serde_json::json!({
                    "id": file_id,
                    "fileName": name,
                    "size": meta.len(),
                    "fileType": mime,
                }),
            );
            ids.push(file_id);
        }
        let base = format!("http://{}:{}/api/localsend/v2", device.ip, device.port);
        let req_body = serde_json::json!({ "info": my_info(app), "files": files_map });

        // prepare-upload
        let resp = ureq::post(&format!("{base}/prepare-upload"))
            .timeout(Duration::from_secs(60))
            .set("Content-Type", "application/json")
            .send_string(&req_body.to_string())
            .map_err(|e| match e {
                ureq::Error::Status(403, _) => "对端拒绝了发送".into(),
                ureq::Error::Status(409, _) => "对端正忙（其他会话进行中）".into(),
                ureq::Error::Status(code, _) => format!("对端返回 HTTP {code}"),
                ureq::Error::Transport(t) => format!("无法连接对端：{t}"),
            })?;
        let body = resp.into_string().unwrap_or_default();
        let v: serde_json::Value =
            serde_json::from_str(&body).map_err(|_| "对端响应格式错误".to_string())?;
        let session_id = v["sessionId"]
            .as_str()
            .ok_or("对端未返回 sessionId")?
            .to_string();
        let tokens: HashMap<String, String> = serde_json::from_value(v["files"].clone())
            .map_err(|_| "对端未返回文件令牌".to_string())?;

        // 逐文件上传（流式发送，大文件不整体读入内存；文本临时文件在 items 中一并上传）
        for (i, id) in ids.iter().enumerate() {
            check_cancel(cancel)?; // 每文件前检查取消
            let token = tokens.get(id).ok_or("对端未接受该文件")?.clone();
            let f = std::fs::File::open(&items[i].0).map_err(|e| format!("打开文件失败：{e}"))?;
            let url = format!("{base}/upload?sessionId={session_id}&fileId={id}&token={token}");
            // 分块取消：大文件上传中取消 → CancelReader 中断读取 → ureq transport 错误
            let cancel = cancel.cloned();
            // Arc 共享同一取消标志：clone 给 CancelReader，外层持有供错误分支再次检查
            let mut body = CancelReader { inner: f, cancel: cancel.clone() };
            let r = ureq::post(&url)
                .timeout(Duration::from_secs(300))
                .send(&mut body)
                .map_err(|e| {
                    if check_cancel(cancel.as_ref()).is_err() {
                        "已取消".to_string()
                    } else {
                        match e {
                            ureq::Error::Status(code, r) => {
                                format!("上传失败（HTTP {code}: {})", r.into_string().unwrap_or_default())
                            }
                            ureq::Error::Transport(t) => format!("上传失败：{t}"),
                        }
                    }
                })?;
            drop(r);
            if let Some(a) = app {
                let _ = a.emit_all(
                    "qp:send_progress",
                    serde_json::json!({"index": i + 1, "total": ids.len(), "file": &items[i].1}),
                );
            }
        }
        // 取消后向对端补发 cancel（HTTP + 本地事件），帮助对端清理半成品并唤醒等待
        if cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false) {
            if let Some(a) = app {
                notify_cancel(a, device, &session_id);
            }
            return Err("已取消".into());
        }
        if let Some(a) = app {
            let _ = a.emit_all("qp:send_done", serde_json::json!({"device": device.alias}));
        }
        Ok(())
    })();
    // 无论成败，清理临时文本文件
    if let Some(p) = &tmp_txt {
        let _ = std::fs::remove_file(p);
    }
    result
}

fn mime_of(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "txt" | "md" | "log" | "json" => "text/plain",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "exe" | "msi" => "application/octet-stream",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// 发送侧端到端自测钩子（QP_SEND_TEST="ip:port|path1;path2" → 启动后异步发送，结果写 qp-net.log）
pub fn test_send(app: AppHandle, spec: &str) {
    let spec = spec.to_string();
    std::thread::spawn(move || {
        let mut it = spec.split('|');
        let addr = it.next().unwrap_or("127.0.0.1:53318");
        let (ip, port) = addr
            .split_once(':')
            .map(|(i, p)| (i.to_string(), p.parse().unwrap_or(53318)))
            .unwrap_or(("127.0.0.1".into(), 53318));
        let paths: Vec<String> = it
            .next()
            .unwrap_or("")
            .split(';')
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let dev = Device {
            fingerprint: "MockFP-001".into(),
            alias: "MockPC".into(),
            device_type: "desktop".into(),
            version: "2.2".into(),
            ip,
            port,
            protocol: "http".into(),
            download: true,
            last_seen: 0,
        };
        std::thread::sleep(std::time::Duration::from_millis(3000)); // 等服务端就绪
        match send_files(Some(&app), &dev, &paths, None, None, None) {
            Ok(()) => log("QP_SEND_TEST 发送成功"),
            Err(e) => log(&format!("QP_SEND_TEST 发送失败: {e}")),
        }
    });
}

/* ────────────── IPC 命令 ────────────── */

/// 当前生效的接收保存目录（设置页展示：自定义 → 默认桌面）
#[tauri::command]
pub fn net_save_dir(app: AppHandle) -> String {
    rcv_dir(&app).to_string_lossy().to_string()
}

#[tauri::command]
pub fn net_devices(app: AppHandle) -> Vec<Device> {
    app.state::<NetState>().devices.lock().unwrap().clone()
}

/// 强制广播一轮（手动刷新；修改别名后让其他设备立即发现新名称）
#[tauri::command]
pub fn net_refresh(app: AppHandle) {
    if !net_online(Some(&app)) {
        log("刷新被忽略：局域网服务为离线状态");
        return;
    }
    let s = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut p = my_info(Some(&app));
    p["announce"] = serde_json::json!(true);
    let _ = s.send_to(p.to_string().as_bytes(), SocketAddrV4::new(MC_ADDR, PORT));
    log("手动刷新：已广播 announce");
}

/// 用户接受接收会话（可选 fileIds 子集）
#[tauri::command]
pub fn net_accept(app: AppHandle, session_id: String, file_ids: Option<Vec<String>>) -> Result<(), String> {
    let st = app.state::<NetState>();
    let mut sess = st.sessions.lock().unwrap();
    let s = sess
        .get_mut(&session_id)
        .ok_or_else(|| "会话不存在".to_string())?;
    if s.state != "Pending" {
        return Err("会话已处理".into());
    }
    if let Some(ids) = file_ids {
        s.accepted = ids;
    }
    s.state = "Accepted".into();
    let (lock, cv) = &*s.confirm;
    *lock.lock().unwrap() = Some(true);
    cv.notify_all();
    Ok(())
}

#[tauri::command]
pub fn net_reject(app: AppHandle, session_id: String) -> Result<(), String> {
    let st = app.state::<NetState>();
    let mut sess = st.sessions.lock().unwrap();
    let s = sess
        .get_mut(&session_id)
        .ok_or_else(|| "会话不存在".to_string())?;
    let (lock, cv) = &*s.confirm;
    *lock.lock().unwrap() = Some(false);
    cv.notify_all();
    s.state = "Rejected".into();
    Ok(())
}

/// 发送文件到指定设备（S3 界面复用）
/// names：与 paths 对应的显示名（可选；拖放临时文件时覆盖路径截取的文件名）
/// send_id：本次发送任务的取消令牌 id（可选；UI 提交后可用 net_cancel_send 中断）
#[tauri::command]
pub fn net_send(
    app: AppHandle,
    device: Device,
    paths: Vec<String>,
    names: Option<Vec<String>>,
    text: Option<String>,
    send_id: Option<String>,
) -> Result<(), String> {
    let cancel = match &send_id {
        Some(sid) => Some(take_cancel_marker(&app, sid)),
        None => None,
    };
    // 后台线程执行实际发送：对端确认（最长 60s）、大文件夹展开、大文件上传都会长时间阻塞，
    // 同步执行会占住窗口消息线程导致"未响应"。命令立即返回，进度/结果通过事件上报。
    std::thread::spawn(move || {
        let result = send_files(Some(&app), &device, &paths, names.as_deref(), text, cancel.as_ref());
        if let Some(sid) = &send_id {
            drop_cancel_marker(&app, sid);
        }
        match result {
            Ok(()) => {
                // 成功事件已由 send_files 内部 emit qp:send_done（含发送方取消场景）
            }
            Err(e) => {
                let _ = app.emit_all(
                    "qp:send_error",
                    serde_json::json!({"device": device.alias, "error": e}),
                );
            }
        }
    });
    Ok(())
}

/// 取消发送任务：置位对应 sendId 的取消标志，发送线程检测到后中断上传并通知对端
#[tauri::command]
pub fn net_cancel_send(app: AppHandle, send_id: String) -> Result<(), String> {
    let st = app.state::<NetState>();
    let map = st.send_cancel.lock().unwrap();
    match map.get(&send_id) {
        Some(c) => {
            c.store(true, Ordering::Relaxed);
            Ok(())
        }
        None => Err("该发送任务不存在或已完成".into()),
    }
}

/// 发送纯文本到指定设备：采用 LocalSend v2 文本消息（fileType=text），
/// 接收方直接写入系统剪贴板（对方粘贴即用），不生成实体文件。
#[tauri::command]
pub fn net_send_text(app: AppHandle, device: Device, text: String, send_id: Option<String>) -> Result<(), String> {
    if text.is_empty() {
        return Err("文本为空".into());
    }
    let cancel = match &send_id {
        Some(sid) => Some(take_cancel_marker(&app, sid)),
        None => None,
    };
    // 与 net_send 同理：prepare-upload 可能等待对端确认，同步执行会卡住窗口
    std::thread::spawn(move || {
        let result = send_text_files(Some(&app), &device, &text, cancel.as_ref());
        if let Some(sid) = &send_id {
            drop_cancel_marker(&app, sid);
        }
        if let Err(e) = result {
            let _ = app.emit_all(
                "qp:send_error",
                serde_json::json!({"device": device.alias, "error": e}),
            );
        }
    });
    Ok(())
}

/// 文本发送核心（app 传 None 时跳过事件推送，供集成测试调用）
fn send_text_files(
    app: Option<&AppHandle>,
    device: &Device,
    text: &str,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<(), String> {
    check_cancel(cancel)?;
    let mut files_map: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    let file_id = Uuid::new_v4().to_string();
    files_map.insert(
        file_id.clone(),
        serde_json::json!({
            "id": file_id,
            "fileName": "剪贴板.txt",
            "size": text.len(),
            "fileType": "text",
        }),
    );
    let base = format!("http://{}:{}/api/localsend/v2", device.ip, device.port);
    let req_body = serde_json::json!({ "info": my_info(app), "files": files_map });

    // prepare-upload
    let resp = ureq::post(&format!("{base}/prepare-upload"))
        .timeout(Duration::from_secs(60))
        .set("Content-Type", "application/json")
        .send_string(&req_body.to_string())
        .map_err(|e| match e {
            ureq::Error::Status(403, _) => "对端拒绝了发送".into(),
            ureq::Error::Status(409, _) => "对端正忙（其他会话进行中）".into(),
            ureq::Error::Status(code, _) => format!("对端返回 HTTP {code}"),
            ureq::Error::Transport(t) => format!("无法连接对端：{t}"),
        })?;
    let body = resp.into_string().unwrap_or_default();
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| "对端响应格式错误".to_string())?;
    let session_id = v["sessionId"]
        .as_str()
        .ok_or("对端未返回 sessionId")?
        .to_string();
    let tokens: HashMap<String, String> = serde_json::from_value(v["files"].clone())
        .map_err(|_| "对端未返回文本令牌".to_string())?;
    let token = tokens.get(&file_id).ok_or("对端未接受该文本")?.clone();

    // upload：body 为纯文本（不落盘）
    let url = format!("{base}/upload?sessionId={session_id}&fileId={file_id}&token={token}");
    let r = ureq::post(&url)
        .timeout(Duration::from_secs(60))
        .send_string(text)
        .map_err(|e| match e {
            ureq::Error::Status(code, r) => {
                format!("发送失败（HTTP {code}: {})", r.into_string().unwrap_or_default())
            }
            ureq::Error::Transport(t) => format!("发送失败：{t}"),
        })?;
    drop(r);
    // 文本上传完才发现被取消：补发 cancel 让对端清理（文本单包，上传中无法中断）
    if check_cancel(cancel).is_err() {
        if let Some(a) = app {
            notify_cancel(a, device, &session_id);
        }
        return Err("已取消".into());
    }
    if let Some(a) = app {
        let _ = a.emit_all("qp:send_done", serde_json::json!({"device": device.alias}));
    }
    Ok(())
}

/// 创建 Web 分享会话（链接下载），返回分享 URL
#[tauri::command]
pub fn net_share(app: AppHandle, paths: Vec<String>) -> Result<String, String> {
    if paths.is_empty() {
        return Err("没有要分享的文件".into());
    }
    let sid = Uuid::new_v4().to_string();
    let pin = format!("{:04}", now_ms() % 10000);
    let mut files = Vec::new();
    for (path, name) in expand_send_paths(&paths, None)? {
        let meta = std::fs::metadata(&path).map_err(|e| format!("读取文件失败 {path:?}: {e}"))?;
        files.push(ShareFile {
            file_id: Uuid::new_v4().to_string(),
            file_name: name,
            size: meta.len(),
            path: path.to_string_lossy().to_string(),
        });
    }
    let sess = ShareSession {
        session_id: sid.clone(),
        pin: pin.clone(),
        created_at: now_ms(),
        files,
    };
    {
        let st = app.state::<NetState>();
        st.shares.lock().unwrap().insert(sid.clone(), sess);
    }
    prune_shares(&app);
    let lan = local_ip_address::local_ip()
        .ok()
        .map(|i| i.to_string())
        .unwrap_or_else(|| "127.0.0.1".into());
    Ok(format!("http://{lan}:{}/?s={sid}&pin={pin}", port(Some(&app))))
}

/// 别名设置
#[tauri::command]
pub fn net_set_alias(app: AppHandle, alias: String) -> Result<(), String> {
    if alias.trim().is_empty() {
        return Err("设备名不能为空".into());
    }
    settings_set(&app, "net.alias", serde_json::json!(alias.trim()))
        .map_err(|e| e.to_string())?;
    // 广播新身份
    let s = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };
    let mut p = my_info(Some(&app));
    p["announce"] = serde_json::json!(true);
    let _ = s.send_to(p.to_string().as_bytes(), SocketAddrV4::new(MC_ADDR, PORT));
    Ok(())
}

#[tauri::command]
pub fn net_my_info(app: AppHandle) -> serde_json::Value {
    my_info(Some(&app))
}
