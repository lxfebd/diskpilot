# DiskPilot · 视觉规则（DESIGN）

> 项目的视觉语言与规则：主题系统、语义 token、组件样式约定、布局骨架。
> 写给前端开发者与视觉 AI。代码事实源：`apps/desktop/src/theme.ts` + `apps/desktop/src/styles/tokens.css`。

## 一、总原则

1. **主题即 token**：所有视觉决策收敛到 CSS 变量（自定义属性），组件**不硬编码颜色**，一律消费语义 token。换主题 = 换 token，不碰组件。
2. **语义优先于品牌**：新代码用语义层（`--bg` / `--surface` / `--text` / `--accent` / `--risk-*`），不要用原始色（`--pink` / `--ink` / `--paper` 等 legacy 名保留给旧组件过渡）。
3. **工具密度**：主题里的大型圆角/粗阴影被收敛到适合密集工具行的值（见各预设注释「softened / clamped for tool density」）。
4. **深色不是补丁**：`dark.css` 是历史深色补丁层（54 条覆盖规则），新主题应直接定义深色调，不要依赖它。

## 二、主题系统（theme.ts，17 套预设）

### 2.1 预设清单

| id | 风格 | 明暗 |
|---|---|---|
| `vercel` | Vercel 风格技术仪表盘（单色 + 1px 细边，主色 #121212） | 浅（默认） |
| `minimal` | Minimalist（中性单色，酷中性品牌色阶） | 浅 |
| `doubao` | Doubao（字节风，可读文档风，主色 #0065fd） | 浅 |
| `claude` | Claude（暖米色 + editorial serif，主色 #c96442） | 浅 |
| `google` | Google（分析洁净，蓝链接 #1a73e8） | 浅 |
| `volcengine` | 火山引擎（元理蓝 #1664ff） | 浅 |
| `nerv` | Nerv 暗色霓虹 editorial（信号红 #ea343a，高对比） | 暗 |
| `trae` | TRAE Work 工程工作室（紫 #4b3fe3） | 浅 |
| `motionfit` | Motion Fit（暗色能量，信号橙 #ff4000） | 暗 |
| `barbie` | Barbie 泡泡糖（粉 #e11d48，大圆角 1.8rem） | 浅 |
| `brand` | Apple Copy（克制优雅，iOS 蓝 #007aff，圆角 2rem） | 浅 |
| `golden` | Golden Time（暖 editorial，焦糖金 #b98a4f） | 浅 |
| `vibecamp` | Vibe Camp（赤陶土 #f1481e，复古） | 浅 |
| `21th` | 21st（macOS 石墨极简，方角 radius 0） | 浅 |
| `steam` | Steam-ish（暗色矮人科技绿 #66c0f4） | 暗 |
| `dark` | Nerv dark 兼容别名 | 暗 |
| `pro` | Volcengine dark 兼容别名 | 暗 |

### 2.2 主题结构

- **持久化**：localStorage `diskpilot.theme`（`ThemeSettings { id, accent, fontScale }`）。`initTheme()` 在 main.tsx 渲染前调用，防主题闪烁。
- **应用方式**：`applyTheme()` 把预设所有变量写到 `<html>` 内联样式（内联优先级 > `:root` 默认），切 `data-theme` 属性 + `colorScheme`。
- **动态 accent**：用户自定义主色（#rrggbb）→ `applyAccent()` 按感知亮度派生 `--pink-deep`（暗色提亮 / 浅色压暗）+ `--pink-soft` + `--pink-bg`，同步语义 `--accent` 系；若主色够亮（brightness>0.25）连 `--ok` / `--risk-safe` 一起跟随。
- **字体**：每套主题配 ui/editor 字体族（`THEME_FONTS`），写入 `--font-sans` / `--font-mono`；`--font-scale` 支持 0.8–1.3 倍缩放（乘在 13px 基准上）。
- **事件**：切换广播 `diskpilot:theme-changed`，Settings 弹窗可响应。

### 2.3 新增主题的检查清单

1. 在 `PRESETS` 加完整 token 组（原始色 + 语义层 + 边框 + 阴影 + 圆角 + 状态色）。
2. 决定明暗：暗色主题加入 `DARK_IDS`（否则 accent 派生会按浅色算）。
3. `THEME_FONTS` 配字体族；`ThemeId` 联合类型加 id。
4. **不要依赖 `dark.css` 补丁**：预设本身就要是完整的。

## 三、语义 token 体系（styles/tokens.css `:root`）

### 3.1 层级模型

```
原始色（legacy）        --ink/-2/-3 · --paper/-2/-3 · --card · --pink/-deep/-soft/-bg
                       --lavender · --mint · --peach · --sun（分类色板）
状态色                  --ok · --warn · --err
边框/阴影               --line · --border · --border-strong · --shadow-sm/-/lg
语义层（新代码消费）    --bg · --surface/-2/-3 · --paper-1 · --muted
                       --text/-2/-3 · --accent/-strong/-soft/-bg
风险色                  --risk-safe · --risk-caution · --risk-danger · --risk-soft
                       --warn-soft · --err-soft · --warn-fg
填充                    --risk-fill · --risk-fill-d · --risk-fill-o
字体/圆角               --font-sans · --font-mono · --radius 及派生
```

### 3.2 语义 token 使用约定

