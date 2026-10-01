# 发布插件到社区市场（作者快速上手）

本文件面向**插件作者**：把你自己打包的 CLI 工具发布到 DiskPilot 社区市场，让所有人一键安装。
读者对象 = 会写 `tool.plugin.json`、有 GitHub 账号的人。完整契约见 [`plugin-market-index.md`](plugin-market-index.md)。

## 3 步发布

### 第 1 步：准备插件目录

一个插件 = 一个目录 + `tool.plugin.json`。目录里放工具本体（exe/脚本/资源），清单描述它：

```json
{
  "id": "my-app-cleaner",
  "name": "MyApp 缓存清理助手",
  "category": "系统工具",
  "risk": "low",
  "permission_level": "L1",
  "purpose": "一键清理 MyApp 的缓存与日志（只读扫描 + 回收站删除）。",
  "when_to_use": "用户报告 MyApp 占用越来越大时",
  "when_not_to_use": "用户只想要磁盘总览时",
  "invocation": {
    "mode": "cli",
    "launchable": true,
    "args_template": "--clean",
    "params": [],
    "timeout_secs": 120
  },
  "output": { "format": "stdout", "parser": "stdout", "schema": "自由文本" },
  "side_effects": "删除 MyApp 缓存目录（走回收站，可还原）",
  "examples": [{ "args": "--clean", "desc": "清理缓存", "expect": "输出已释放字节数" }],
  "tags": ["缓存", "清理"],
  "publisher": "社区作者"
}
```

`id` 必须是 **kebab-case 且全仓库唯一**（`my-app-cleaner`）。不要与内置工具净化名撞车（撞了会被市场自动隐藏）。

### 第 2 步：打成 zip（两种方式任选）

**方式 A（应用内）**：把工具目录放进 Tools 根 → 工具墙找到它 → 单击详情 →「插件化」→ 再点「导出 zip」。导出的 zip 内含 `tool.plugin.json`（清单在根），可直接上传。

**方式 B（本地命令行）**：有 Rust 工具链时，把这 3 行存成 `crates/toolbelt/examples/export_plugin.rs` 再跑：

```rust
fn main() {
    diskpilot_toolbelt::export_plugin_zip(
        std::path::Path::new("my-app-cleaner"),   // 插件目录（含 tool.plugin.json）
        std::path::Path::new("my-app-cleaner-1.0.0.zip"),
    )
    .expect("导出失败");
}
```

```bash
cargo run -p diskpilot-toolbelt --example export_plugin
```

两种方式产出的 zip 都能被 `install_plugin_zip` 安装（下载后 sha256 强校验 + 可选 Ed25519 验签）。

### 第 3 步：上传 + 填索引 + 提 PR

1. 把 zip 传到 **GitHub Releases**（自己仓库或官方索引仓库均可），拿到 https 直链。
2. 算好 zip 的 sha256（`sha256sum my-app-cleaner-1.0.0.zip`）。
3. 在官方索引仓库 `index.json` 的 `plugins[]` 加一条：

```json
{
  "id": "my-app-cleaner",
  "name": "MyApp 缓存清理助手",
  "version": "1.0.0",
  "author": "社区作者",
  "description": "一键清理 MyApp 缓存（回收站删除，可还原）。",
  "category": "系统工具",
  "risk": "low",
  "tags": ["缓存", "清理"],
  "url": "https://github.com/author/my-app-cleaner/releases/download/v1.0.0/my-app-cleaner-1.0.0.zip",
  "sha256": "<zip 的 sha256 hex>",
  "signer": "",
  "downloads": 0,
  "versions": [],
  "license": "MIT",
  "depends_on": [],
  "verified": false,
  "homepage": "https://github.com/author/my-app-cleaner"
}
```

4. 提 PR。索引仓库 CI 自动校验：schema=1 / 必填字段 / url 必须 https / url 填了则 sha256 必填。

## 发布清单

- [ ] `tool.plugin.json` 字段齐全，`id` 是唯一 kebab-case
- [ ] zip 能被 `install_plugin_zip` 安装（本地装一次验证）
- [ ] `url` 是 https 直链（GitHub Releases）
- [ ] `sha256` 与 zip 实际哈希一致
- [ ] （可选）Ed25519 签名：`signer` 公钥 hex 与包内一致 → 市场显示"已验签"

## 常见问题

- **没有 Rust 工具链怎么打包？** 用方式 A（应用内导出），不需要本地 Rust。
- **zip 传哪？** 任何 https 直链都行；GitHub Releases 最省事（自带版本管理 + 直链）。
- **索引要签名吗？** 不强制。无签名市场显示"未验签"，有签名强制验签失败则拒绝安装。
- **工具被市场隐藏了？** 检查 `id` 是否与内置工具（如 `hwinfo`/`crystaldiskinfo`）净化名撞车，改名即可。
- **想更新版本？** 上传新 zip，改 `index.json` 里该条目的 `version`/`url`/`sha256`（或加进 `versions[]`），再提 PR。

---

状态：**契约冻结（schema 1）**。改动 schema 需同步主仓库 `REGISTRY_SCHEMA`。
