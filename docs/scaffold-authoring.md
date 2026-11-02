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
- 勘测记录**不入库**：`docs/scaffold-requirements/` 已随公开仓库精简移除（`12c6c74`），
  逐应用勘测只留在 PR 描述 / issue / CHANGELOG；本仓库内可执行的形式就是
  「TOML + safety test」两者本身，测试头部的「实测勘测附录」即指向那份外部记录

## 2. 写 TOML

**用脚手架 CLI 起一份新脚本**（自动填 id/name/risk、写盘前自检、输出后续步骤）：

```bash
cargo run -p diskpilot-scaffold-lint -- new <id> [--name "显示名"] [--risk low|medium|high]
# 例：cargo run -p diskpilot-scaffold-lint -- new some-app --name "某应用" --risk low
cargo run -p diskpilot-scaffold-lint -- list     # 看现有脚本格式与风险分布
```

生成的 `scaffolds/<id>.toml` 带完整模板注释，再按下面的要点填充 scope。

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
cargo test -p diskpilot-scaffold <id>_safety
cargo run -p diskpilot-scaffold-lint -- scaffolds/*.toml   # 21 份 TOML 红线校验（另有 _templates/ 一份模板）
```

## 4. 本地验证

```bash
pnpm install && pnpm tauri dev      # Studio 里看到卡片、scope 数字合理
```

再对本机真实目录做 dry-run 预览清单，确认无误删项。

全仓门前自检（与 CI 同一条命令）：`pnpm check:rust`（fmt + clippy `-D warnings`）、
`pnpm -C apps/desktop check`（tsc + eslint + vitest）、`pnpm check:docs`（相对链接）。

## 5. 签名并上架社区索引（R498）

社区仓库安装强制 Ed25519 签名（未签名脚本只能本地粘贴安装），所以上架前必须签名：

```bash
# lint 先过（不合格脚本 sign 会拒绝），再签名；不带 --key 时现场生成新密钥对
cargo run -p diskpilot-scaffold-lint -- sign scaffolds/<id>.toml
# 已有私钥（续签/更新版本）时复用：
cargo run -p diskpilot-scaffold-lint -- sign scaffolds/<id>.toml --key <secret_hex>
```

输出的 `signer`（公钥 hex）与 `signature`（签名 hex）填进社区索引仓库
（`lxfebd/diskpilot-scaffolds-index`）对应条目的 `signer` / `signature` 字段。
签名对象是 TOML 全文（单文件 `scaffold.toml`），安装端 `scaffold_registry_install`
下载后强制验签 + sha256 校验 + 红线校验，任一步失败都不会落盘。

私钥只打印一次，请妥善保存；泄露后任何人都能冒充你签名，需要轮换密钥并通知索引维护者。

用户端拉取最新索引：Studio → 清理脚本 → 社区脚本仓库 → 「刷新索引」
（默认拉官方索引，`scaffold_registry_refresh` 命令，https + SSRF 校验 + 可选索引级验签）。

## 6. 提 PR

模板会带检查清单。CI 全绿（fmt / clippy / cargo test / scaffold-lint / vitest / tsc）即进入 review。
