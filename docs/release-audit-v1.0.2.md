# v1.0.2 发布级审核报告（2026-11-12，审核基线 tag v1.0.2 = 145d34e）

审核范围：`git diff v1.0.1..v1.0.2`（R545–R548），以及发布链路证据（CI run / release 资产 / latest.json）。

## 1. 改动范围

18 个文件 +292/−23，**全部为前端 UI + 文档 + 版本号三件套**（Cargo.toml / package.json / tauri.conf.json 的 1.0.1→1.0.2；Cargo.lock 变动由版本 bump 引起）。
**零后端 Rust 业务代码、零新增或修改 Tauri 命令、零 nonce 闸涉及、零自动写盘**。

三个功能：
- R545 待人工确认清单 CSV 导出（用户 save() 对话框主动触发，既有豁免路径，与 R533 Markdown 导出同款）
- R546 体检格嵌入官方插件下载排行 Top5（静态快照，采集日期标注 2026-11-08）
- R547 通知中心按类型筛选 tab（5 档 filter + aria-pressed）

## 2. 细审结论（三路并行只读审查）

| 功能 | 结论 | 问题 |
|---|---|---|
| R545 CSV 导出 | ⚠️ 1 HIGH | **CSV 公式注入未防护**（esc 只做 RFC4180 转义，未处理 `= + - @ \t \r` 前缀；数据源是诊断插件产物，导出后 Excel/WPS 打开可触发 DDE/WEBSERVICE）。**已在 R549 修复**（`'` 前缀 + 单测断言），v1.0.2 未包含 |
| R546 排行榜 | ✅ 通过 | 静态快照标注采集日期（过期性属设计内）；leaderboardTop 三级排序兜底稳定；纯文本渲染无 XSS；CSS 类全定义；i18n 双语齐备 |
| R547 筛选 tab | ✅ 通过 | 1 low：筛选 group 的 aria-label 复用「全部」与首个 tab 同名；loadNotices 字段校验浅（**R549 已加 kind 白名单 + Number.isFinite 校验**，未发版） |

RFC4180 结构转义本身完整（引号/逗号/CR LF 包裹 + 引号翻倍 + BOM + CRLF 行尾）。

## 3. 影响范围

只读渲染 + localStorage 读取 + 用户主动 save() 导出。不涉及删除/写盘/插件安装/MCP 写/确认链。回归面限于：体检卡片、历史页通知区、清单导出按钮三个入口。

## 4. 覆盖性证据

- 测试：R545 时 1328 → R546 1332 → R547 **1334 全绿**（93 文件），i18n coverage / classParity / a11y 守卫均在套件内
- 门禁：R548 提交时 pre-release-check **21/21**
- CI：release run **42543783808 四平台全部 success**（R544 的 bundler 预下载步骤生效，一次过）
- 发布物：release **438646931** published 2026-11-09T01:39:59Z，**21 资产**（10 安装包 + 10 `.sig` + latest.json + 4 平台 SHA256SUMS）；tag v1.0.2 → 145d34e；latest.json `version: 1.0.2`；已有真实下载（updater 链路工作正常）

## 5. 总评与建议

**v1.0.2 可放行**：无 critical，改动面小且全部只读/用户主动触发。但存在一个**已修复但未随 v1.0.2 发布的 HIGH**（CSV 公式注入，R549 修复）——建议尽快发布 **v1.0.3**（含 R549 审查修复 + R551 的 rmcp 3.9 安全升级），v1.0.2 用户可通过内置 updater 自动获得。
