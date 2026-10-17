# UI 风格规范 —— 新拟物 Neumorphism

> 来源：参考页面 `.monkeycode/uploads/index.html`（Soc智能限频）。
> 适用：后续所有新项目的 Web 页面，优先采用此风格。
> 参考实现已存于 `token-tracker/dashboard.html`（同风格零依赖仪表盘）。

## 0. 一句话原则

**单一浅灰底，卡片/输入/按钮与背景同色，形状全靠阴影表达；光源在左上，凸起=左上亮+右下暗，凹陷=inset 反相；分隔用刻痕，不画硬边框；反馈=凸变凹。**

## 1. 设计令牌（CSS 变量）

```css
:root{
  /* 背景与表面同色：塑形全靠阴影，不靠色差 */
  --bg:#e9edf2; --card:#e9edf2; --card2:#e9edf2;
  /* 两层阴影：左上受光、右下投影 */
  --lt:#ffffff; --sh:#c0c7d2;
  --line:#cfd6e0;              /* 刻痕的暗侧，不是描边色 */
  --fg:#3c4453; --mut:#7f8a9b;
  --acc:#4d6fa4; --ok:#2f8f6d; --warn:#a5762c; --err:#b3524d;
  --r:16px;
  /* 凸起 / 凹陷 两个基元，所有组件复用 */
  --up:-5px -5px 11px var(--lt), 5px 5px 12px var(--sh);
  --up-sm:-3px -3px 7px var(--lt), 3px 3px 8px var(--sh);
  --down:-2px -2px 5px var(--lt), 2px 2px 6px var(--sh);
  --in:inset -4px -4px 8px var(--lt), inset 4px 4px 9px var(--sh);
  --in-sm:inset -2px -2px 5px var(--lt), inset 2px 2px 6px var(--sh);
}
```

- 功能色必须**压暗**：浅底上亮饱和色对比度不足，数值行会看不清。
- 不要新增描边色；需要分隔就做刻痕（见下）。

## 2. 基础规范

| 项 | 值 |
| --- | --- |
| 字体 | `15px/1.55 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif` + `-webkit-font-smoothing:antialiased` |
| 标题 | `21px; font-weight:650; letter-spacing:.3px` |
| 次要文字 | `11.5–12.5px; color:var(--mut)` |
| 数字 | `font-variant-numeric:tabular-nums`；长值 `word-break:break-all` |
| 代码/日志 | `ui-monospace,Menlo,Consolas,monospace`；11.5px 常见 |
| 布局 | 移动优先 `max-width:680px;margin:0 auto;padding:16px`；底部 fixed TabBar + `env(safe-area-inset-*)` |
| 圆角 | 卡片 `--r:16px`；组件 12–13px；胶囊/开关 `999px` |
| 图标 | 行内 SVG stroke 风格：`fill:none;stroke-width:2;stroke-linecap/join:round`，头部 2px、Tab 1.9px，`stroke:var(--acc)` |

## 3. 组件配方

### 3.1 凸起卡片
```css
.card{background:var(--card);border-radius:var(--r);box-shadow:var(--up);padding:16px;margin-bottom:18px}
```
卡片标题：`h2` + 16px 行内 SVG 图标（--acc）+ 下方 `.hint`（12px 灰字说明用途）。

### 3.2 刻痕分隔行
```css
.row{display:flex;align-items:center;gap:12px;padding:12px 0;
     border-top:1px solid var(--lt);box-shadow:0 1px 0 rgba(192,199,210,.75)}
.row:first-of-type{border-top:0;box-shadow:none}
```

### 3.3 输入/下拉/滑杆（一律凹进背景）
```css
select,input[type=text]{background:var(--card2);border:0;border-radius:12px;
  padding:10px 12px;box-shadow:var(--in-sm)}
select:focus,input:focus{outline:none;box-shadow:var(--in)}   /* 焦点=加深内阴影 */
```
滑杆：槽 `height:6px;border-radius:999px;box-shadow:var(--in-sm)`；钮 16px 圆 + 凸起内外双层阴影；激活时钮变 `--acc`。

### 3.4 按钮（核心反馈：按下去凸变凹）
```css
button{background:var(--card2);border:0;border-radius:13px;font-weight:600;
  padding:10px 16px;box-shadow:var(--up-sm);transition:box-shadow .16s,transform .16s}
button:active{box-shadow:var(--in-sm);transform:translateY(1px)}
button.pri{background:var(--acc);color:#f2f5f9}   /* 主按钮 */
button.dan{color:var(--err)}                       /* 危险(文字红) */
button:disabled{opacity:.45;box-shadow:none}
```

### 3.5 开关（槽凹钮凸，开启染色雾）
```css
.sw i{background:var(--card);border-radius:999px;box-shadow:var(--in-sm)}
.sw i:before{width:18px;height:18px;background:var(--card);border-radius:50%;box-shadow:var(--up-sm)}
.sw input:checked+i{box-shadow:var(--in-sm),inset 0 0 0 2px rgba(77,111,164,.22)}
.sw input:checked+i:before{transform:translateX(20px);background:var(--acc)}
```

### 3.6 状态 / KV 格
- 状态行：极淡色雾 `linear-gradient(90deg, rgba(色,.14), rgba(色,0) 62%)` + 左侧 4px `currentColor` 竖带 + 文字着状态色；用 `on/game/off` 分档。
- KV 格：凹陷 `box-shadow:var(--in-sm)` 的两列网格（`.hero-meta` 内 `.kv`），key 10.5px 灰、value 15px/700。
- 状态图标：爱心 SVG + `@keyframes beat` 心跳；故障/暂停时动画关闭。

### 3.7 日志 / 名单列表
- 日志框：`box-shadow:var(--in)` 凹陷 + 等宽字体，空时 `::after{content:"暂无日志"}`。
- 列表行：刻痕分隔；应用图标 38px 凸起方块；取不到图标时用「包名哈希算色相」的渐变字母头像（`hsl(var(--h) 34% 74%)`）。

### 3.8 TabBar / Toast / 警示条
- TabBar：fixed 底部，同底色，选中格 `box-shadow:var(--in-sm)` 按进去，图标+文字竖排。
- Toast：fixed 底部居中，`box-shadow:var(--up)`，`.show` 从下方上浮淡入。
- 警示条（`.nobar`）：凹陷 `box-shadow:var(--in)` + 文字色表示等级，不用边框。

## 4. 禁止事项（Do NOT）

- ❌ 不画 1px 硬边框做卡片/分组（用阴影和刻痕）。
- ❌ 不用亮饱和色做功能色（浅底上必须压暗一档）。
- ❌ 不给输入框画边框 + outline 焦点（用内阴影深浅变化）。
- ❌ hover 依赖（触屏优先，反馈走 `:active`）。
- ❌ 在浅底上用纯黑/纯白文字（用 `--fg #3c4453` / `--mut #7f8a9b`）。

## 5. 落地参考

- 模板: `token-tracker/dashboard.template.html`（同一套变量的可运行实例）
- 原始参考: `.monkeycode/uploads/index.html`