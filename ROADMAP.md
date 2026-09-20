# DiskPilot · 功能开发路线图

> 本文件是**面向功能的开发路线**：回答「下一步做什么、为什么、怎么验收」。
> 与 [TODO.md](TODO.md) 分工：TODO 是**执行台账**（当前聚焦 + 已完成记录 + 待决策），
> ROADMAP 是**分期规划**（把功能项排进阶段，每项带价值/动作/验收）。做完一项，
> TODO.md 里划掉并在括号补日期与证据，ROADMAP 对应行标记状态。
>
> 文档风格：中文 prose + 英文术语（`scope` / `glob` / `dry-run` / `recycle` 不翻译）。
> 铁律不因路线改变：清理先清单后确认、scope glob 不碰红线、写操作走权限门 + 守卫、不 commit。

## 现状定位（2026-09-10）

核心闭环已全部落地，功能面很全但**操作能力刚起步**：

| 维度 | 现状 |
|---|---|
| 前端 | 23 组件 / 3 视图 / 设置 6 标签 / 17 主题 / 18 项四级权限 |
| 后端 | 75 个 Tauri 命令（11 域子模块）+ 7 crates 零横向依赖 |
| AI | 22 内置工具 + 75 MCP 工具（64 只读 + 11 写操作）+ 可插拔用户 MCP 服务器 |
| 清理 | 18 份 scaffold / 90 scope / 回收站默认 / undo.jsonl 撤销 / 隔离区 |
| 平台 | Windows 优先（MFT / USN / 回收站），Linux 未真机验证 |

**路线主线**：① 把「信息提取」补齐为「操作控制」（写工具三件套 + 卸载助手）；② 把「扫一次的数据」复用到底（执行/建议/趋势都不再二扫）；③ 生态（社区 scaffold 仓库 + i18n + macOS/Linux）。

---

## 第一期 · 效率补强（P0 · 短平快，不动架构）

> 目标：现有数据一次扫描、处处复用；AI 从「能看」到「能给明确处置建议」。

### R1. execute_scope 全量复用扫描树缓存
- **现状**：TODO 标「部分完成」——目录粒度 + dry_run 已走内存树；文件粒度 / 带 `days` 仍全盘 walk（缓存树无 mtime 结构性约束）。
- **为什么**：磁盘 IO 是瓶颈（2026-09-07 性能专项结论），执行路径二扫是唯一剩的大头。
- **做法**：给缓存树节点补 `mtime`（扫描时已可取，仅需持久进 `scan_tree` 结构）；文件粒度 glob / `days` 过滤直接在内存树求值，树外文件命中时按 root 精确局部 walk（只走未缓存盘）。
- **验收**：`cargo test -p diskpilot-desktop --lib` 新增契约测试（带 days 与不带 dry_run 均不触发 walkdir）+ `cargo check --workspace` 0 警告；日志验证执行 `execute_scope` 期间零磁盘枚举。

### R2. AI 重复文件清理建议
- **现状**：`disk_find_duplicate_files`（agent-server）已按大小分组 + 头部 64KB 哈希，只出报告不出处置。
- **为什么**：用户真正要的是「哪份保留、哪份进回收站」，这是最安全的高价值清理场景（删除对象有另一份全等副本）。
- **做法**：新增内置 AI 工具 `propose_duplicate_cleanup(root)` → 全等哈希聚类（必要时全文件哈希二次确认）→ 返回「建议保留路径 + 建议回收路径列表」→ 走现有 `execute_ai_plan` 确认门。保留策略默认「路径最短保留」+「旧版本日期新的风险提示」，杜绝误选系统/运行中文件（守卫：正在运行的进程占用的文件跳过）。
- **验收**：工具墙「AI 可操作」出现该工具；冒烟断言（含构造重复目录 case）；`cargo check` / `tsc` / `vitest` 全绿。

