# 如何编写清理脚本（scaffold）

> 目标：为新应用增加一份"按 scope 无风险清理"的脚本。一份脚本必须同时交付 **TOML + 安全测试**，缺一不可合入。

## 0. 前置认知

- TOML 放 `scaffolds/<id>.toml`，测试放 `crates/scaffold/tests/<id>_safety.rs`
- 现成模板：[`scaffolds/_templates/scaffold.toml`](../scaffolds/_templates/scaffold.toml) 与 [`crates/scaffold/tests/_templates/scaffold_safety.rs`](../crates/scaffold/tests/_templates/scaffold_safety.rs)
- 红线尺子是单一实现：`crates/scaffold` 的 `canonicalize_for_red_line` / `red_line_violations`，CI lint、装载闸、agent-server 共用——本地过了哪都过

## 1. 实地勘测（不允许拍脑袋写 glob）

在**真实机器**上使用该应用一段时间，然后只列目录名（不读文件内容）：

1. 找到应用的数据根（常见：`%APPDATA%`、`%LOCALAPPDATA%`、`%USERPROFILE%\Documents\<App>`）
2. 区分三类目录：**缓存/可再生**（可清）、**用户数据**（收藏/存档/设置，红线）、**登录态/凭据**（红线）
3. 记录每个可清目录的重建行为（删了会不会自动重建、重建代价多大）

## 2. 写 TOML

结构要点：

- `detect`：路径存在性规则，决定 Studio 卡片是否出现
- `[[scope]]`：每个 scope 一组 glob + `mode = "recycle"`（唯一允许的删除模式）+ 可选保留天数 prompt
- `protect`（红线清单）：把勘测出的用户数据/凭据路径逐条列上
- glob 宁窄勿宽：早期 `node-modules` 模板把 Cursor/VSCode/游戏内嵌 node_modules 也命中了，这类教训是 36 份旧脚本被整体下架的原因

## 3. 写安全测试（CI 硬闸）

每个 scaffold 的测试文件包含两组断言：

- **正向**：构造假的缓存目录树，断言 scope glob 命中
- **红线反向**：构造聊天 DB / 凭据 / 用户数据目录，断言**所有 scope 的 glob 对它们 0 命中**

跑法（不需要提权）：

```bash
cargo test -p scaffold <id>_safety
cargo run -p scaffold-lint          # 18+1 份 TOML 红线校验
```

## 4. 本地验证

```bash
pnpm install && pnpm tauri dev      # Studio 里看到卡片、scope 数字合理
```

再对本机真实目录做 dry-run 预览清单，确认无误删项。

## 5. 提 PR

模板会带检查清单。CI 全绿（fmt / clippy / cargo test / scaffold-lint / vitest / tsc）即进入 review。

> Claude Code 用户在仓库根目录敲 `/add-scaffold <id>` 可一键走完全流程。
