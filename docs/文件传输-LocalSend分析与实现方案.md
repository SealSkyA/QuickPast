# 文件传输 —— LocalSend 全面分析与 QuickPaste 实现方案

> ⚠️ **2026-10-16 实测订正**（依据 `localsend/main` 源码，已钉死）：下文为初版分析，
> 其中多处猜测已被《01-协议实现规格》推翻，**编码一律以 01 为准**：
> 1. 组播组是 **224.0.0.167**（不是 224.0.0.251）；UDP 载荷是 **JSON**（不是"原始 HTTP 文本"）
> 2. 端点基路径是 **`/api/localsend/v2/`**（老的 `/api/v1/*` 已被取代）；上传是**每文件一次裸字节 POST**（不是 multipart）
> 3. 旧的 `docs/protocol.md` 不存在；协议文档 = `packages/core` Rust 源码与 DTO 定义
> 4. 浏览器接收 = **本机 HTTP 直出下载页**（`GET /` + `GET /api/localsend/v2/download`），官方也不经云中继
> 5. 加密 = 自签证书 + **mTLS + 证书指纹固定**；v3 增 nonce/令牌签名（仅 WebRTC 场景）
> 6. 会话为**单槽**（并发 409），IP 绑定，每文件独立 token，sha256 可验
>
> 参考项目：https://github.com/localsend/localsend （Apache-2.0，可自由借鉴/引用）
> 环境说明：本分析曾基于离线记忆写成；现源码已拉取至 `E:\2Monkcode\localsend-main` 可随时对照。

---

## 一、LocalSend 是什么 / 为什么参考它

- 免费开源的**跨平台局域网文件互传工具**（Flutter 编写，Windows/macOS/Linux/Android/iOS）。
- 核心理念与 QuickPaste 完全同频：**无账号、无云服务器、纯局域网点对点、离线可用**。
- 设计要点：**每个设备自己跑一个 HTTP 服务器 + UDP 组播发现**，两台设备互通即完成传输。
- “发送文本/剪贴板”在 LocalSend 里就是**生成一个虚拟文本文件**来传——与 QuickPaste 的「发送文本 / 粘贴板」需求天然对应。

**结论**：协议、发现、流控、确认交互都值得全面复刻；复刻协议还能带来一个 bonus——**局域网内 QuickPaste 与装了 LocalSend 的其它设备可直接互传**。

---

## 二、总体架构（LocalSend 技术拆解）

```
┌─ 本机 ──────────────────────────────────────────────┐
│  Flutter UI（列表/确认弹窗/设置）                      │
│        │                                             │
│  Dart 后端核心：                                      │
│   ├─ HTTPServer  TCP 0.0.0.0:53317（所有网卡）        │
│   │    提供 /api/v1/* 全套 HTTP 接口                  │
│   ├─ UDP 组播发现（IPv4 224.0.0.251:53317 等）       │
│   ├─ 设备表（指纹去重 + alias/类型/版本）              │
│   ├─ 会话管理（sendId/token/进度/过期）               │
│   └─ 收件箱（确认弹窗 / 自动保存 / 信任列表）          │
└─────────────────────────────────────────────────────┘
         ▲ 互发 HTTP（同 IP 段，走 TCP 53317）
         │ 互发 UDP 组播 hello（收到即回）
┌─ 对端 LocalSend / 兼容设备 ──────────────────────────┐
│   同样的 HTTPServer + 发现 + 会话                      │
└─────────────────────────────────────────────────────┘
```

- **单一端口** 53317 同时承载 HTTP(TCP) 与发现组播(UDP) —— 只开一个端口，防火墙策略简单。
- 地址只在本机 socket 上（`0.0.0.0`），**不依赖任何第三方服务**。

---

## 三、设备发现机制

1. 启动时 UDP 组播一条 `POST /api/v1/hello`（原始 HTTP 文本打进 UDP 报文）到组播组，
   头字段含：`Device-Name`(别名) / `Device-Model` / `Device-Type` / `Fingerprint`(持久化随机指纹) / `Version` / `Port`。
2. 收到者解析包的**源 IP** + Port，**单播回一条 HTTP `POST /api/v1/hello`** 给发起者，携带自己的指纹等头。
3. 发起者再对回包者发 `GET /api/v1/devices`，拿到设备详情 JSON（alias、deviceType、fingerprint、port、protocol、download 等）后入列表。
4. 设备表用 **Fingerprint** 做唯一键去重/刷新；手动刷新 = 再发一轮组播。
5. 进入不同网段/离开，靠周期性广播与超时（一定时间没回应就标离线/移除）。

