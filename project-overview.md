# DiskPilot · 项目整体说明

> 项目定位、目标用户、功能全貌、里程碑与技术栈。想快速上手开发看 [AGENTS.md](AGENTS.md)；想深入了解某部分用文末的导航。

## 一句话

**扫盘 · 看懂 · 一条一条删干净。**

DiskPilot 是开源（MIT）的 Windows 磁盘清理桌面应用：秒扫整盘看清空间分配，把不认识的文件夹拖给 AI 问清楚，再按清理脚本（scaffold）逐项放心删——默认进回收站，永远不读你的文件内容。

## 给谁用

- **主力用户**：小白 / 普通用户。磁盘满了不敢乱删，需要"有人告诉我这个文件夹是什么、能不能删、删了会丢什么"。
- **进阶用户**：喜欢图吧工具箱那类工具集，需要硬件信息、压力测试、外设工具墙的集成体验。
- **开源贡献者**：愿意给常见软件写清理脚本（每份 = 一份 TOML + 一份 Rust safety 测试）。

## 三大能力

### 1. 秒扫整盘（磁盘地图）

- Windows 上直读 NTFS MFT（主文件表），整盘 C: **2–5 秒**；跨平台（macOS/Linux/移动盘）自动 fallback 到 jwalk 并行遍历。
- 输出双视图：左侧虚拟滚动树（每行带占用百分比条）+ 中间 d3-hierarchy treemap（大文件夹画大块）。
- 有 USN Journal 时走增量扫描（`scan_path_usn`），二次扫描秒级。

### 2. 拖给 AI 分析

- 把不认识的文件夹从树/路径拖进中间聊天框，AI 解释这是什么、能不能删、删了会丢什么。
- **BYOK**：填自己的 Anthropic / OpenAI / Gemini Key，或本地 Ollama 完全免费。
- **隐私**：只发目录元数据（路径名、大小、文件数、扩展名占比、≤20 条样本路径），路径脱敏（`$HOME`）；**永远不读文件内容**。AI 没有直接删除能力——只生成待确认清单，用户逐项勾选才执行。

### 3. 已知应用走专属清理脚本（scaffold）

- 18 份脚本 / 90 个清理项：微信 PC（23 项，3.x+4.x 兼容）、Conda 环境、浏览器缓存、开发缓存（npm/yarn/pip/cargo）、Firefox 缓存、Steam 着色器缓存、崩溃转储、系统临时、Docker Buildx、HuggingFace 缓存、OBS 录像缓存、IDE 缓存、QQ (PC)、Discord、游戏引擎缓存、Microsoft Teams、Zoom、安全软件套装。
- 每份脚本带 **safety 测试**（正向断言 + 红线断言），CI 强制跑——红线：聊天 DB、账号 key、用户收藏、加密物料永远 zero-match。
- 所有删除默认进**系统回收站**可恢复；可选 7 天隔离区；每次操作写 `undo.jsonl` 可追溯可撤销。

## 衍生能力（2026-09 已就位）

| 能力 | 说明 |
|---|---|
| **工具墙（toolbelt）** | 图吧工具箱集成，12 分类导航 + 图标网格；30 个工具 manifest（CLI 白名单执行 / GUI 启动），双击启动、单击详情卡（参数/示例/风险/权限级别） |
| **四级权限模型** | L0 只读恒开 · L1 受控（默认开+确认+会话免确认）· L2 高危（默认关+每次确认）· L3 永禁（格式化/刷 BIOS/超频/删系统文件不可解锁）。权限中心可开关 AI 对工具的调用能力 |
| **硬件中心** | CPU/GPU/内存/主板/磁盘 SMART 读取 + 压力测试（Prime95/FurMark/AIDA64）+ Markdown/JSON/HTML 报告导出 |
| **Steam 盘点** | 只读解析 Steam 库（libraryfolders.vdf + appmanifest.acf）：游戏清单、占用、最近游玩；Workshop 条目弹窗 |
| **检查更新** | 设置页「版本更新」区块，GitHub Releases API 对比版本 |
| **系统垃圾/大文件/重复文件** | 分类清理入口 + 大文件定位；重复文件查找开发中 |
| **首页资产总览** | 磁盘卡片墙 + 分类占用卡片墙（cache/media/backup/envs）+ Top N 大目录，点击定位回工作台 |