| token | 用途 | 禁止 |
|---|---|---|
| `--bg` / `--surface` / `--surface-2/-3` | 页面背景 / 卡片 / 次级表面层级 | 用 `--paper` 画新卡片 |
| `--text` / `--text-2` / `--text-3` | 主文本 / 次级 / 弱化 | 用 `--ink` 写新文本 |
| `--accent` / `--accent-strong` / `--accent-soft` / `--accent-bg` | 主操作 / 强调 / 弱强调底 / 强调背景 | 用 `--pink` 做新按钮 |
| `--risk-safe` / `--risk-caution` / `--risk-danger` | 低/中/高风险徽标与语义红绿 | 用 `--ok`/`--err` 画风险（`--ok` 跟随 accent 会变色） |
| `--border` / `--border-strong` | 常规/强调边框 | 硬编码 #e8e8e8 之类 |
| `--radius` 及派生 | 统一圆角阶梯 | 每组件自造 radius |

> 已知待清理：`--ok`/`--warn`/`--err` 在旧组件里承担风险色，新组件一律走 `--risk-*`；`--pink` 系仅存量组件过渡用。

## 四、布局骨架（三视图）

```
┌──────────────────────────────────────────────────────────────┐
│ 顶栏：Logo · 视图切换（总览/工作台/工具墙）· 设置 · AI 侧栏开关   │
├──────────────────────────────────────────────────────────────┤
│ .app-content grid（三栏，可拖 Splitter，窗口缩放自动收缩）       │
│   左栏 LeftPanel（树/treemap/文件 tab） · 中栏详情 · 右栏 Studio │
├──────────────────────────────────────────────────────────────┤
│ 全局 AI 侧栏 ChatPanel（.ai-rail，可开合，占位影响三栏预算）      │
└──────────────────────────────────────────────────────────────┘
```

- 三视图：`overview`（总览）/ `workspace`（工作台三栏）/ `tools`（工具墙）。
- 三栏最小预算 `MIN_LEFT=320 / MIN_CENTER=360 / MIN_RIGHT=220`：内容区宽度不够时右栏自动隐藏、左栏收缩，中栏保底。
- 窗口缩放 / AI 栏开合后自动 fit 三栏（App.tsx useEffect）。

## 五、组件样式约定

| 组件 | 样式文件 | 关键约定 |
|---|---|---|
| 总览页 | overview.css | 磁盘卡 / 环形图 / 分类占用卡片墙（cache=绿 / media=橙 / backup=金 / envs=紫）/ Top N 大目录 / 建议卡三态（loading/empty/has） |
| 工具墙 | toolwall.css | `.toolbelt` 布局、工具卡（图标+名+风险徽标+入口角标+主分类）、详情 modal、回收确认（.recycle-confirm 420px） |
| 工作台 | layout.css | 树行（22px 高 + 占用百分比条）、Studio 卡片（category 分组：media 置顶 / cache 合并 / backup 置底 / envs） |
| AI 侧栏 | chat.css | 气泡、turns、工具确认卡（.cli-confirm-remember 会话免确认勾选） |
| Steam | steam.css | Steam 三栏 + 工具卡 |
| 设置 | settings.css | 设置弹窗 + .progress-button 进度填充按钮 |
| 深色 | dark.css | 历史补丁层（54 条 `html[data-theme="dark"/"pro"]` 覆盖，只换色不动布局） |

### 风险徽标约定

- 低风险 = `--risk-safe`（绿）· 中 = `--risk-caution`（橙）· 高 = `--risk-danger`（红）。
- 权限 L3 工具（格式化/刷 BIOS/超频/删系统文件）在工具墙显示红色「永禁」角标，启动被拦截并提示永久禁止。
- 危险操作按钮：首次点击变红 + 5 秒内再点一次才执行（ProgressButton 模式）。

### 图标与视觉

- 图标统一 lucide-react（stroke 风格），尺寸 14–16px 常规，视场合 20px。
- Logo：`components/Logo.tsx` SVG（粉色垃圾桶），`scripts/make-icon.ps1` 可再生成像素版 app 图标（32/128/256/512 PNG + ICO）。
- 文件/分类色板：`colors.ts`（5 套文件/分类色板），treemap/树视图按扩展名/分类取色。

## 六、性能与视觉

- **避免 backdrop-filter / 大面积模糊**：仅在 steam.css / layout.css 少量使用；新组件默认不用（低配机性能敏感，2026-09-07 专项确认过非卡顿根因但仍是约束）。
- 虚拟滚动（TreeView @tanstack/react-virtual）只渲染可视区；行组件无重渲染放大器（Row 已 memo）。
- 动画：仅 @keyframes 全局去重后的 spin/toast 等；新动画命名先查 styles/ 是否已有。

## 七、给视觉 AI 的快速起点

1. 想换皮 = 改/加 `PRESETS` 预设或调 token，组件不动。
2. 排查配色问题先查组件是否用了 legacy 色（`--pink`/`--ink`/`--paper`/`--ok`/`--err`），改为语义 token。
3. 深色主题问题优先修预设本身，`dark.css` 只是兜底补丁。
4. 布局改动集中在 `layout.css` + App.tsx 三栏逻辑；组件内部样式就近放（如 overview.css 管总览）。
5. 交接背景见 [ROADMAP.md](ROADMAP.md)（2026-09-05：视觉验收用 headless Edge CDP；遗留「流量卡广告」排查——源码层已干净，若再出现只可能是工具墙某 exe 自身弹窗）。