### R3. 清理计划定时建议（只建议不自动删）
- **为什么**：DiskPilot 的价值在使用频率上——定时出清单让用户「每周一次点几下」替代「想起来了才扫」。
- **做法**：复用 `cleanup_suggestions` 引擎 + 已有扫描树基线，新增「定时建议」——要么 app 内轻计划（会话内提醒，无后台进程），要么系统任务（`schtasks` 生成 XML，属于写操作走 `sys.control` 权限门）。**只生成建议清单并弹通知，绝不自动执行**（铁律）。
- **验收**：总览页出现「上次建议生成时间」；清单与手动扫描结果一致（同一缓存树）。

---

## 第二期 · 操作能力扩展（P1 · 延续「工具要有操作能力」主线）

> 目标：AI 从只读顾问变成「你确认，我执行」的受控助手。全部沿
> `process_kill` / `service_control` 已验证的**三层防线**：agent.rs confirmed 硬校验 →
> 前端 `sys.control` 权限门（L2 默认关 + 每次确认）→ agent-server 守卫。

### R4. 写操作工具三件套
| 工具 | 做什么 | 守卫要点 |
|---|---|---|
| `file_recycle(path)` | 把文件/目录移入系统回收站（**可逆**） | PathGuard 拒绝盘根/系统目录/主目录根；跳过进程锁占用 |
| `process_start(target, args?)` | 启动应用/脚本（前台程序或已注册项） | 白名单：仅用户目录 / Program Files 下可执行文件；拒绝 `.bat`/`.ps1` 外裸脚本 + 一切系统路径 |
| `scheduled_task_manage(name, action, ...)` | 查/启/停/删计划任务 | 系统关键任务黑名单（`\Microsoft\*` 系统任务只读）；删除需确认 |

每工具：`#[tool(description)]` 注册 + 入 `WRITE_TOOLS`（agent.rs）+ `confirmPerm:'sys.control'`（tools.ts）+ 冒烟守卫断言。
- **验收**：`file_recycle` 真实回收往返（回收站可还原）；`process_start` 启动记事本成功且黑名单（`C:\Windows\system32\cmd.exe` 等）被拒；冒烟 3 新断言绿；cargo check / tsc / vitest 全绿。

### R5. 卸载助手
- **现状**：`app_list`（agent-server）已有完整已装程序清单（版本/发布者/大小）。
- **为什么**：「这台电脑能卸什么」是个高频问题，但用户不敢乱点「卸载」——DiskPilot 可给出**证据充分**的建议（大小 + 上次使用）+ 安全卸载入口。
- **做法**：新内置 AI 工具 `propose_uninstall_candidates()` → 按大小 + 系统程序过滤 + 出版者匹配过滤 → 候选清单；用户选定后走 `execute_ai_plan` 式确认门 → 调系统 `UninstallString`（带 `/S` 静默参数白名单，禁止任意参数透传）。
- **验收**：确认门拦截 Windows 关键程序（系统组件过滤）；真实卸载走 UAC；冒烟断言 + tsc / vitest 绿。

---

## 第三期 · 智能与趋势（P2① · 数据沉淀价值）

### R6. 磁盘空间趋势
- **做法**：每次扫描把 `{root, total_bytes, used_bytes, at}` 追加到 `app_data_dir/space-history.jsonl`（增量小文件）；总览页多日曲线 + 「本周新增 N GB 于 X」定位（复用 scan_tree 差集）。只读展示，清理走既有通道。
- **验收**：三次 mock 扫描后曲线正确；后端单测（追加/读回/缺文件静默）。

### R7. 硬件报告对比历史
- **做法**：`hw_report` 输出归档到 `app_data_dir/hw-history/*.json`；硬件页新增「历史快照」区，对比温度/SMART 磨损/通电时长变化。数据现成（hw_disk_health 已含 SMART），只差归档与展示。
- **验收**：两次采集可并排对比；tsc / vitest 绿。

---

## 第四期 · 生态与平台（P2②）