## 技术栈

| 层 | 技术 |
|---|---|
| 前端 | React 18 + TypeScript + Tauri 2 + zustand + d3-hierarchy + @tanstack/react-virtual + react-markdown + lucide-react + Vite 5 |
| 后端 | Rust workspace（7 个 crate）+ Tauri IPC（75 个命令） |
| 扫描 | NTFS MFT 直读（`ntfs` crate）/ jwalk 并行遍历 / USN Journal 增量 |
| 清理执行 | `trash` crate（系统回收站）+ 自建 quarantine + undo 日志 |
| AI | BYOK：Anthropic / OpenAI / Gemini / Ollama，JSON-mode 结构化输出 |
| 主题 | 17 套预设（CSS 变量驱动）+ 动态 accent + 字体缩放 |

## 安全模型（写死的红线）

1. **三层防御**：静态（scaffold-lint 红线校验 + safety 测试）→ 运行时（protected_path 守卫 + 两步确认 + 权限分级）→ 可逆（回收站/隔离区/undo 日志）。
2. **AI 最小权限**：AI 只出清单，无直接删除；执行必须用户逐项勾选。
3. **零遥测**：无账号、无上传、无错误上报；唯一对外请求 = 用户配置的 AI 服务商。

## 里程碑

| 阶段 | 内容 | 状态 |
|---|---|---|
| 核心 | 秒扫 + treemap/树 + scaffold 清理 + 撤销 | ✅ |
| AI 顾问 | 拖拽问 AI + 4 协议 + 待确认清单执行 | ✅ |
| 安全专项 | scaffold-lint 红线 + 8 份 scaffold safety 测试全绿 | ✅ |
| 性能专项 | 流式扫描 / USN 增量 / 内存树复用 / 批量剪枝 / TreeView 虚拟滚动 / 启动不遍历 | ✅ |
| 工具墙 | 图吧工具箱集成 + 权限中心 + 硬件中心 + Steam 盘点 + 检查更新 | ✅ |
| 首页改版 | 磁盘资产总览（卡片墙 + Top N + 可回收潜力） | ✅ |
| 更多脚本 | QQ / Discord / Zoom / Teams / 游戏引擎缓存 | ⏳ 待写 |
| 社区生态 | 用户自写/自分享脚本（scaffold new CLI + 社区仓库） | ⏳ 进行中 |
| 插件系统完整版 | 插件分发/下载/签名校验 | ⏳ 待产品决策 |
| 国际化 | 多语言 | ⏳ 待产品决策 |
| 跨平台 | macOS / Linux 预编译版 | ⏳ 长期 |

## 文档导航

| 文档 | 内容 | 读者 |
|---|---|---|
| [AGENTS.md](AGENTS.md) | AI 协作入口：铁律、目录导航、核心概念、命令速查 | AI / 新开发者 |
| [architecture.md](architecture.md) | 架构分层、模块职责、数据流、IPC 命令表 | 开发者 / AI |
| [DESIGN.md](DESIGN.md) | 视觉规则：主题系统、语义 token、组件样式约定 | 前端 / 视觉 AI |
| [development.md](development.md) | 开发方式、命令、回归清单、环境坑 | 开发者 |
| [component-api.md](component-api.md) | 23 个组件 + 22 个 AI 工具的 API 参考 | 前端 |
| [user-guide.md](user-guide.md) | 面向使用者的功能说明 | 用户 |
| [TODO.md](TODO.md) | 当前任务、优先级、进度 | 全员 |
| [CLAUDE.md](CLAUDE.md) | Claude Code 工作约定（scaffold 流程） | Claude Code 用户 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 讲人话的架构（带图） | 普通用户 |
| [docs/DESIGN.md](docs/DESIGN.md) | 详细技术设计（结构体/接口/流程全量） | 深度开发 |
| [docs/ARCHITECTURE_FOR_AI.md](docs/ARCHITECTURE_FOR_AI.md) | AI 视角完整架构 + 接手建议 | AI |
| [README.md](README.md) | 项目门面：下载、截图、贡献指南 | 所有人 |
