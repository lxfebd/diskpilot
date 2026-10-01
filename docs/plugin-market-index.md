# 插件市场 · 社区索引契约

本文件定义 DiskPilot 插件市场**社区索引**的格式、发布流程与验证方法。
它是客户端 `plugin_registry`（`apps/desktop/src-tauri/src/plugin_registry.rs`）对接的外部契约——
客户端代码已完整落地，本文件是**建官方索引仓库时照此执行**的说明书。

## 1. 一句话

社区索引 = 一个 **HTTPS 托管的 `index.json`**，描述"可下载插件"的元数据（名称/版本/下载地址/期望哈希/签名者）。用户在设置 → 插件市场填索引 URL → 刷新后即可浏览/安装/更新/回滚。

代码侧已就绪：`plugin_registry_refresh`（拉取→验签→原子写）、`plugin_registry_config`（配置 URL）、前端 `toolbelt/market.tsx`（市场 UI）。唯一缺的是**官方索引仓库本身**。

## 2. 索引文件格式（schema 1）

`index.json` 顶层是一个**信封**：

```json
{
  "schema": 1,
  "source_url": "https://raw.githubusercontent.com/lxfebd/diskpilot-scaffolds-index/main/index.json",
  "fetched_at": 0,
  "signature": "",
  "signer": "",
  "plugins": [
    {
      "id": "my-app-cleaner",
      "name": "MyApp 缓存清理助手",
      "version": "1.0.0",
      "author": "社区作者",
      "description": "一键清理 MyApp 的缓存与日志（只读扫描 + 回收站删除）。",
      "category": "系统工具",
      "risk": "low",
      "tags": ["缓存", "清理"],
      "url": "https://github.com/author/my-app-cleaner/releases/download/v1.0.0/my-app-cleaner-1.0.0.zip",
      "sha256": "",
      "signer": "",
      "downloads": 0,
      "versions": [],
      "license": "MIT",
      "depends_on": [],
      "verified": false,
      "homepage": "https://github.com/author/my-app-cleaner"
    }
  ]
}
```

- 所有字段（除 `id/name/version/author/description/category` 外）都带 `#[serde(default)]`，**旧客户端兼容**：缺字段的条目也能解析。
- `schema` 必须为 `1`；不匹配时客户端拒绝写入并提示升级。
- `url` 必须 `https`（客户端强制，否则禁用安装按钮）。
- `versions[]` 可选：多版本发布时按**发布顺序倒序**（第一条 = 最新）；顶层 `version/url` 是 latest 的快捷字段。
- `min_app`（在 `versions[]` 条目内）：最低 DiskPilot 版本，不满足时前端禁用安装。

### 签名（可选但推荐）

- 无 `signature`/`signer` → 客户端接受，市场列表显示"未验签"。
- 有签名 → **强制验签**，失败保留旧索引并报错（绝不覆盖为未验签内容）。

签名对象 = `plugins` 数组的**规范 JSON 字符串**（`serde_json::to_string(&plugins)`，无 pretty、无尾随换行）的 Ed25519 签名，公钥 hex 放 `signer`。与插件包签名模型不同——索引签名覆盖整个 plugins[] 的 JSON 文本，无路径帧。

## 3. 发布流程（社区作者）

一个插件 = 一个 zip 包（内含 `tool.plugin.json` + 工具文件），在索引里占一个条目。

1. **打插件包**：用 toolbelt 的 `export_plugin_zip`（crates/toolbelt）把插件目录打包为 zip（含 `tool.plugin.json`）。
2. **上传**：把 zip 传到任何 HTTPS 直链（GitHub Releases 最省事），拿到 URL。
3. **（可选）签名**：用 `sign_entries` 对包内文件签名，公钥 hex 写进 `tool.plugin.json` 的 `signer`。
4. **提索引条目**：在官方索引仓库按模板加一条 `plugins[]`（把 `url`/`sha256`/`signer` 填上），提 PR。
5. **验证**：本地跑 `plugin_registry_refresh` 拉取新索引，能装、能更新、能回滚即通过。

### 发布清单

- [ ] zip 可被 `install_plugin_zip` 安装（含合法 `tool.plugin.json`）
- [ ] `url` 是 https 直链
- [ ] `sha256` 与 zip 实际哈希一致（客户端下载后强校验）
- [ ] 有签名的：`signer` 公钥 hex 与包内一致
- [ ] `id` 是合法 kebab-case，不与内置工具净化名撞车（撞车会被 `filter_builtin_dupes` 剔除）

## 4. 官方索引仓库布局（建仓模板）

```
diskpilot-scaffolds-index/
├── index.json          # 信封（schema 1，plugins[] 按 id 排序）
├── README.md           # 认领/发布说明，指回本文档
├── plugins/            # （可选）各插件 zip 的源文件目录，Releases 发布
└── .github/            # CI：校验 index.json 可解析 + schema=1（见下）
```

**建议 CI 门禁**（照 .github/workflows/ci.yml 风格）：

```yaml
- name: Validate index
  run: |
    python - <<'EOF'
    import json
    d = json.load(open('index.json'))
    assert d['schema'] == 1, "schema 必须为 1"
    assert len(d['plugins']) > 0
    for p in d['plugins']:
        assert p['id'] and p['name'] and p['version'] and p['url']
        assert p['url'].startswith('https://'), "url 必须 https"
    EOF
```

## 5. 客户端对接（已实现，供核对）

| 步骤 | 代码 |
|---|---|
| 读索引 URL | `load_registry_url`（`registry-config.json` 的 `registry_url`） |
| 刷新索引 | `plugin_registry_refresh`（confirmed + `plugin.manage` 双校验 → https → schema → 验签 → 原子写 + 台账） |
| 校验签名 | `RegistryEnvelope::verify`（对 `serde_json::to_string(&plugins)` 验 Ed25519） |
| 剔内置重复 | `filter_builtin_dupes`（按 `builtin_plugin_ids` 净化名比对） |
| 安装/更新/回滚 | `install_plugin_zip` + 台账 `LedgerEntry`（200 条裁剪） |
| 前端 | `toolbelt/market.tsx`（市场 UI，配置 URL / 刷新 / 安装按钮） |

## 6. 验证索引文件

```bash
# 1. JSON 可解析 + schema 正确（用上面的 python 片段）
# 2. 在 app 里：设置 → 插件市场 → 填 index.json 的 URL → 刷新
# 3. 预期：市场列表出现新条目；安装/更新/回滚可操作；有签名的显示"已验签"
```

---

状态：**契约已冻结（schema 1）**。改动 schema 必须同步 bump `REGISTRY_SCHEMA` 并让客户端升级提示生效。
