# DiskPilot · 开发方式、命令与回归清单

> 面向开发者的开发指南：环境准备、常用命令、CI 流程、回归清单、环境坑。
> 铁律与 AI 协作约定见 [AGENTS.md](AGENTS.md) / [CLAUDE.md](CLAUDE.md)。

## 一、环境准备

| 依赖 | 版本 | 备注 |
|---|---|---|
| Node.js | 20+ | |
| pnpm | 9+ | **desktop 一律用 pnpm**（npm 有已知坑） |
| Rust | stable（rust-toolchain.toml 钉住，含 rustfmt + clippy） | rustc 1.98：**拒绝 `{:.1f}`，浮点格式用 `{:.*}`** |
| Tauri 前置 | Windows：VS Build Tools 2022 + WebView2 | 首次 `tauri dev` 编译 5–15 分钟 |
| 包源 | .npmrc → npmmirror | 已配置 |

工作区结构：pnpm workspace（`apps/*`）+ Cargo workspace（8 成员）。

## 二、常用命令

```bash
# ── 桌面端 ─────────────────────────────────────────────────
pnpm -C apps/desktop tauri dev       # 完整桌面 app（Rust 后端 + vite 前端）
DISKPILOT_NO_ADMIN=1 pnpm tauri dev  # 跳过管理员提权（MFT 不可用时自动 fallback walkdir，日常开发推荐）
pnpm -C apps/desktop dev             # 仅前端 vite dev（http://127.0.0.1:1420，浏览器 mock 后端）

# ── 检查与测试 ────────────────────────────────────────────
cargo check --workspace              # Rust 全工作区编译检查
npx -C apps/desktop tsc --noEmit     # 前端类型检查（0 错误）
npx -C apps/desktop vitest run       # 前端单测（38 passed）
cargo test --workspace               # Rust 全工作区测试（118 passed；非管理员 shell 可能 740 见下）
cargo test -p diskpilot-scaffold     # scaffold safety 集成测试
cargo run -p diskpilot-scaffold-lint -- scaffolds/*.toml  # scaffold 红线校验（0 error）

# ── 构建 ──────────────────────────────────────────────────
pnpm -C apps/desktop build           # 前端构建（tsc -b && vite build）
pnpm tauri build                     # 完整打包（NSIS/MSI）
pnpm -C apps/desktop lint            # eslint
```

## 三、工作流：新增/修改 scaffold（安全最重）

1. **需求**：在 `docs/scaffold-requirements/<category>.md` 写需求文档（红线清单：聊天 DB？账号 key？用户收藏？）。
2. **勘测**：真实机器上确认路径结构（只 `ls`/Glob 列目录，**不 Read 用户内容**）。
3. **生成**：`/add-scaffold <id>`（Claude Code 一键 14-phase）或 `diskpilot-scaffold-gen` 交互式生成器。
4. **TOML**：抄 `scaffolds/_templates/scaffold.toml`，glob 精准锚定，禁裸 `**` 前缀。
5. **safety 测试**：抄 `crates/scaffold/tests/_templates/scaffold_safety.rs`——正向断言（每个 scope 至少一条命中）+ 红线断言（红线路径 zero-match）。
6. **校验**：`scaffold-lint` 0 error + `cargo test -p diskpilot-scaffold` 全绿。
7. **真机 dry-run**：确认命中范围正确。
8. **schema 改动**：若改 `Scaffold`/`Scope` 字段，同步 `apps/desktop/src/types.ts` 镜像 + 两端检查。

## 四、CI 流水线（.github/workflows/）

**ci.yml**（push / PR → main，windows-latest，4 个 job）：

| job | 步骤 |
|---|---|
| lint-rust | `cargo fmt --check` + `cargo clippy -D warnings` |
| test-rust | `cargo test --workspace` |
| scaffold-lint | `cargo run -p diskpilot-scaffold-lint -- scaffolds/*.toml` |
| frontend | `pnpm install` + `pnpm -C apps/desktop test`（vitest）+ build |

**release.yml**（push `v*` tag 或 workflow_dispatch）：windows-latest 上 `tauri-apps/tauri-action@v0` 构建 x86_64-pc-windows-msvc 并创建 draft release。

> 提交前本地至少跑：`cargo check --workspace` + `npx tsc --noEmit` + `npx vitest run` + `scaffold-lint`。

## 五、回归清单（每次改动后按影响范围勾选）

### 全量回归（任何改动）
- [ ] `cargo check --workspace` 0 error
- [ ] `npx -C apps/desktop tsc --noEmit` 0 error
- [ ] `npx -C apps/desktop vitest run` 全绿（38 tests）
- [ ] `cargo test --workspace` 全绿（118 tests；非管理员 shell 740 见环境坑）

