# DiskPilot 下一步开发方向（2026-11-16，基线 v1.0.3 / 59c4b3d）

数据依据：插件索引实时下载数（2026-11-16，合计 126）、app 各版本累计下载（v1.0.1=32 / v1.0.2=22 / v1.0.3=22 自 11-12 发布 4 天）、R552 回顾与 R553 v1.0.2 审核的遗留项。

## 0. 现状速览

- 版本：v1.0.3 已发布（release 442696571，21 资产，latest.json=1.0.3），CI/CodeQL 绿，工作区干净，PR 队列清零（dependabot toml PR #4 因目标版本被 1.1.8 超越已关闭）。
- 插件下载排行：bigfile-finder 16、temp-hunter 15、dup-finder 15、startup-audit 15、log-hunter 14、port-hunter 11、browser-cache-hunter 10、drive-health 10、schtask-hunter 7、reg-hunter 7、svc-hunter 6。**硬盘空间/垃圾类占据前 5 的 4 席**，审计三件套垫底但稳定。
- 4 天净增仅 1 次下载——市场进入平稳期，新插件应瞄准"高频刚需"而非长尾。

## 1. 工程收尾（建议优先，成本低、收益是 CI 稳定性）

1. **release.yml / ci.yml 的 action 锁 commit SHA**（R549 审查遗留 M2）——release job 持有 contents:write + 签名私钥，tag 劫持即代码执行；dependabot 可维护 `# vX.Y.Z` 注释。
2. **weekly_report 同秒两次生成文件名覆盖**（nit）——文件名加毫秒/自增后缀。
3. **4 个 R542 遗留 eslint exhaustive-deps warnings**（App.tsx notify 依赖）——补依赖或重构。
4. **体检格排行榜改为实时拉取**（现在是 2026-11-08 静态快照）——插件索引已有 download_count，前端拉一次即可，去掉过期风险。

## 2. 新插件方向（按下载热度排序）

硬盘工具类是绝对主力，建议下一批插件全部落在「空间占用深度审计」（只读 + 入待确认清单，走现有 --json 契约）：

| 优先级 | 插件 | 理由 |
| --- | --- | --- |
| P0 | `icon-cache-hunter`（缩略图/图标缓存：Explorer thumbcache、系统图标缓存） | Windows 用户刚需，体量大，与 temp-hunter/log-hunter 同类用户群 |
| P0 | `recycle-audit`（回收站深度审计：各盘 $Recycle.Bin 实际占用、可安全清空项） | 与现有清理闭环天然衔接，但必须只读+确认窗删除 |
| P1 | `downloads-organizer`（下载文件夹审计：按类型/大小/日期聚合，重复大文件） | 下载文件夹是典型垃圾温床，与 dup-finder 互补 |
| P1 | `system-files-audit`（pagefile.sys / hiberfil.sys / swapfile.sys 占用报告，只读不改） | 大文件Top常客，用户常问"能不能删"，给只读答案+风险提示 |
| P2 | `game-cache-hunter`（Steam/Epic/育碧 shader 缓存审计） | Steam 盘点已有用户群，缓存清理是高频需求 |
| P2 | `browser-profile-audit`（浏览器 profile 瘦身：离线缓存、旧版本、崩溃转储） | browser-cache-hunter 的自然延伸 |

审计类（综合检测）暂不扩充——下载垫底且已有三件套覆盖启动项/服务/计划任务/注册表。

## 3. 产品增强方向

- **周报/通知实时化**：排行实时拉取（见 1.4）；通知中心支持富卡片（磁盘红盘一键跳转硬件页）。
- **体检格扩容**：把"今日体检"从插件格扩展到内置项（启动项变化、温度趋势、空间预警）的一键总览。
- **MCP 侧**：agent-server 工具已 77 个，考虑加 `disk_space_forecast`（结合 R519 回归预测给出预计填满日期的只读工具）。
- **增长**：README 徽章区已有静态下载数，可换成 shields.io 动态徽章（`img.shields.io/github/downloads/...`）。

## 4. 不建议做的

- 不再扩充审计类插件（边际收益低）；不做自动删除任何东西（红线：删除永远走确认窗）；不碰 updater 签名链（v1.0.3 链路已验证健康）。

## 5. 建议节奏

先做 §1 工程收尾（1-2 轮），再发一个插件 P0（icon-cache-hunter）+ 排行实时化，攒 2-3 个功能发 v1.0.4。每次发版仍走「发布前弹确认」铁律。