要点：**不需要 mDNS/SSDP**，极简的“HTTP over UDP 组播”即完成发现；这也让复刻成本很低。
[待核对]：IPv6 组播地址、hello 头的确切拼写、广播频率与离线超时阈值。

---

## 四、HTTP API 一览（核心为发送/接收）

| 端点 | 方法 | 作用 | 备注 |
| --- | --- | --- | --- |
| `/api/v1/hello` | POST | 发现握手（UDP 组播 + TCP 单播回执都可） | 头字段指纹/别名/版本 |
| `/api/v1/devices` | GET | 对端返回设备详情 JSON | 发现流程收尾 |
| `/api/v1/send` | POST | 发起发送，body 为 multipart，含 `session`(JSON) + 各文件流 | 对端确认后返回 200+sessionId；拒绝 409 |
| `/api/v1/prepareUpload` | POST | 上传前声明会话（alias/sendId/token/fileCount/totalSize） | [新版本] 先行握手 |
| `/api/v1/upload` | POST | 单文件上传（multipart：info JSON + 文件体） | 每个文件一次 |
| `/api/v1/cancel` | POST | 取消发送会话 | 对端回收 |
| `/api/v1/download…` | GET | [新版本] 收端以会话提供下载 | [待核对] 确切路径 |

发送封装（multipart 字段约定，**互通的关键**）：
- `session` 部分 = JSON：`{"info":{"alias","sendId","token","fileCount","totalSize"},"fileId":<id>,"info":{"fileName","size","fileType","preview"?}}`
- 文件体部分 = `fileId|<序号>`：原始字节流（支持大文件，流式解析不落整内存）
- 状态码约定：200=接受并返回 sessionId/expiresAt；409=拒绝/忙碌。
[待核对]：`session` JSON 键名与 `prepareUpload`/`upload` 在各版本的准确字段（1.16+ 变更过多）。

---

## 五、发送 / 接收流程

**发送（本机→对端）**
1. 用户选文件/文本 → 组播/列表里有对端 → 触发 `POST /api/v1/send`。
2. 对端弹确认框（或匹配“自动接收/信任列表”直接 200）。
3. 接受后按文件逐个 `upload`；发送端展示进度；中途可 `cancel`。
4. 全部完成，通知两端。

**接收（对端→本机）**
1. 收到 `send` → 校验指纹/大小 → 弹窗：接受 / 拒绝。
2. 接受 → 创建会话 → 收 `upload` 流式落盘到保存目录（默认 下载/或用户指定）。
3. 校验总大小/文件数符合声明，完成后通知。

**状态与过期**：会话带 `expiresAt`，超时自动回收；拒绝/取消即时释放。

**安全模型**
- **无认证**：默认信任局域网内人人可发（靠用户确认框把关）。
- **指纹**：持久化随机串，作身份与（可选）信任列表凭据。
- **可选 HTTPS 自签**（新版本）：防局域网嗅探；成本是证书分发/忽略告警。
- **大小限制**：可设单文件/总大小上限，防止撑爆磁盘。
[待核对]：信任列表判定字段、超时阈值、上限默认值。

---

## 六、对 QuickPaste 的借鉴与裁剪

| LocalSend 能力 | QuickPaste 怎么做 | 说明 |
| --- | --- | --- |
| UDP 组播发现 | 完全复刻（224.0.0.251:53317） | 互通的基础 |
| 单端口 HTTP 服务器 | 完全复刻（0.0.0.0:53317） | 防火墙 / 一条规则 |
| multipart 上传协议 | 完全复刻字段格式 | **可与 LocalSend 互传** |
| 文件/文件夹/文本/剪贴板发送 | 文本/剪贴板 = 虚拟文本文件 | 与需求 4 类型对应 |
| 接收确认弹窗 | 保留确认弹窗 + 自动保存开关 | 与「自动保存」需求对应 |
| 信任列表 | 收藏设备 = 优先段；自动接收名单 | 与「收藏」需求对应 |
| 链接分享（浏览器接收） | **本机 HTTP 直接推 Web 下载页**（不走云端中继） | LocalSend 用云端 WebSocket 中继，我们纯本地更离线 |
| HTTPS 自签 | 暂列二期（可选） | 内网默认 HTTP 够用 |
| Flutter UI | 不复刻，用已确认的 Neumorphism Web UI | 前端已就绪 |

**互通目标**：QuickPaste 与同一局域网内的 LocalSend 设备可直接互发/互收（协议一致）。实现阶段先按文档实现，再在有网环境用真机对拍验证。

---

## 七、技术栈选型（QuickPaste 是 Windows 桌面端）

前提：Windows 10/11 x64 + x86(优先 x64)、纯离线、要托盘/全局热键/剪贴板监听、UI 已是 HTML+CSS+JS。

