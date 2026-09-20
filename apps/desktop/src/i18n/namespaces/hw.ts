// 硬件表面：硬件检测报告卡片 / 压测确认面板 / 压测监控条 / 历史快照区（components/HwPanels.tsx）
// + 本机性能档位那一句话（hwProfile.ts）。
//
// 不进文案表的东西（属设计而非欠账，见 ./index.ts 不译清单）：
// - 与后端原值比对的字符串（health === '良好' / '注意'、确认门输入 === '确认'）；
// - 发给模型的上下文行（hwProfile.perfContextLine 恒中文）；
// - WMI / SMART / CrystalDiskInfo / ADL 的原始属性值与厂商型号串（直接透传显示）。
export const hw = {
  ns: 'hw',
  zh: {
    // GPU 实时行来源标签（nvidia-smi 优先，缺席时退 AMD atiadlxx ADL 兜底）
    'hw.liveSourceAdl': 'atiadlxx ADL 实时',
    'hw.liveSourceNvsmi': 'nvidia-smi 实时',
    'hw.liveSourceGpu': 'GPU 实时',
    'hw.gpuCountSuffix': ' · 共 {count} 块',
    'hw.vramOverflow': '≥4 GB（WMI 32 位溢出，实际可能更大）',

    // 通用兜底
    'hw.unknown': '未知',
    'hw.notReadable': '（不可读）',
    'hw.nowSuffix': ' · 当前 {mhz}',
    'hw.close': '关闭',
    'hw.cancel': '取消',
    'hw.refresh': '刷新',

    // 硬件检测报告卡片
    'hw.reportTitle': '硬件检测报告',
    'hw.reportLoading': '正在读取硬件信息…',
    'hw.reportFoot': '数据来源：WMI CIM 查询 + 图吧工具箱（CrystalDiskInfo /CopyExit）。读不到的字段留空，不编造。',
    'hw.cellMem': '内存',
    'hw.cellGpu': '显卡',
    'hw.cellTemp': '温度',
    'hw.cpuSpec': '{cores} 核 {threads} 线程 · {mhz} MHz',
    'hw.loadSuffix': ' · 占用 {pct}%',
    'hw.memSpec': '{count} 条 · {mhz} MHz',
    'hw.xmpInactive': '⚠ XMP/EXPO 可能未生效',
    'hw.xmpActive': '✅ XMP/EXPO 已生效',
    'hw.tempGpu': '{temp}℃（GPU）',
    'hw.sensorCount': '{count} 个传感器',
    'hw.sectionDisk': '磁盘',
    'hw.sectionBios': 'BIOS / 主板',
    'hw.diskHealthTitle': '硬盘健康（SMART {state}）',
    'hw.smartOk': '可用',
    'hw.smartLimited': '受限',
    'hw.powerOnHours': '通电 {hours}h',
    'hw.lifeSuffix': '· 寿命 {pct}%',

    // 压测确认面板（L1：逐项确认才执行）
    'hw.stressTitle': '硬件压力测试确认',
    'hw.stressTypeLabel': '测试类型：',
    'hw.stressTypeValue': '{label}（{tool}）',
    'hw.stressDurationLabel': '持续时间：',
    'hw.stressDurationValue': '{sec} 秒',
    'hw.stressTempLabel': '温度上限：',
    'hw.stressTempValue': '{temp}℃（超过自动停止）',
    'hw.stressNoteCpu': 'Prime95 将使 CPU 满载，风扇会高速运转。请确保散热正常，笔记本建议接电源。',
    'hw.stressNoteGpu': 'FurMark 将使 GPU 满载，显存压力大。请确保散热正常，机箱通风良好。',
    'hw.stressNoteGpuBench': 'FurMark 基准测试会跑满 GPU 约数分钟，结束后显示分数。',
    'hw.stressNoteCpuBench': 'AIDA64 基准测试会跑满 CPU 约数分钟，结束后生成报告。',
    'hw.stressNoteFallback': '高负载操作，请确保散热与电源余量充足。',
    'hw.stressWarn': '⚠️ 测试期间 CPU/GPU 将满载、温度升高。温度超限会自动停止；你也可以随时在监控条上点「停止」中断。测试结果只读，不修改任何硬件设置。',
    'hw.stressRemember': '本次会话免重复确认（今天不再逐次弹窗）',
    'hw.stressInputHint': '输入「确认」以解锁开始按钮',
    'hw.startTest': '开始测试',

    // 压测实时监控条
    'hw.monitorRunning': '{type} · {tool} 进行中',
    'hw.peakTemp': '峰值 {temp}℃',
    'hw.sensorUnavailable': '传感器不可读',
    'hw.stopTest': '停止测试',
    'hw.collapse': '收起',

    // 硬件历史快照区（只读归档 + 两份并排对比）
    'hw.histTitle': '硬件历史快照',
    'hw.histLoading': '读取历史归档…',
    'hw.histEmpty': '还没有快照。在「硬件报告」里生成一次报告后，之后每次生成都会自动归档（最多保留最近 30 份）。',
    'hw.histCount': '{count} 份',
    'hw.histRefreshTitle': '刷新归档列表',
    'hw.histOlder': '较旧归档',
    'hw.histNewer': '较新归档',
    'hw.histCompare': '对比 {from} → {to}',
    'hw.histNoComparable': '所选归档缺少可对比的磁盘健康字段。',

    // 本机性能档位（hwProfile：设置页展示的那一句话）
    'hw.tierWeak': '低配',
    'hw.tierMedium': '中配',
    'hw.tierStrong': '高配',
    'hw.perfLabel': 'CPU {cores} 核 · 内存 {mem} GB（{tier}）',
  },
  en: {
    // GPU live-row source tag (nvidia-smi first, AMD atiadlxx ADL as fallback)
    'hw.liveSourceAdl': 'atiadlxx ADL live',
    'hw.liveSourceNvsmi': 'nvidia-smi live',
    'hw.liveSourceGpu': 'GPU live',
    'hw.gpuCountSuffix': ' · {count} GPUs',
    'hw.vramOverflow': '≥4 GB (WMI 32-bit overflow, may be larger)',

    // Shared fallbacks
    'hw.unknown': 'unknown',
    'hw.notReadable': '(not readable)',
    'hw.nowSuffix': ' · now {mhz}',
    'hw.close': 'Close',
    'hw.cancel': 'Cancel',
    'hw.refresh': 'Refresh',

    // Hardware inspection report card
    'hw.reportTitle': 'Hardware inspection report',
    'hw.reportLoading': 'Reading hardware info…',
    'hw.reportFoot': 'Data source: WMI CIM queries + TBToolbox (CrystalDiskInfo /CopyExit). Fields we cannot read stay blank — nothing is invented.',
    'hw.cellMem': 'Memory',
    'hw.cellGpu': 'GPU',
    'hw.cellTemp': 'Temperature',
    'hw.cpuSpec': '{cores} cores {threads} threads · {mhz} MHz',
    'hw.loadSuffix': ' · load {pct}%',
    'hw.memSpec': '{count} sticks · {mhz} MHz',
    'hw.xmpInactive': '⚠ XMP/EXPO may not be enabled',
    'hw.xmpActive': '✅ XMP/EXPO enabled',
    'hw.tempGpu': '{temp}℃ (GPU)',
    'hw.sensorCount': '{count} sensors',
    'hw.sectionDisk': 'Disks',
    'hw.sectionBios': 'BIOS / motherboard',
    'hw.diskHealthTitle': 'Drive health (SMART {state})',
    'hw.smartOk': 'available',
    'hw.smartLimited': 'limited',
    'hw.powerOnHours': '{hours} h powered on',
    'hw.lifeSuffix': '· life left {pct}%',

    // Stress-test confirmation panel (L1: runs only after explicit confirmation)
    'hw.stressTitle': 'Hardware stress test confirmation',
    'hw.stressTypeLabel': 'Test type: ',
    'hw.stressTypeValue': '{label} ({tool})',
    'hw.stressDurationLabel': 'Duration: ',
    'hw.stressDurationValue': '{sec} s',
    'hw.stressTempLabel': 'Temp limit: ',
    'hw.stressTempValue': '{temp}℃ (stops automatically above)',
    'hw.stressNoteCpu': 'Prime95 pins the CPU at 100% and the fans will run at full speed. Make sure cooling is healthy; on a laptop stay plugged into mains.',
    'hw.stressNoteGpu': 'FurMark pins the GPU at 100% and puts heavy pressure on VRAM. Make sure cooling is healthy and the case has airflow.',
    'hw.stressNoteGpuBench': 'The FurMark benchmark drives the GPU flat out for a few minutes, then shows the score.',
    'hw.stressNoteCpuBench': 'The AIDA64 benchmark drives the CPU flat out for a few minutes, then produces a report.',
    'hw.stressNoteFallback': 'High-load operation — make sure cooling and power headroom are sufficient.',
    'hw.stressWarn': '⚠️ CPU/GPU will run at full load and temperatures will rise. The test stops automatically once the limit is hit; you can also click Stop on the monitor bar at any time. Results are read-only — no hardware setting is modified.',
    'hw.stressRemember': 'Skip re-confirmation for this session (no more popups today)',
    'hw.stressInputHint': 'Type “确认” to unlock the Start button',
    'hw.startTest': 'Start test',

    // Live stress monitor bar
    'hw.monitorRunning': '{type} · {tool} running',
    'hw.peakTemp': 'peak {temp}℃',
    'hw.sensorUnavailable': 'no temperature sensor readable',
    'hw.stopTest': 'Stop test',
    'hw.collapse': 'Collapse',

    // Hardware history snapshots (read-only archive + side-by-side comparison)
    'hw.histTitle': 'Hardware history snapshots',
    'hw.histLoading': 'Reading archives…',
    'hw.histEmpty': 'No snapshots yet. Generate one report in Hardware Report, and every later run is archived automatically (the last 30 are kept).',
    'hw.histCount': '{count} snapshots',
    'hw.histRefreshTitle': 'Refresh archive list',
    'hw.histOlder': 'Older archive',
    'hw.histNewer': 'Newer archive',
    'hw.histCompare': 'Compare {from} → {to}',
    'hw.histNoComparable': 'The selected archives have no comparable drive-health fields.',

    // Local performance tier (hwProfile: the one-line summary on the settings page)
    'hw.tierWeak': 'low-end',
    'hw.tierMedium': 'mid-range',
    'hw.tierStrong': 'high-end',
    'hw.perfLabel': 'CPU {cores} cores · {mem} GB RAM ({tier})',
  },
};
