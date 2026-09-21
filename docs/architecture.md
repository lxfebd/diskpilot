# DiskPilot 架构总设计

> 适用于：想读懂源码、修 bug、加功能的开发者。
> 代码位置：`apps/desktop/`（前端 + Tauri 后端）、`crates/`（7 个 Rust crate）、`scaffolds/`（清理脚本 TOML）。

## 1. 总体分层

```
┌────────────────────────────────────────────────────┐
│  React 18 + TypeScript 前端（apps/desktop/src）      │
│  Zustand store · i18n(15 命名空间) · 主题(light/dark) │
└──────────────────────┬─────────────────────────────┘
              Tauri IPC（invoke + event）
┌──────────────────────┴─────────────────────────────┐
│  Tauri 后端（apps/desktop/src-tauri/src，命令层）     │
│  scan / cleanup / ai / agent / toolbelt / hw / …    │
└──────────────────────┬─────────────────────────────┘
                       │ 依赖
┌──────────────────────┴─────────────────────────────┐
│  Rust workspace 7 crates（纯逻辑，可独立测试）        │
│  scanner · scaffold · scaffold-lint · executor      │
│  steam-inspector · toolbelt · agent-server          │
└────────────────────────────────────────────────────┘
        独立进程：agent-server（MCP stdio，可被外部 AI 挂接）
```

设计原则：**业务逻辑全部下沉到 crates**，src-tauri 只做命令编排与 IPC 序列化。每个 crate 都能脱离 Tauri 单测。

## 2. 扫描管线（scanner + src-tauri/scan.rs）

### 2.1 三条扫描路径

| 路径 | 条件 | 实现 |
|---|---|---|
| NTFS MFT 直读 | Windows + 管理员 + NTFS 卷 | `crates/scanner/src/mft.rs`，直接解析 Master File Table，整盘秒级 |
| USN 日志增量 | 已有全量树 + NTFS | `crates/scanner/src/usn.rs`，回放 USN Journal 只重扫变更文件 |
| jwalk 遍历 | 其他平台 / MFT 失败兜底 | `crates/scanner/src/lib.rs`，跨平台并行目录树遍历，UI 给出 MFT fallback 提示 |

### 2.2 数据流与内存预算

```
walk/MFT → 平铺 (path,size,children) → build_tree（复用扫描一趟数据）
  → suggestions_from_tree（scaffold 批量检测打标）
  → tag_and_truncate（深度裁剪：根下 100 / 50 / 20 三层展开上限）
  → 截断树过 IPC 给前端；完整树留在后端内存 scan_tree 缓存（按需下钻再取）
```

关键决策（2026-09 扫描卡死事故的修复，勿回退）：

- **IPC 只传截断树**，绝不整树克隆；选中节点以引用传递（前端 `selectedNode` 存节点本身，不做 DFS 反查）
- 每目录只保留 top-K 文件明细；系统目录黑名单（WinSxS / Installer / $Recycle.Bin / hiberfil.sys 等）不进树
- 进度事件按「文件数 + 时间」双阈值节流；扫描有取消通道
- TreeView 虚拟滚动；树只在「工作台」视图挂载（总览页不渲染巨树）

### 2.3 磁盘资产总览

`AssetOverview.tsx` 聚合：盘符卡片墙（DriveStrip 探测容量）、按 scaffold 分类的占用（父节点已打标则子节点不重复累计）、Top N 大目录、空间趋势曲线（`space_history.rs` 每次扫描落盘）。

## 3. 清理脚本体系（scaffold + scaffold-lint + executor）

### 3.1 一份脚本 = TOML + 安全测试

- `scaffolds/<id>.toml`：detect 规则（路径存在性）、scope 列表（glob + mode=recycle + 保留天数 prompt）、红线清单（protect 列表）
- `crates/scaffold/tests/<id>_safety.rs`：**正向断言**（该命中的缓存目录命中）+ **红线反向断言**（聊天 DB / 凭据 / 用户数据 0 命中）。CI 必跑，无测试不合入
- 装载即校验：`canonicalize_for_red_line` / `red_line_violations` 是**单一尺子**，CI lint、运行期安装、启动装载、agent-server 四处共用；命中红线的脚本直接拒绝装载并留 `scaffold_red_line_reject` 记录