| 方案 | 体积/内存 | 热键/托盘/剪贴板 | HTTP+UDP 服务 | x86 支持 | 结论 |
| --- | --- | --- | --- | --- | --- |
| **Tauri 2（Rust + WebView2）** | 安装包 ~8MB | 三个官方/成熟 crate 全有 | Rust 直接写（tokio+hyper+multipart） | 支持 i686 目标 | ✅ 推荐：轻、稳、复用现成 HTML UI |
| Electron（Node） | ~150MB，RAM 高 | 内置 | Node 写 HTTP/UDP 容易 | 支持 | 可用但笨重，内网办公机不友好 |
| C# (.NET8 + WebView2) | ~80MB | Win32 生态成熟 | Kestrel | 双目标轻松 | 可行，但绑定 Windows 且不够轻 |

**推荐：Tauri 2（Rust 后端 + 系统 WebView2）。** 理由：体积小、内存低（企业旧办公机友好）、WebView2 是 Win10/11 系统自带无需分发、Rust 写 HTTP 服务器/组播/剪贴板监听最稳、前端零改动复用已确认的 Neumorphism 页面。

---

## 八、模块划分（Rust 后端 / 前端接入点）

```
backend/ (Rust crate)
├─ clipboard/     剪贴板监听（轮询+差异），文字/位图/文件列表(CF_HDROP)
├─ store/         SQLite 历史库（文字/图片/文件/便签 + 上限清理）
├─ hotkey/        全局热键 Ctrl+Alt+Z 呼出（区分置顶/呼出窗口）
├─ tray/          托盘：显示历史/设置/清空历史/打开缓存目录/关于
├─ net/
│  ├─ discover     UDP 组播 hello + 单播回执 + 设备表
│  ├─ server       TCP 53317：/api/v1/* LocalSend 兼容接口
│  ├─ session      sendId/token/进度/过期
│  └─ sharelink    本机 Web 下载页（GET /s/<id> → 浏览器下载）
└─ frontend/       现有 ui/index.html（经 Tauri 内嵌，IPC 桥剪贴板/设置/设备）
```

---

## 九、分阶段实施计划

| 阶段 | 内容 | 验收 |
| --- | --- | --- |
| **S0 骨架** | Tauri2 项目 + 内嵌 Neumorphism UI；托盘/置顶/最小化/关闭进托盘；Ctrl+Alt+Z 呼出；设置持久化 | 桌面体验三件套可用，UI 零改动载入 |
| **S1 剪贴板** | 监听写入历史（文字/图片/文件）；SQLite；上限/搜索/收藏/置顶/删除；单击复制/粘贴；批量选择 | 原型页功能在真实数据上全部可用 |
| **S2 传输核心** | UDP 发现 + HTTP 53317 + LocalSend 兼容 send/upload/cancel；接收确认弹窗与自动保存 | 两台 QuickPaste 互传成功；设备列表刷新 |
| **S3 发送 UI** | 类型选择(文件/夹/文本/粘贴板)、设备搜索、单选即发、多选+底栏、发送模式弹窗；别名/服务器状态 | 与 S2 接口打通，页面无假数据 |
| **S4 链接分享** | 本机 Web 下载页 + 分享临时 URL；安全限制（过期/大小） | 浏览器直接下载成功 |
| **S5 兼容对拍** | 有网环境装 LocalSend 真机对拍：QuickPaste ⇄ LocalSend 双向 | 互通成功（否则按差异修正字段） |
| **S6 打包发布** | x64/x86 安装包（NSIS），离线安装，图标/开机自启 | 干净机安装可跑 |

**建议并行前置**：S5 的协议对拍最好尽早——先在开发机装 LocalSend 用抓包把 multipart 字段/头确认死，再写 S2，避免返工。

---

## 十、风险与待核对清单（离线无法完成项）

1. **[待核对]** `/api/v1/send` 的 multipart `session` JSON 键名、`fileId|idx` 文件体字段（最新版是否拆分 prepareUpload/upload）。
2. **[待核对]** 发现握手的确切头字段拼写（大小写）、UDP 载荷格式（是否“原始 HTTP 文本”）、IPv6 组播地址。
3. **[待核对]** HTTPS 自签/信任列表的默认配置与开关位置（决定 S2 是否含 TLS）。
4. **[待核对]** 空闲/离线判定的广播周期与超时阈值（影响设备列表 UX）。
5. 知识外的不确定性：以 [待核对] 标注，**实施 S2/S5 时必须在有网环境拉取 `localsend/server` 源码与 `docs/protocol.md` 校准**。

---

*文档路径：本项目 docs/，供 S0–S6 各阶段引用。*