# Changelog

DiskPilot 变更日志。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [SemVer](https://semver.org/lang/zh-CN/)。

## [未发布]

### 2026-10-14 AI 分区卡片显示相关工具（R135）

- 「AI 可操作」分区的每张卡片底部显示 top-2 相关工具名（复用 R134 的
  `similarAgentTools` 描述词袋打分，useMemo 缓存 O(n²) 结果）——列表层
  即见关联，不必逐个点开详情弹窗。无关联的卡不显示该行。
- i18n `toolbelt.wall.aiRelated`（zh/en 成对）；CSS `.toolwall-ai-related`
  单行截断防撑破卡片。

### 2026-10-13 AI 工具详情「同类推荐」（R134）

- AI 可操作工具的详情弹窗（agentDetail）底部新增「同类 AI 工具」——
  agent-server 的 MCP 工具没有 tags/category 元数据，同类判断靠描述词袋
  重叠打分（新纯函数 `similarAgentTools` + `descTokens`/`descOverlap`）。
  点击推荐项直接切换详情。
- 分词策略：拉丁/数字连续串一个 token；CJK 走双字滑窗组词（「温度」
  「获取」），比单字切分更贴近概念——「温度」vs「湿度」共享单字不再
  误判同类。中英停用词表（虚词/高频词），无标签描述不硬凑。
- 新文件 similar.ts + similar.test.ts（+9 tests，414 总 / 34 files）；
  i18n `toolbelt.wall.aiSimilar`（zh/en 成对）。

### 2026-10-13 甜甜圈压测温度曲线全长历史（R133）

- 退出摘要的「温度曲线」行从「最近 60 个采样窗口」改为全长历史——
  手动长压测（R111 支持任意时长）不再丢前段升温过程（室温→爬升→
  稳态）；峰值也从全长取（开头瞬时高温不会丢）。HUD 趋势线仍为
  60s 滚动窗口（画布小，只需近段）。
- 熔断场景（90℃ 必然在 60s 内触发）不受影响。gpu-donut.test.ts +1
  （>60 样本完整往返，405 总）。

### 2026-10-13 清理预览窗大小口径说明（R132）

- dry-run 预览窗的路径列表上方加一行说明：行内标注的是本次清理实际
  释放空间（dry-run 实测），头部合计含扫描估算，两者可能略有出入——
  诚实呈现口径差异，避免用户对「行内大小之和 ≠ 合计」产生困惑。
- i18n：`cleanup.previewSizeNote`（zh/en 成对）；样式
  `.cleanup-preview-size-note`。

### 2026-10-13 清理预览窗显示单文件大小（R131）

- 清理页 dry-run 预览窗的路径清单每条在行尾显示「实际释放字节」——
  大文件一眼可见；bytes_freed 缺失（后端未统计）的行省略大小。
- `DryRunPreview` 新增 `sampleEntries`（与 samplePaths 同序的路径+字节
  条目）；抽纯函数 `sampleEntriesByBytes`（与 samplePathsByBytes 同排序
  规则的条目版本）+4 tests（404 总 / 33 files）。预览行 flex 布局：
  路径省略 + 大小右对齐。

### 2026-10-13 清理预览路径按字节降序（R130）

- 清理页 dry-run 预览窗的路径清单（前 80 条）从「后端返回顺序」改为
  「实际释放字节降序」——大文件优先展示，用户一眼看到本次清理占最多
  空间的匹配项；bytes_freed 缺失（后端未统计）的沉底且保持相对顺序。
- 探活时保留 UndoEntry.bytes_freed（此前类型注解丢弃）；抽纯函数
  `samplePathsByBytes(entries, cap)`（cleanup/model.ts，默认 cap=80）+
  （+4 tests，400 总 / 33 files）。

### 2026-10-13 压测叠画 y 轴动态上界（R129）

- 历次压测叠画（CPU/GPU）y 轴从固定 0–100 改为动态上界 = max(100, 全部
  系列峰值)——长时间压测曲线罕见超 100°C 不再被顶部截断；正常 ≤100
  的显示与固定 0–100 完全一致（视觉口径不变）。
- 抽纯函数 `stressHistYMax` + 新测试文件 charts.test.ts（+3 tests，
  396 总 / 33 files）。

### 2026-10-13 dry-run 预览按字节降序（R128）

- 总览「建议清理」展开明细按可回收字节降序——大文件优先展示
  （PEEK_LIMIT 截断时留下的是占用最大的项，而不是后端返回顺序里的
  前几条）；bytes 缺失（后端未统计）的项沉底且保持相对顺序。
- 抽纯函数 `sortPreviewItemsByBytes`（不改入参数组）+ 单测
  （+3 tests，393 总）。

### 2026-10-13 GPU 压测历史叠画标题区分（R127）

- 甜甜圈历史叠画标题改为「历次 GPU 压测温度对比」——CPU（stress_test）
  与 GPU（gpu_donut）两个叠画区块同页并存，标题不再混淆；aria 同步。
- `StressHistoryChart` 加 `kind` prop（默认 cpu，向后兼容）。

### 2026-10-13 建议清理按可回收字节降序排序（R126）

- 总览「建议清理项目」按可回收字节降序排列——最大收益项（微信缓存/
  大块头）排最前，用户一眼看到哪些能清出最多空间；后端顺序不稳定
  （map 迭代序），前端统一口径。
- 抽纯函数 `sortCleanItemsByBytes`（不动原数组）+ 单测（+3 tests，390 总）。

### 2026-10-13 工具详情「同类建议」（R125）

- 详情卡新增「同类建议」区块：按 **tags 重叠**推荐其他工具（排除自身，
  按共享 tag 数排序取前 5），点击直接切换详情——看一个工具时顺带发现
  同场景的其它选择，不再需要退出搜索。
- 复用目录内既有 tags 数据（tool.tags / manifest.tags），无 tags 的工具
  如实不显示区块。

### 2026-10-13 甜甜圈压测温度曲线可视化 + 历次 GPU 压测叠画（R124）

- **曲线不丢**：甜甜圈退出把每秒温度采样序列化成「温度曲线」行（同
  stress_test 契约，`温度曲线：0.0s=…; 1.0s=…`），结果区渲染 GPU 温度
  折线（GPU 蓝，与传感器趋势图同色系）——之前曲线只在 HUD 短暂显示，
  退出即丢。
- **历次 GPU 压测叠画**：甜甜圈历史（localStorage）同图对比最近 8 次
  温度曲线（复用 R113 叠画组件，Y 轴 0–100℃ 可比），GPU 散热随时间的
  演变一眼可见。
- 纯函数 `tempHistCurveLine`（采样 → 曲线行）+ 双向契约单测：序列化
  后可被 `parseStressCurve` 原样解析回（+3 tests，387 总）。
- `StressTempChart` 加 `kind` 参数（cpu/gpu），图例/配色区分。

### 2026-10-13 甜甜圈压测摘要带 GPU 温度峰值 + GPU 压测温度曲线（R123）

- **甜甜圈（WebGL）**：退出摘要新增「峰值 GPU 温度 {temp}℃」——压测期间
  每秒轮询采样（R112 的温度趋势窗口）有值才如实带出，没采到温度（无 N
  卡 / hw_temperature 失败）不虚构，只报 FPS；温度窗口改由 ref 镜像，
  退出闭包能读到最新采样。
- **stress_test_gpu（后端）**：守门采样循环收集「温度曲线：0.0s=xx°C;
  1.0s=xx°C; …」行（同 stress_test 契约，前端 parseStressCurve 可直接
  复用），压测前温度作为 0s 起点基线。

### 2026-10-13 传感器通道纳入 GPU 温度（R122）

- **后端**：`sensor_trend` / `sensor_history` 快照新增 `gpu_temp_c` 字段
  （N 卡 nvidia-smi / A 卡 ADL 双通道，`hw::gpu_temp_c`）；JSON 契约
  `ts + cpu_temp_c + gpu_temp_c + mem_percent` 全可空，无 GPU/读失败记
  null 不虚构；text 输出加「/ GPU x.x℃」。历史旧行缺字段 → null，前端
  只画有值曲线（诚实性原则）。
- **前端**：趋势折线图加第三条 GPU 温度线（蓝）；长期视图健康度评级卡
  新增「长期 GPU 温度」维度（≥70℃ 高值占比，三档评级，无数据如实不
  评级）；修复护顶峰显示「CPU x℃ / GPU y℃」双值；新增 GPU 温度分布
  色阶条（同 heatDist 区间口径，无 GPU 读数不渲染）。
- 后端单测补 `gpu_temp_c` 契约断言；前端解析测试更新（R122 起含 GPU）。

### 2026-10-13 跑分峰值与最近差距结论（R121）

- **巅峰结论行**：跑分历史条形图（R115）下方新增「巅峰 {value} {unit}
  （{date}）」——历次最高分是哪次跑的、拿了多少，一眼落定；不足 2 次不
  对比。最近一次低于巅峰：「低 {pct}%」（琥珀）；已达或超过巅峰：「已达
  巅峰」（绿，不唱衰）。
- 纯函数 `scoreBest`（巅峰、并列取更近）/ `scoreRecentDelta`（低多少的
  正值，超巅峰 clamp 0）锁语义；3 个新单测（32 files · 384 tests）。

### 2026-10-13 工具墙搜索覆盖 AI 工具分区（R120）

- **搜索一次搜遍两个分区**：工具墙顶栏搜索此前只过滤 exe 工具墙，「AI 可操作」
  分区（agent-server 全部 MCP 工具）在搜索时整区消失——用户搜「sensor」「disk」
  看不到 AI 侧命中。现在搜索词同样按 name/description 过滤 AI 工具，两个分区
  一起搜；无词时照常整区展示，0 命中不显示空分区。
- 纯前端增强，复用现有搜索框与过滤口径。

### 2026-10-13 空间耗尽风险分级（R119）

- **耗尽风险分级**：R98 的「约 N 天满/已满」预测标签从单一琥珀色升级为三档
  视觉分级——≤30 天或已满红色加粗（critical）、≤90 天琥珀（warn）、余量
  充足绿色（ok）；预测不可用（释放中/不足 2 点）如实显示 ok 不唱衰。
- 纯函数 `spaceRisk` 锁定档位（null/30/90 边界）；4 个新单测
  （32 files · 380 tests）。

### 2026-10-13 长期健康度评级卡（R118）

- **评级卡**：长期历史视图顶部汇总一句人话结论——温度 / 内存各自把高值占比
  （≥70℃ / ≥85%）映射成评级：<5% 优秀、<20% 良好、否则关注；徽标按档位取
  --risk-safe / --risk-caution / --risk-danger 语义色。先出结论，再看下方
  分布明细；某一维度无有效样本如实显示「无数据」不评级。
- 纯函数 `rateHealth` 锁定档位（<5% / <20% / 否则）；2 个新单测
  （32 files · 376 tests）。
- 长期历史解读闭环收束：R114 只读历史 → R116 温度分布 → R117 内存分布 →
  R118 健康度评级。

### 2026-10-13 内存占用分布（R117）

- **分布条泛化**：R116 的温度分布条重构为通用 `DistributionBar`（ramp 区间
  色阶 + 高值阈值 + i18n 命名空间三参数注入），温度 / 内存两路复用；`buildBuckets`
  / `distSummary` 纯函数替代 `buildTempBuckets`/`tempSummary`（旧名薄封装保留，
  R116 单测与接线不动）。
- **内存占用分布条**：长期历史的 `mem_percent` 样本按 <50 / 50–70 / 70–85 /
  85–95 / ≥95% 分层（`MEM_RAMP` 单源，阈值口径与 sensor_alert 内存告警一致），
  摘要 + ≥85% 吃紧占比解读——「32GB 日常够不够挤」一眼可见。
- 单测 +4（泛化分层边界与阈值注入语义，32 files · 374 tests）。

### 2026-10-13 温度分布色阶条（R116）

- **长期历史温度分布条**：点「长期历史（只读）」后，除折线回放外再渲染一条
  分段色阶带——把全部历史 CPU 温度样本按 <50 / 50–60 / 60–70 / 70–80 / ≥80℃
  分层（色阶从清凉绿到过热红，阈值口径与告警一致），下方图例带各区间样本数
  + 峰值/均值/样本数摘要 + ≥70℃ 高温占比解读（>10% 才提示，否则如实「没有」）。
- 纯函数 `buildTempBuckets` / `tempSummary` 锁定分层与统计口径（非有限值跳过
  不猜 0、负值合法落入低温桶）；7 个新单测（32 files · 370 tests）。
- 色阶收敛到 colors.ts 单源 `TEMP_RAMP`（沿用 W4 色板收敛原则）。

### 2026-10-13 基准跑分历史对比（R115）

- **历次基准跑分对比条形图**：bench_cpu / bench_memory / bench_disk 的历史
  结果文本里已存分数（整数 ops/s、读带宽 MB/s、顺序读 MB/s），此前只折叠成
  文本逐条看。现在解析历次跑分画成「旧→新」条形图，最佳那次用强调色标出，
  Y 值缩放取最高/最低分——睿频/散热/后台负载对分数的影响、分数随时间漂移
  一眼可见。
- 解析纯函数 `parseScoreLine` / `buildScoreSeries` 锁定后端输出格式契约
  （含千分位逗号与科学计数法；无分数行如 dry-run 计划 / 被拒 / 失败条目如实
  跳过不猜 0）；6 个新单测（32 files · 363 tests）。
- 纯前端增强：零后端改动，数据源就是 localStorage 里已积累的跑分历史。

### 2026-10-13 压测温度可视化三件套（R111–R113）+ 只读传感器历史（R114）

- **压测实时温度监护 + 可选手动时长**（`b387d3d`）：CPU 压测期间并行轮询只读
  `sensor_trend`（每 2s，画实时温度曲线 + 峰值）；时长可选 5/30/60/120/300s
  （AIDA64 风格），确认弹窗同步显示所选时长。
- **GPU 甜甜圈 HUD 温度趋势线**（`841406b`）：GPU 压测 HUD 增加温度 spark 趋势
  （最近 60 点）+ 当前温度区间；温度读数缺失的点不虚构（诚实性约定）。
- **历次压测温度叠画对比**（`34b503a`）：把历史压测的温度曲线叠画在 0–100℃
  统一坐标系（≥2 条才渲染，最多 8 条，最近最亮），图例带时间 + 各次峰值——
  散热状态/峰值随时间的演变一眼可见。
- **只读 `sensor_history` 工具 + 长期历史视图**（本轮）：新增纯只读 MCP 工具
  `sensor_history`（读 `sensor-history.jsonl` 最多 500 条，不追加不裁剪——
  与会追加快照的 `sensor_trend` 分开，页面展示无写副作用）；趋势卡片新增
  「长期历史（只读）」按钮 + 非等时轴说明 + 空态区分；Rust 单测
  `points_from_rows_takes_tail_and_keeps_order` 锁定取尾/顺序/上限语义。

### 2026-10-13 维护

- **MCP 补齐 resources 与 prompts 能力面**：此前只实现 `tools`，现新增三个
  只读资源（`system://profile` 系统摘要 / `system://disk-usage` 磁盘分区用量 /
  `system://env` 关键环境变量，内容与同名工具同一口径）与两个任务模板
  （`system-report` 体检报告 / `cleanup-plan` 目录清理计划，供 AI 客户端一键填入），
  能力声明与 instructions 同步补全。协议冒烟实测通过。
- **Linux 交叉编译 cfg 技术债全量清偿**：Windows-only 子系统（hw/sys/sec/net/
  diskx 等）的 import / re-export / PS 常量 / helper 函数按消费方补
  `#[cfg(windows)]` 或 `#[cfg_attr(not(windows), allow(dead_code))]`，
  累计 49 个文件；`cargo check --target x86_64-unknown-linux-gnu --all-targets`
  从 25 errors + 120 warnings 清到 **0 errors 0 warnings**（agent-server 及
  全部 7 个 workspace crate），Windows 侧 check/test/clippy/fmt 全绿。

### 代码审查修复汇总（第 1–9 轮）

对全仓库（desktop 应用 / agent-server / scanner / executor / toolbelt）执行了
9 轮深度代码审查，覆盖：PS 注入面、写路径确认门、SSRF/路径守卫、资源上限、
二进制解析、前端 XSS/竞态、UI/UX 视觉、整体架构。以下为每轮修复清单。

#### 第 9 轮（整体架构优化）

**架构重构（消除跨进程重复）** 新增共享 crate `crates/psutil`（diskpilot-psutil），
把 desktop 与 agent-server 双份维护的 PowerShell 原语收敛为单源：

- **修复（高）** `ps_capture` 执行器双份漂移：desktop `hw.rs` 版无 UTF-8 首行
  注入（中文系统 GBK 乱码只能靠兜底解码），agent-server `ps.rs` 版有改良
  ——统一迁入 psutil（UTF-8 注入 + GBK 兜底 + CREATE_NO_WINDOW + 超时强杀），
  desktop 保留 `(script, timeout)` 两参薄封装转发，**desktop 同时获得 UTF-8
  根治能力**（此前缺失）；agent-server `ps.rs` 变为纯 re-export 薄壳，
  两调用面不变。
- **修复（高）** AMD ADL 显卡探针脚本 `ADL_PROBE_PS`（2927 字符）desktop
  `hw.rs` 与 agent-server `tools/hw.rs` **逐字节相同却双份维护**，任何一侧
  修改即漂移——迁入 psutil 单源，两侧 `use` 同一常量。
- **修复（中）** `escape_ps_single_quoted`（PS 单引号转义，命令注入铁律）
  desktop `startup.rs` 与 agent-server `sys.rs` 双份——迁入 psutil 单源；
  回归测试随迁（psutil 新增 +2 测试，desktop 侧删本地重复测试）。
- **净效果**：`-372/+66` 行（抽取共享），workspace 零横向依赖约束保持
  （psutil 为纯叶 crate，仅依赖 encoding_rs）。
- **验证**：`cargo check/clippy --workspace --all-targets` 0 新警告；
  workspace 测试全绿（psutil 2 项新测试通过）；前端 tsc/eslint/vitest 不受影响。

**架构审查结论（本轮确认健康，无需改动）**
- AppState 短生命周期作用域锁、无 await 持锁——并发模型无死锁面；
- 前端 `api.ts` 按域聚合、`format.ts` 字节格式化单源——无跨文件重复；
- workspace 各 crate 零横向依赖（ci.yml 声明与实现一致）；
- agent-server 13 个细分采集脚本与 desktop 3 个聚合脚本为不同封装层级
  （面向不同消费方），非重复，内容级比对无 >50% 重叠。

> 第 6–8 轮（UI/UX 视觉审查，`e403751` `d4a01bc` `740ba54`）见下方
> 「UI/UX 视觉审查」section。

#### 工程专项：fmt 存量清零（`b4c5a9a`）

- **修复（工程·中）** 全 workspace 43 处存量 fmt 差异（19 文件，HEAD 即
  dirty）——CI 三平台 `cargo fmt --all -- --check` 一直在红，护栏形同虚设；
  `cargo fmt --all` 机械重排清零，clippy/test 验证全绿。
- **修复（工程·低）** `rustfmt.toml` 的 `imports_granularity` / `group_imports`
  为 nightly-only 选项，stable rustfmt 静默忽略并持续告警，CI（stable）永远
  无法验证该声明——移除，stable 语义成为事实标准。

#### 工程专项：重复代码收敛（`b9cb596` `7f79811`）

- **修复（中）** agent-server `bsod.rs` 本地 `fmt_bytes`（GB/MB/B 三档）与
  `tools/mod.rs` 共享版（B/KB/MB/GB/TB 五档）重复——删除本地版改用共享版
  （净 -12 行；dump 大小 1KB~1MB 区间输出从原始字节变 KB，更可读）。
- **修复（低）** 前端 `extOf`（取扩展名）在 FileView/TreeView 各有一份私有
  实现——逻辑收敛 `format.ts` 单源（无扩展名返回 `''`），FileView 保留
  `'(none)'` 分组哨兵映射；+2 单测。
- **排查排除**：`emit` 3 行惯用模式（i18n/hwCache）不抽公共层（收益为零）；
  `ToolCard` 两组件同名不同义（目录卡 vs 图标卡）非重复；`fmtTime`/
  `relativeTime` 输出格式与 i18n key 不同，非真重复。

#### 工程专项：巨型文件拆分（`7568df9` `3af6564` `6e6f7d0`）

- **修复（工程·中）** desktop `hw.rs`（2259 行）按边界拆分：报告 Markdown/
  HTML 格式化（`html_escape`/`html_wrap`/`is_vram_overflow`/`fmt_gb`/`v_str`/
  `arr`/`build_hw_markdown`）与风扇建议规则（`build_fan_advice`）为无状态纯
  函数，连同 12 条单测搬入新模块 `hw_report.rs`；`hw.rs` 保留 Tauri 命令层，
  降到 1684 行。
- **修复（工程·中）** desktop `mcp.rs`（1156 行）拆分：stdio/http 连接与
  校验（`validate_http_url`/`validate_stdio`/`connect_stdio`/`connect_http`/
  `connect_server` + `McpService` 别名）连同 2 条单测搬入 `mcp_bridge.rs`；
  `mcp.rs` 降到 921 行。
- **修复（工程·中）** desktop `ai.rs`（944 行）拆分：免 key 联网搜索
  （`web_search` 命令 + `SearchHit` + `bing_parse`/`ddg_lite_parse`/
  `html_decode`/`strip_tags`/`attr_value`/`text_between`/`url_encode`/
  `fetch_limited`）搬入 `ai_net.rs`；`ai.rs` 保留请求代理层（SSRF 防护/
  手动重定向/取消），降到 723 行。命令注册改 `ai_net::web_search`。
- **方式**：同 crate 内模块移动、`pub(crate)` 导出、零行为变化——仅移动
  不改逻辑；130 passed / 2 ignored，clippy/fmt 全绿。
- **排查排除**：`cleanup.rs`（1009 行）命令层与逻辑层交织（命令内联引用
  多个逻辑函数、测试混居）；`plugin_registry.rs`（1546 行）类型/命令/
  持久化高度交织（`RegistryEnvelope`/`RegistryPlugin`/`LedgerEntry` 等
  结构体 + `verify` 验签内聚在头部），两者拆分收益递减风险上升，留待后续。

#### 第 5 轮（`d0815a5`）

- **修复（Bug 风险）** `agent-server report.rs` `extract_free_gb`：
  - 磨损行「寿命剩余约 92%」被误判为剩余容量（8 → 8.0 GB），健康 SSD 触发
    虚假的「磁盘剩余低于 20GB」告警；
  - 「已用 300GB / 剩余 120GB」结构取行首**已用**值而非**剩余**值；「扫同一行
    所有数字取最后」的兜底分支因首数字几乎总存在而**永远不可达**（死代码）。
  - 修复：跳过含 `%` 行、要求容量单位（GB/TB）、以关键词为锚取词后数值、
    兜底改取整行最后一个有效数字；+6 断言（含磨损反例与已用/剩余结构）。
- **修复（Bug 风险）** `mcp.rs` `mcp_call_tool` 无操作级超时：
  - 连接 + `tools/call` 无任何超时包裹，挂死的 MCP 服务器（stdio 子进程阻塞 /
    HTTP 无响应）会**永久卡住整个 AI 回合**（后端 await 不返回、前端 busy 永不清除）。
  - 修复：端到端 60s 兜底（probe/list 原有 10s 不变）；`timeout_secs=0` 在
    reqwest 语义是「立即超时」而非「不限时」——把一切请求打失败，显式按未设置处理。

#### 第 4 轮（`311f775`）

- **修复（安全漏洞·高）** `startup.rs` 注册表启动项值名 → **PowerShell 注入**：
  - 启停/删除启动项把**注册表值名**（外部数据——恶意软件可写成
    `x'; <任意 PS>`）直接拼进 PS 单引号字面量，操作本身成为代码执行入口；
  - 修复：新增 `escape_ps_single_quoted`（单引号翻倍，与 agent-server `sys.rs`
    同一条铁律）并接入三处拼接；+2 回归测试钉死恶意值名转义后无法闭合字面量。

#### 第 3 轮（`5ed2e10`，含第 1–2 轮全部修复）

**安全漏洞**
- `sys.rs`：AI 辅助工具拼接用户输入进 PowerShell 单引号串**未转义**（注入面，高）
  ——新增 `escape_ps_single_quoted` + 回归测试。
- `net.rs`：Wi-Fi 名称同理注入（高）——转义 + 测试。
- `mcp.rs`：本地命令/参数含 `\x00` 时 spawn 永远失败并遗留僵尸服务器（中）
  ——配置层拒绝 NUL + 测试。
- `advisor agent.ts/provider.ts`：AI 请求头 `x-goog-api-key` 提供方写死，改配置透传。

**Bug 风险**
- `executor.rs`：dry-run 清理把「未确认的路径」写进 undo 记录，撤销时错误恢复（高）
  ——dry-run 不产 undo；已确认路径才记录。
- `store.ts`：bytes_freed 在多轮清理后重复累加（清理统计虚高）。
- `ai.rs`：流式响应 **10MB 无上限**（多请求共吃内存）。
- `hwinfo.rs`：共享内存布局解析 `try_into().unwrap()` 前**偏移无界**（长度前检）。
- `mft.rs`：MFT 解析两处 `try_into().unwrap()` 前长度边界（加前检）。

**性能问题**
- `ai.rs`：跨主机重定向跟随（凭证/私有地址泄露风险）——禁自动重定向 + 逐跳校验。
- `cleanup.rs`：目录 walk 无上限（超大目录 OOM）——加深度/条目上限。
- `files.rs`：读文件**整块进内存**——改流式 + `MAX_DIR_ENTRIES=100_000` 上限。

**代码风格 / 改进建议**
- `process.rs`：`$pid`（PS 自动只读变量）改 `$targetPid`。
- `hw.rs`：`is_vram_overflow` 用 float 算术比较判断 vram 溢出（改用整型）。
- `bench.tsx`：GPU 压测历史 shape 与后端不一致（修复持久化/回读）。
- `lib.rs`：scaffold 加载遇坏文件直接失败（改跳过坏文件）。
- `gpu-donut` / `SteamInspector` / `market` / `ChatPanel` 等前端 9 处
  useCallback 依赖链 + 未用变量（lint 警告清零）。

#### 前置架构审计（`81xxx` 序列，审查前基础）

- 删死接口 / 去重工具 / 实记清理字节 / 后台禁全盘 walk。
- agent-server 采集收敛（去重 + TTL 缓存 + 温度口径统一）。
- 详见各提交：`808770d` `0ead78a` `c47d278` `21b846a` `9c431b0` `21ba5d7` `6291e5f`。

### 巨型文件拆分 + 前端 bundle 优化（`eb7a0d1`~`64b81b1`）

对全部点名巨型 Rust 文件执行**域拆分**（子模块化，函数按域迁入同名目录），
消除 400–520 行的单文件，按 crate 逐一提交，每提交前 check 0 警告 /
test 全绿 / clippy 0 / fmt 干净：

- **desktop（`apps/desktop/src-tauri/src`）**
  - `scan.rs` 470→422：抽 scaffold 标记域至 `scan/tag`；
  - `hw_report.rs` 440→342：抽风扇建议域（`build_fan_advice`）至 `hw_report/fan`；
  - `hw_test.rs` 413→371：抽温度采样域（TTL 缓存 + `sample_thermal_c`）至 `hw_test/thermal`；
  - `ai.rs` 488→245：抽单测至 `ai_tests`。
- **scanner（`crates/scanner/src`）**：`scan_core.rs` 504→405（`scan_core/build_tree`）、
  `walker.rs` 522→396（`walker/tally`）、`mft.rs` 452→332（`mft_tests`）、
  `usn.rs` 456→245（`usn/win`）、`usn_tree.rs` 436→237（`usn_tree/apply`）、
  `mft_tree.rs` 400→268（`mft_tree/build_node`）。
- **steam-inspector**：`lib.rs` 452→220（库扫描域 → `inspect`）。
- **scaffold**：`winapp2.rs` 490→295（Winapp2.ini 解析域 → `winapp2/parse`）。
- **agent-server（`crates/agent-server/src/tools`）**
  - `sys_boot.rs` 455→295：抽注册表枚举域至 `sys_boot/registry`；
  - `sys_services.rs` 475→394：抽解析辅助域至 `sys_services/parse`；
  - `bench_stress.rs` 408→305：抽 GPU 压测域（温度熔断守门）至 `bench_stress/gpu`。
- **toolbelt（`crates/toolbelt/src`）**：`tree.rs` 456→380（`tree/tree_helpers`）、
  `plugin.rs` 416→286（`plugin/validate` + `plugin/green`）。
- **executor**：`lib.rs` 447→261（回收站域 → `recycle` + 受保护路径判定 → `protect`，
  保持 `pub use` 顶层重导出，外部 crate 调用面不变）。

**前端 bundle 优化（`eb7a0d1`）**：页面级代码分割，主入口 **765KB→428KB（-44%）**，
11 个懒加载页面 chunk 288KB，预加载 vendor-markdown/vendor-react/vendor-tauri；
`tsc -b` 0 / eslint 0 / vitest 224 passed。

**净效果**：30 个拆分提交（`eb7a0d1` 起 15 个，含此前 scanner/toolbelt 系列），
全仓最大 src 文件从 522 行降至 429 行（`scan_core.rs`），无 >450 文件。
workspace `cargo check --workspace --all-targets` 0 警告（唯一可接受项 binrw
v0.11.3 future-incompat）、`cargo test --workspace` 全绿、clippy 0、fmt 干净。

### 工具清单扩容（`9517e2c`）

- toolbelt 工具清单 **30 → 69 条**（内存/外设/硬盘/烤机检测工具）；
- 移除 FPT64（BIOS 刷写，L3 永禁高危）与 USBDeview（可禁用/卸载 USB 设备）——
  缩小默认执行面，与安全模型一致；
- 测试断言同步扩容。

### UI/UX 视觉审查（第 6–8 轮，`e403751` `d4a01bc` `740ba54`）

对全前端做了 3 轮视觉审查（浏览器 1280×800 实渲染 + DOM 像素级取证 +
WCAG 对比度量化 + 视觉子代理交叉验证），修复清单按严重度排序：

- **修复（高）** `button.primary` 等 accent 填充文字用 `var(--ink)` 近黑字，
  在 vercel(#1447e6)/doubao/google/trae 等深色 accent 主题下对比仅 **2.9:1**：
  新增语义 token `--on-accent`（accent 上文字色，按亮度自动选白/深字，
  WCAG ≥4.5），`applyTheme` 预设路径与 `applyAccent` 自定义主色路径均计算；
  primary / `.seg-opt.active` / `.tree-row.selected` / `::selection` /
  `.trace-chip` / `.toolbelt-nav.active` 六类统一改用它。
  实测 vercel 2.9→**6.83**、claude 浅 accent 自动深字 **4.46**。
- **修复（中）** 系统优化页「刷新电源计划」按钮误复用 `system.startup.refresh`
  （显示「刷新启动项」）——新增 `optimizer.power.refresh` 中英 key。
- **修复（中）** `settings-group-title` 组标题 11px+ink-3 比正文还小还淡
  （层级倒挂）——统一 12px+ink-2。
- **修复（中）** 无效 token `--ink-1`（theme.ts 仅 ink/ink-2/ink-3）4 处引用
  全部静默回退失效——permOff 权限提示改 `--risk-caution` 琥珀强调色；
  设置页 select 改 `--ink`。
- **修复（中）** 清理页 21 条脚本 10 条长名被 ellipsis 截断且 hover 无提示
  ——`cp-script` 补 `title`。
- **修复（低）** 工作台无盘布局塌陷（`drives=[]` 时 ws-strip 不渲染，
  `aside.left` 落进 grid auto 行、1fr 行 479px 空白）——三栏+splitter
  显式 `grid-row: 2`，实测 227px→706px 铺满。
- **修复（低）** 设置页主题选择器 17 套主题 3 列分段控件溢出截断长中文名
  ——改自适应 flex 换行块（17 项全显 0 截断）；清理/工具墙空态弱色
  ink-3→ink-2（对比 3.5→10.09）。

**取证排除（非缺陷）**：脚本名单行省略为侧栏列宽物理限制；历史页空态
内容流式正常；字号滑杆数值与轨道精确对齐；「设置」在侧栏底部固定区为
成熟桌面工具标准布局。

---

## 较早版本

### v0.2.3

- AI 实事求是约束 + 守卫只读盘根放行 + 会话续跑。
- 并发会话遗留批量收尾（winapp2 预览 / Studio 重构 / McpServers 重排 /
  只读工具切 check_read / 样式 token）。
- 深色主题 5 主题全修复 + 布局/设置控件统一（见历史提交）。

## 格式说明

- `[未发布]`：当前工作区 HEAD 之后的累积变更（含已提交的 5 轮审查修复）。
- 严重度标注：`高/中/低` 遵循审查报告分级；修复均附回归测试。