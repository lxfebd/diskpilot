# DiskPilot 项目回顾报告（2026-11-12，HEAD cea8f8c / R551）

覆盖范围：全部 git 历史（734 个 commit，其中 R 编号轮次 386 条，R1–R551），v0.2.4..HEAD 共 146 文件 +8523/−556 行。

## 1. 总览

| 指标 | 数值 |
| --- | --- |
| 总 commit | 734 |
| R 编号轮次 | 386（R1–R551） |
| 正式版本 | v0.2.0 → v0.2.4 → **v1.0.0**（2026-11-08）→ v1.0.1 → v1.0.2（8 个 tag，4 个正式版已 publish，各 21 资产） |
| 官方插件 | 11 个全部线上 Ed25519 验签通过（8 硬盘工具 + reg/svc/schtask 审计三件套 @1.1.0 `--json`） |
| 前端测试 | 983（R375 时）→ **1334**（R547 时），93 个测试文件 |
| 后端测试 | 275（desktop lib）+ 99（agent-server 等），clippy -D warnings 零警告 |
| 门禁 | pre-release-check 21/21（fmt/clippy/audit/测试/插件/前端/self-test），check-changelog + check-docs-links（95 链接） |
| CI | 三平台 lint/test + CodeQL + self-test 冒烟 + Release 四平台自动发布 |

## 2. 阶段划分（按主题聚类 R 号）

- **R1–R100 早期建设**：扫描内核（MFT/USN 直读）、清理引擎与确认窗、插件市场雏形、首次自动更新发布（v0.2.0），三轮代码审查修复 20 项。
- **R100–R320 体验与 a11y 大专项**：52+ 轮无障碍/键盘/读屏/对比度/焦点管理修复（aria 角色、焦点进出弹窗、live region、键盘可操作列表/树/卡片），UI/UX 审查修复，i18n 中英对齐与守卫。
- **R320–R410 工程化与治理**：CHANGELOG 拆分归档、契约守卫（命令面/invoke、localStorage 键对偶、主题事实表）、死代码清理（406 行死 CSS、零引用导出）、观测持久化内核（obs jsonl）、USN 游标落盘。
- **R381–R521 v1.0.0 冲刺**：写操作确认闸门完整方案（R508 一次性 nonce + 来源校验 + 频率限制 + 审计，R509/R515 补齐到 100% 覆盖）、深度安全审查修复（R505：签名契约/路径穿越/SSRF DNS 二次校验/MCP 权限失配）、插件市场安全加固（R502/R503）、**11 个官方诊断插件分批上线**（R504/R507/R510/R512/R517/R521/R523/R527，全部 Ed25519 双签 + sha256 锚定公钥 + CI 结构门禁 + 客户端验签）、扫描报告导出、空间趋势预测、发布前验收清单（R514）、self-test 冒烟（R513）。
- **R522–R551 成熟期**：v1.0.0 正式发布（R522）、v1.0.1（R544，含 release.yml bundler 预下载抗 504）、通知中心/红标/待确认清单/导出（R524–R542）、v1.0.2（R548）、**深度代码审查 R504–R548 三路并行 + 修复**（R549：confirm_queue 原子写+锁、svc-hunter 注入防御、CSV/MD 转义、release.yml sha256 pin、ci 权限收紧）、项目整理（R550）、**rmcp 1.8→3.9 修复 RUSTSEC-2026-0462**（R551，CI audit 转绿验证）。

## 3. 安全主线（贯穿始终的红线）

1. **确认链**：所有写操作（清理执行、插件安装/卸载、MCP 写工具、agent 写）统一「确认面板 + 一次性 nonce（60s TTL 防重放）+ 主窗口来源校验 + 频率限制 + 审计落盘」，覆盖率 100%（R508/R509/R515），webview 伪造 confirmed 攻击面清零。
2. **插件供应链**：zip 结构门禁 + Ed25519 签名信封 + sha256 + 发布公钥锚定 + safe_join 防路径穿越 + signer 锚定（R497–R505/R518 实测：坏结构被 CI 拒、未重签被客户端拒）。
3. **路径/红线守卫**：盘根/系统目录/用户主目录写保护，unix 侧红线补全（mac /etc|/tmp|/var 解析漏放已修）。
4. **网络**：SSRF 私网判定独立模块（ai_ssrf），DNS 二次校验（R505）。
5. **依赖审计**：cargo audit 白名单 2 项（RUSTSEC-2024-0370/0429），R488 清零 15 项，R551 修复 RUSTSEC-2026-0462（rmcp DoS）——CI audit 持续绿。
6. **R549 审查又补一层**：confirm_queue 原子写+互斥锁+字段截断、svc-hunter cmd 注入防御、CSV 公式注入、MD 围栏击穿、localStorage 字段级校验、release.yml 预下载 sha256 pin + 不可变 URL。

## 4. 质量体系（越做越硬）

- 守卫测试成体系：命令面契约、i18n 覆盖（中英键齐备+无裸中文）、classParity（新 CSS 类必须有样式）、a11y（aria 标签/开关）、theme 对偶、localStorage 键对偶、MCP 工具目录与文档强一致、焦点样式源码守卫。
- 门禁从 14 项（R514）扩到 21 项；每次 R 都标「N/N 绿」才合并。
- 纯函数优先 + 单测覆盖：leaderboardTop、buildQueueCsv/Md、filterNotices、hwTemp*、disk_alert_lines 等都有专属测试。

## 5. 遗留项 / 已知风险

- release.yml 的 action 仍是可变 tag（未锁 commit SHA）——R549 审查 M2，计划 R552+ 处理。
- weekly_report 同秒两次生成文件名相同会覆盖（nit）。
- 4 个 R542 遗留的 eslint exhaustive-deps warnings（notify 依赖）待清理。
- v1.0.2 已发布但 R549+R551 修复尚未发版——建议尽快发 **v1.0.3**。
- 本地宿主：ops_bash 的 niubash exec-tree 损坏（dsh-ops <0.5.2 已知 bug），本轮用 terminal 绕过。

## 6. 经验教训

1. **先门禁后合并**：守卫测试让重构敢下手（R375 删 406 行死 CSS 零事故）。
2. **安全闸门要一次性做满**：R508 一次到位比补丁式补齐（R509/R515）省心——写命令清单应从设计上枚举。
3. **发布链路的外部依赖要 pin**：GitHub release-assets 504 → 预下载（R544）；continuous tag 可变 → sha256 pin（R549）；上游新漏洞 → 依赖升级要 willingness 跨大版本（rmcp 1→3，R551）。
4. **审查要三路并行**（后端/前端/CI 各一份报告）再统一修，覆盖面远大于单线。
5. **BOM/反斜杠这类字节级坑别手工塞进源码**——用转义序列 + 字节级脚本修 + 测试断言防回归（R545/R549 教训）。