### 3.2 执行器（executor）

- 删除一律走 `trash-rs` → 系统回收站；可选 7 天 quarantine（先移入 `~/.diskpilot/quarantine/`）
- 每次操作追加写 `~/.diskpilot/undo.jsonl`（含真实 `bytes_freed`），撤销面板按日志找回
- `confirm_gate(dry_run, confirmed)`：dry-run 放行，真删必须 `confirmed=true`，后端硬校验
- **两套闸**：人驱动命令只查 confirmed；AI 驱动命令（execute_ai_plan / WRITE_TOOLS / plugin_* 等）另查权限中心 perm_grants——避免 L2 默认关闭把普通用户锁死

### 3.3 前端确认流

所有破坏性 UI 走「清单预览 → 两步确认 → 执行」三件套（`ConfirmDialog` / `CleanupModal`），全仓禁用 `window.confirm`（有回归测试锁定）。高危 scope 需输入口令 `确认`（`DANGER_WORD` 数据常量，不进翻译表）。

## 4. AI 层

### 4.1 对话分析（src-tauri/ai.rs + ChatPanel）

- BYOK 四协议：Anthropic / OpenAI / Gemini / Ollama，Key 存本机
- 上下文只含**目录元数据**：路径名、大小、文件数、扩展名占比、≤20 条样本路径；永不读文件内容
- 拖拽目标 = 树节点 / treemap 区块；AI 回答 markdown 渲染（react-markdown）

### 4.2 agent-server（75 个 MCP 工具 = 64 只读 + 11 写）

- 独立二进制，MCP stdio 传输（rmcp 框架），可被 Claude Desktop 等任意外部 agent 挂接
- `tools/mod.rs` 用 `#[tool_router]` 声明全部工具；`tools/*.rs` 按域实现（disk/hw/net/process/sec/bench/steam/…）
- 写工具清单 `WRITE_TOOLS`（src-tauri/agent.rs）：process_kill / service_control / file_recycle / uninstall_app / stress_test(_gpu) / fan_control / fan_selfheal_fix / scheduled_task_manage / toolbelt_run / process_start——协议层要求 `confirmed`，桌面 UI 两步确认后才下发
- 路径守卫：从环境变量推导盘根/系统目录/主目录根并拒绝；WiFi 工具绝不读密码
- 桌面「工具墙 → AI 可操作」分区即这套工具的本地视图，带只读/可写徽标

## 5. 工具墙与硬件中心（toolbelt + hw.rs）

- 目录数据由后端 catalog 下发（12 分类 90+ 条目），CLI 走**白名单**执行、GUI 一键启动
- 四级权限模型：L0 只读 / L1 低危 / L2 需授权（默认关）/ L3 永禁；权限中心（PermissionCenter）统一管理 perm_grants
- 硬件中心：基准（CPU/内存/磁盘/GPU）、压测（温度熔断守门）、传感器快照/趋势/告警（fancmd = LibreHardwareMonitor 内核桥）、SMART、蓝屏转储分析、内存检测、体检报告
- `plugin_registry.rs` / `plugin_remote.rs`：插件市场，安装强制签名校验，SSRF 重定向防护

## 6. Steam 盘点（steam-inspector）

只读解析 Steam `appmanifest_*.acf`，输出游戏库/容量/安装时间；清理建议按 scaffold 分类，只出建议不删文件；着色器缓存清理由 `steam-shadercache.toml` 脚本执行。

## 7. 发布管线

`.github/workflows`：CI（fmt / clippy / cargo test / scaffold-lint / vitest / tsc / build）→ release.yml（tauri-action 三平台打包 → updater 签名 → 上传安装包 + `.sig` + 签名 `latest.json`）→ 应用内自动更新（tauri-plugin-updater）。签名私钥仅存 CI Secret，绝不入库。

## 8. 测试策略

| 层 | 手段 |
|---|---|
| Rust | 每 crate 单测 + 每 scaffold 一个 safety 集成测试（18 份）；MFT 用例需提权终端 |
| 前端 | vitest（store / tooltree / 组件回归），断言译文的用例 `setLang('zh')` 钉语言 |
| CI | 上述全跑 + scaffold-lint 红线闸 + i18n 覆盖率守卫（键集对等 + 硬编码中文棘轮） |
