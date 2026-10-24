# crates/ —— DiskPilot Rust workspace

8 个 crate 构成全部后端逻辑，**纯逻辑层**，可脱离 Tauri 独立编译与测试。桌面后端（`apps/desktop/src-tauri`，包名 `diskpilot-desktop`）以 path 依赖消费其中 6 个；`agent-server` 是独立进程，与桌面后端走 MCP stdio 通信。

## 一览

| 目录 | 包名 | 产物 | 职责 | 源码规模 |
| --- | --- | --- | --- | --- |
| [agent-server](agent-server) | `agent-server` | bin | AI 可操作工具集 —— MCP（Model Context Protocol）stdio 服务，76 个工具（65 只读 + 11 写） | 110 文件 / 18,769 行 / 27 个测试模块 |
| scanner | `diskpilot-scanner` | bin | 跨平台磁盘扫描：NTFS MFT 直读、USN 日志增量、jwalk 并行兜底 | 20 文件 / 5,014 行 |
| scaffold | `diskpilot-scaffold` | lib + bin | 清理脚本装载与匹配：TOML 解析、红线单一尺子、WinApp2 规则 | 7 文件 / 1,946 行 / 24 个测试 |
| scaffold-lint | `diskpilot-scaffold-lint` | bin | 清理脚本静态校验器（CI 门禁） | 1 文件 / 398 行 |
| executor | `diskpilot-executor` | lib | 清理动作安全执行：回收站 / quarantine / 永久删除 + undo.jsonl | 4 文件 / 574 行 / 1 个测试 |
| toolbelt | `diskpilot-toolbelt` | lib | 图吧工具箱 CLI 工具目录：文档驱动的参数表、路径解析、白名单执行 | 15 文件 / 3,124 行 / 2 个测试 |
| steam-inspector | `diskpilot-steam-inspector` | lib | Steam 库只读盘点：解析 `.acf` / `.vdf`，只读元数据 | 5 文件 / 1,276 行 / 2 个测试 |
| psutil | `diskpilot-psutil` | lib | 共享 PowerShell 原语：执行器 + 单引号转义 + AMD ADL 探针脚本 | 1 文件 / 332 行 |

## 依赖方向

真实 path 依赖（解析自各 Cargo.toml，非推测）：

```
diskpilot-desktop (Tauri 后端)          agent-server (独立进程)
├─ diskpilot-scanner   (叶子)           ├─ diskpilot-scaffold      (叶子)
├─ diskpilot-scaffold                   ├─ diskpilot-steam-inspector (叶子)
├─ diskpilot-executor  (叶子)           ├─ diskpilot-toolbelt      (叶子)
├─ diskpilot-steam-inspector            └─ diskpilot-psutil        (叶子)
├─ diskpilot-toolbelt
└─ diskpilot-psutil

diskpilot-scaffold-lint (CI 门禁)
└─ diskpilot-scaffold   ← path 依赖
```

- **唯一「crate 依赖 crate」的地方是 scaffold-lint → scaffold**：红线实现
  （`red_line_violations` / `canonicalize_for_red_line`）与 `recycle_granularity`
  校验放在 scaffold crate 里单测，CI lint 与运行期装载**共用同一把尺**，杜绝
  「CI 一把尺、运行时另一把尺」的口径漂移（详见
  [docs/architecture.md](../docs/architecture.md) 3.1）。
- **执行器与扫描器不进 agent-server**：AI 侧只做只读盘点与工具调用，删除动作一律
  交由桌面端 executor 执行 —— 这也是 WRITE_TOOLS 权限模型的边界。
- **内部依赖一律走 path**：`diskpilot-scanner = { path = "../../../crates/scanner" }` 形式。
  根 `Cargo.toml` 的 `[workspace.dependencies]` **只收三方 crate**（serde / windows-sys /
  rmcp / trash …），不放内部 crate —— version.workspace / edition.workspace 这类版本字段仍走 workspace 继承。
- **agent-server 不进桌面依赖图**：桌面后端只在需要时 spawn 它，走 MCP stdio 通信。

## 目录约定

```
crates/<name>/
├── Cargo.toml     # 包名统一 diskpilot-<name>（agent-server 除外）
├── README.md      # crate 级说明（可选，被 check-docs-links 扫描链接）
├── src/           # lib.rs 或 main.rs
└── tests/         # 集成测试（scaffold 的 24 个脚本安全测试在此）
```

- 新增 crate 必须同步：① 根 `Cargo.toml` 的 `members`；② 本表；③ `docs/architecture.md`。
- 中文/英文混排：现有 Cargo.toml 与 README 行尾沿用 CRLF，新文件请与同目录保持一致。
- 本文件由 `scripts/check-docs-links.mjs` 校验其中相对链接。

## 相关文档

- [docs/architecture.md](../docs/architecture.md) —— 分层架构总览
- [crates/agent-server/README.md](agent-server/README.md) —— MCP 工具集详细文档
- [docs/changelog/README.md](../docs/changelog/README.md) —— 逐轮变更归档
