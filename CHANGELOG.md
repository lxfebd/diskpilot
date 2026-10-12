# Changelog

DiskPilot 变更日志。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [SemVer](https://semver.org/lang/zh-CN/)。

## [未发布]

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