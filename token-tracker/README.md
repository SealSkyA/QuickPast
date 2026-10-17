# Token 用量记录工具

为「公司 token 使用量比赛」而做的本地工具：自动从 OhMyAgent 会话数据中采集每次模型请求的
token 使用量，生成统计 JSON / CSV，并渲染一个**完全离线、无需外部依赖**的网页仪表盘。

## 两种使用模式

### A. 实时模式（推荐，Python ≥3.8，仅标准库）

1. 双击 **`启动仪表盘.bat`**（或在终端运行 `python serve.py`）。
2. 本地服务启动并自动打开浏览器（`http://127.0.0.1:8756/`）。
3. **每次刷新页面都会自动重新采集最新数据**，时间与用量实时更新。
4. 关闭终端窗口（或 Ctrl+C）即停止服务。
   - 换端口：`python serve.py 9000`；数据目录可用环境变量 `OHMYAGENT_DATA_DIR` 覆盖。

#### 开机自启（已配置）

已在本机「启动文件夹」创建快捷方式 `Token用量仪表盘.lnk`，**开机登录后自动静默启动**
服务（`pythonw serve.py --no-browser`，无窗口、不自动弹浏览器）。服务启动后访问
`http://127.0.0.1:8756/` 即可。

- **手动停止**：双击 `停止服务.bat`（服务为开机自启，电脑重启后会自动再起来）。
- **取消开机自启**：删除启动文件夹中的 `Token用量仪表盘.lnk`。路径为
  按 Win+R 输入 `shell:startup` 回车可见。
- **立即验证**：服务已在运行，刷新的仪表盘数据即为最新。

### B. 离线模式（PowerShell 5.1 原生，无需 Python）

1. 手动运行：`powershell -NoProfile -ExecutionPolicy Bypass -File collect.ps1`
2. 生成自包含 `dashboard.html`（数据内嵌、双击即开），并刷新 `data/` 下的 JSON / CSV。
3. 注意：离线模式是**生成时快照**，浏览器 F5 不会重新采集，需先重跑脚本。

## 文件结构

```
token-tracker/
├── serve.py                  # 实时模式：采集 + 本地服务（Python 标准库，开机自启用 --no-browser 静默模式）
├── collect.ps1               # 离线模式：采集 + 生成静态仪表盘（PowerShell 5.1 原生）
├── dashboard.template.html   # 仪表盘模板（含占位符，勿手动编辑）
├── 启动仪表盘.bat            # 一键启动实时模式（带窗口）
├── 停止服务.bat              # 手动停止后台服务
├── README.md
└── data/                     # 每次采集自动生成
    ├── usage_stats.json      # 统计结果（机器可读）
    └── usage_detail.csv      # 请求明细（UTF-8 BOM，Excel 直接打开不乱码）
```

### 可选参数

| 参数 | 说明 |
| --- | --- |
| `-DataDir <路径>` | 指定 OhMyAgent 数据目录。默认 `%APPDATA%\com.chaitin.baizhi.monkeycode\ohmyagent`，也支持环境变量 `OHMYAGENT_DATA_DIR` |
| `-OutDir <路径>` | 指定输出目录，默认脚本目录下的 `data\` |

## 数据来源与统计口径

| 指标 | 口径 |
| --- | --- |
| 请求次数 | `messages.jsonl` 中 `context_tokens` 记录条数（每次真实模型请求一条；限流重试不重复计数） |
| 输入 token（实际） | 平台记录的 `baseline_actual_tokens`，即发送给模型的完整上下文 token 数（含历史重复，与 OpenAI prompt tokens 计费口径一致） |
| 输入 token（估算） | 平台记录的 `baseline_estimated_tokens` |
| 输出 token（估算） | 由该请求产生的全部输出文本（思考 thinking + 正文 text + 工具入参 tool_use）按长度估算：中文/全角 ≈ 1.5 字符/token，其余 ≈ 4 字符/token |
| 合计 token（估算） | 输入（实际）+ 输出（估算） |

> 注意：平台未存储响应中的 completion tokens，因此「输出 token」为估算值；
> 标题生成等不落 `context_tokens` 记录的轻量请求未计入。明细 CSV 中同时给出
> 「输出字符数」，便于自行复核换算。

## 常见问题

- **双击 bat 提示找不到会话目录**：OhMyAgent 数据目录不在默认位置时，编辑
  `启动仪表盘.bat`，在 `collect.ps1` 后追加 `-DataDir "你的实际路径"`。
- **Excel 打开 CSV 乱码**：文件为 UTF-8 带 BOM，Excel 应正常识别；若仍乱码，
  用记事本打开另存为「带 BOM 的 UTF-8」。
- **想加手动记录**：明细 CSV 为只读导出，手动补充可自行编辑 CSV 或直接改
  `data/usage_stats.json` 后刷新仪表盘（仪表盘为一次性生成，需重新运行脚本才会重嵌数据）。