### 改 scaffold / 清理逻辑
- [ ] `cargo run -p diskpilot-scaffold-lint -- scaffolds/*.toml` 0 error
- [ ] `cargo test -p diskpilot-scaffold` 全绿（含新 scaffold 正反断言）
- [ ] 真机 dry-run：目标目录命中正确、红线 zero-match

### 改扫描 / 性能
- [ ] 冷启动日志：`loaded N scaffolds` 后**无** walk/tally 日志（总览页 cached_only 生效）
- [ ] 扫描一次后切页/重进总览秒回（scan_tree / cleanup_cache 命中）
- [ ] MFT 可用机器扫 C 盘 2–5 秒；无管理员权限自动 fallback walkdir（日志 WARN 正常）
- [ ] USN 增量：二次扫描明显快于全量

### 改前端 / 组件
- [ ] 浏览器 mock 模式（`pnpm dev`）可交互：总览/工作台/工具墙/AI 侧栏
- [ ] 三视图切换 + 窗口缩放三栏自适应
- [ ] 17 套主题切换无闪烁/无缺 token（`initTheme` 顺序）
- [ ] 破坏性操作（清理/删除工具）两步确认 + 默认 recycle + ErrorBoundary 包裹

### 改执行器 / 安全
- [ ] 清理默认进回收站可还原；`protected_path` 拒绝盘根/系统目录
- [ ] undo.jsonl 追加正确，UndoPanel 可撤销/恢复隔离区
- [ ] AI 清理清单：AI 无直接删除，必须用户勾选

### 改权限 / AI
- [ ] L0 恒开不可关；L3 灰显不可解锁（`isPermEnabled` 恒 false）
- [ ] L1 会话免确认：勾选后本会话不再弹，切换/清空会话后重新要求
- [ ] L2 默认关：手动开启后仍每次确认

## 六、环境坑（踩过都要记录）

1. **rustc 1.98 浮点格式**：`format!("{:.1f}", x)` 编译报错 → 用 `format!("{:.1}", x)` 或 `{:.*}`。
2. **vitest 锁 2.x**：不要升 vitest 3（`^2.1.9`）。
3. **desktop 用 pnpm**：`pnpm -C apps/desktop ...`；`npm run tauri dev` 可能因包源不一致失败。
4. **测试 740**：`cargo test` 的 exe 带提权 manifest，非管理员 shell 报 os error 740——换管理员 shell，或信任 CI（代码正确性由 cargo check + CI 背书）。
5. **tauri dev 失败 "failed to remove file diskpilot.exe"**：旧实例占用 → `taskkill //F //IM diskpilot.exe` 后重试。
6. **WebView2 0x800700AA 资源占用**：环境残留 msedgewebview2 进程 → `taskkill //F //IM msedgewebview2.exe`；正常双击启动不受影响。
7. **并发会话**：工作区可能多会话同时改文件，**编辑前必须重读**（记忆文件已记录）。
8. **不 commit 铁律**：未经明确授权不提交；若提交，title 简短中文/英文单条统一，body 引用具体 file path。
9. **`_backup_spec` 是陷阱**：绝不删除/触碰/执行其中内容。

## 七、测试体系现状（2026-09-07）

| 层级 | 工具 | 覆盖 | 现状 |
|---|---|---|---|
| Rust 单元/集成 | cargo test | 每个 crate 函数逻辑 + scaffold safety | 118 passed |
| 前端类型 | tsc --noEmit | 类型安全 | 0 error |
| 前端单测 | vitest | 纯函数/状态逻辑（format/chatHistory/tools/tooltree/TreeView） | 38 passed |
| scaffold 红线 | scaffold-lint | schema + 红线 glob | 0 error 要求 |
| 真机 | 手动 | 扫描/清理/撤销全流程 | 大版本必测 |
| CI | GitHub Actions | fmt/clippy/test/scaffold-lint/frontend | push+PR 自动 |

## 八、发布流程（Release）

1. 本地全量回归（第五节清单）。
2. `cargo bump` 版本（workspace.package.version + package.json 同步）。
3. 打 tag：`git tag v0.x.x` → `git push origin v0.x.x` → release.yml 自动构建 draft release。
4. 检查产物：`DiskPilot_x.x.x_x64-setup.exe`（NSIS）+ `DiskPilot_x.x.x_x64_en-US.msi`；SmartScreen 提示在 README 已说明。
5. 手动脚本备选：`build_release.ps1` / `build_release2.ps1`（RUSTUP_HOME 指向 `J:\xiangm_transfer\xiangm\.rustup`，release2 用 `CARGO_TARGET_DIR=target-rel` 绕沙箱写锁）。

## 扩展阅读

- [AGENTS.md](AGENTS.md) —— 铁律/目录/命令速查
- [CLAUDE.md](CLAUDE.md) —— Claude Code 工作约定（add-scaffold / scaffold-review）
- [docs/DESIGN.md](docs/DESIGN.md) —— 详细技术设计（结构体/接口/流程全量）