> ✅ R8/R9/R10 已全部落地（2026-09-10，未 commit）——状态/证据见 TODO.md 对应行。

### R8. 社区 scaffold 仓库 ✅
- **做法**：GitHub 仓库托管 `scaffolds/` 目录 + 索引 `index.json`（id/版本/签名）；app 内「Studio → 社区」列索引 → 一键拉取安装（Ed25519 签名与插件同管线）。
- **落地**：`scaffold_registry.rs`（seed index + 文件 override + https-only + sha256 可选 + **Ed25519 签名强制**）+ `scaffold_registry_list/install` 命令 + Studio「社区脚本仓库」区（签名徽标 + 两段式确认）。验证：desktop lib 85 / cargo 0 警告。
- **待仓库**：真实 GitHub 索引仓库（两占位条目无 url →「即将上架」）。

### R9. 国际化 i18n ✅
- **做法**：前端文案表 + `useT()`，后端错误文案保持中文（工具输出语义化在前端翻）。
- **落地**：`i18n.ts`（ZH/EN + `t(key, params?)` 占位符 + `useT()` useSyncExternalStore 即时重渲染 + localStorage `diskpilot.lang.v1` + `system` 读 navigator.language）；**默认中文零回归**（en 缺失 key 回退中文原文）；Settings.tsx 全量迁移。验证：i18n 7 单测 / vitest 80 / tsc 0。
- **待决策**：目标语言范围扩展（>2 语言）/翻译维护流程。

### R10. macOS / Linux 预编译版 ✅
- **做法**：Linux/macOS 代码层可移植（Windows-only 助手 cfg-gated）；CI 加三平台构建 matrix。
- **落地**：agent-server 跨平台审计——`ps.rs` 非 Windows fallback（明确中文错误；消费模块运行时各归各的降级分支）+ `hw.rs` nvidia-smi 三件套 cfg 门控；agent.rs 二进制名按平台取 `agent-server`/`agent-server.exe` 并断言钉死；ci.yml lint/test-rust 扩 ubuntu+macos+windows matrix（Linux 装 webkit2gtk 系统依赖）；release.yml 4 平台（win x64 / mac x64+arm64 / linux x64）+ APPLE_* secrets 自动签名。验证：cargo check 0 警告 / desktop lib 86 / agent-server clippy 0 / tsc 0 / vitest 80。
- **待真机**：三平台 `tauri build` 产物真机冒烟（扫描/清理/回收站各平台语义）——CI matrix 出产物后逐平台验证。clippy 存量噪音（~42 处 desktop/toolbelt）另立专项，清完 ci 加回 `-D warnings`。

---

## 持续安全与质量门（每期都必须过）

| 门 | 命令/标准 |
|---|---|
| Rust | `cargo check --workspace` 0 警告；`cargo test -p diskpilot-desktop --lib` 全绿 |
| 前端 | `npx -C apps/desktop tsc --noEmit` 0 错误；`npx -C apps/desktop vitest run` 全绿 |
| agent-server | `scripts/smoke_test.py` 44/44 起步，新工具逐项加守卫断言 |
| scaffold | `cargo test -p diskpilot-scaffold` + `cargo run -p diskpilot-scaffold-lint -- scaffolds/*.toml` 0 error |
| 铁律 | 清理先清单后确认 / glob 不碰红线 / 写操作 confirmed + 权限门 + 守卫 / 不 commit |

---

## 执行提议

- **先做第一期 R1→R3**：都不动架构，且直接踩在当前性能铁律与「AI 给明确处置」主线上。
- **再做第二期 R4**：写工具三件套是「操作能力」最直接的一步，安全防线全套现成，工期可控。
- R5～R10 每项独立可插队；R8/R9/R10 需先过产品决策（决策点已在任务内标注）。

> 变更记录：2026-09-10 初稿（承接 project-structure.html 功能全清单 + TODO.md 存量待办）。