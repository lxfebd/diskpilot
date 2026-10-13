import { describe, expect, it } from 'vitest';
import { parseStressCurve } from './bench';
import { parseTrendJson } from './charts';

// R99：压测温度曲线解析锁定格式契约——后端输出行「温度曲线：0.0s=52.1°C; …」
// 是中文格式（非文案），解析正则改动必须有测试保护（同 gpu-donut parseGpuTemp）。

describe('parseStressCurve（压测温度曲线解析）', () => {
  it('解析整行 → 有序点序列', () => {
    const text = '压测完成，最大温度 65.0°C\n温度曲线：0.0s=52.1°C; 1.0s=58.3°C; 2.0s=65.0°C';
    const pts = parseStressCurve(text);
    expect(pts).toEqual([
      { sec: 0.0, temp: 52.1 },
      { sec: 1.0, temp: 58.3 },
      { sec: 2.0, temp: 65.0 },
    ]);
  });

  it('支持小数秒与无小数数值', () => {
    const pts = parseStressCurve('温度曲线：0s=50°C; 0.5s=55.5°C; 1s=60°C');
    expect(pts).toEqual([
      { sec: 0, temp: 50 },
      { sec: 0.5, temp: 55.5 },
      { sec: 1, temp: 60 },
    ]);
  });

  it('无温度曲线行 → null；只有 1 个点 → null（不足画线）', () => {
    expect(parseStressCurve('压测完成，最大温度 65.0°C')).toBeNull();
    expect(parseStressCurve('温度曲线：0.0s=52.1°C')).toBeNull();
    expect(parseStressCurve('')).toBeNull();
  });

  it('与其它行混排也能找到曲线行', () => {
    const text = ['第一行：开始压测', '温度曲线：0.0s=40°C; 1.0s=41°C', '最后一行：结束'].join('\n');
    expect(parseStressCurve(text)).toHaveLength(2);
  });
});

// R111：sensor_trend format=json 实时通道解析——锁定与后端 rep_report.rs 的
// JSON 契约（{ total, points: [{ ts, cpu_temp_c?, mem_percent? }] }）。
describe('parseTrendJson（sensor_trend JSON 解析）', () => {
  it('解析标准输出 → 温度/内存点序列', () => {
    const text = JSON.stringify({
      total: 3,
      points: [
        { ts: 1000, cpu_temp_c: 52.1, mem_percent: 41.2 },
        { ts: 1001, cpu_temp_c: 58.3, mem_percent: 43.0 },
        { ts: 1002, cpu_temp_c: 65.0, mem_percent: 44.5 },
      ],
    });
    expect(parseTrendJson(text)).toEqual([
      { ts: 1000, cpu_temp_c: 52.1, mem_percent: 41.2 },
      { ts: 1001, cpu_temp_c: 58.3, mem_percent: 43.0 },
      { ts: 1002, cpu_temp_c: 65.0, mem_percent: 44.5 },
    ]);
  });

  it('cpu_temp_c 为 null 的点跳过该字段（后端无温度源时如实记 null）', () => {
    const text = JSON.stringify({
      total: 2,
      points: [
        { ts: 1000, cpu_temp_c: null, mem_percent: 40.0 },
        { ts: 1001, cpu_temp_c: null, mem_percent: 41.0 },
      ],
    });
    const pts = parseTrendJson(text);
    expect(pts).toEqual([
      { ts: 1000, mem_percent: 40.0 },
      { ts: 1001, mem_percent: 41.0 },
    ]);
  });

  it('非 JSON / 缺 points / 不足 2 点 → null（不足画线）', () => {
    expect(parseTrendJson('不是 JSON')).toBeNull();
    expect(parseTrendJson('{"total":5}')).toBeNull();
    expect(parseTrendJson('{"points":[{"ts":1,"cpu_temp_c":50}]}')).toBeNull();
    expect(parseTrendJson('{"points":[]}')).toBeNull();
  });

  it('非法点（缺 ts / 字段类型错误）跳过，其余保留', () => {
    const text = JSON.stringify({
      points: [
        { ts: 1000, cpu_temp_c: 50 },
        { ts: 'bad', cpu_temp_c: 88 },
        { cpu_temp_c: 90 },
        { ts: 1002, cpu_temp_c: 'na' },
        { ts: 1003, cpu_temp_c: 51 },
      ],
    });
    expect(parseTrendJson(text)).toEqual([
      { ts: 1000, cpu_temp_c: 50 },
      { ts: 1002 },
      { ts: 1003, cpu_temp_c: 51 },
    ]);
  });

  it('非法点过滤后只剩 1 个 → null（不足画线）', () => {
    const text = JSON.stringify({
      points: [
        { ts: 'bad', cpu_temp_c: 88 },
        { ts: 1000, cpu_temp_c: 50 },
      ],
    });
    expect(parseTrendJson(text)).toBeNull();
  });
});

// R115：跑分历史对比——锁定后端 bench_cpu / bench_memory / bench_disk 的
// 输出行格式契约（行前缀中文格式，非文案），解析失败条目如实跳过。
import { parseScoreLine, buildScoreSeries } from './charts';

describe('parseScoreLine（跑分行解析）', () => {
  it('bench_cpu：整数 ops/s 主分数行', () => {
    const text = 'CPU 基准结果：\n线程：8 · 实测时长：3.0s\n整数（质数筛）：12345678 ops/s\n浮点（π 级数）：9.9e7 iters/s\n';
    expect(parseScoreLine(text, '整数（质数筛）')).toEqual({ value: 12345678, unit: 'ops/s' });
    expect(parseScoreLine(text, '浮点（π 级数）')).toEqual({ value: 99000000, unit: 'iters/s' });
  });

  it('bench_memory：读带宽 MB/s（千分位与小数）', () => {
    const text = '内存基准结果：\n读带宽：12,345.6 MB/s\n写带宽：8,888 MB/s\n复制带宽：10,000 MB/s\n延迟（指针追逐）：72.3 ns\n';
    expect(parseScoreLine(text, '读带宽')).toEqual({ value: 12345.6, unit: 'MB/s' });
    expect(parseScoreLine(text, '写带宽')).toEqual({ value: 8888, unit: 'MB/s' });
  });

  it('bench_disk：顺序读（128KB 块）MB/s', () => {
    const text = '磁盘基准结果：\n目录：C:\\TEMP（临时文件已删除）\n顺序读（128KB 块）：3500 MB/s\n随机读（4KB 块）：120 MB/s\n';
    expect(parseScoreLine(text, '顺序读（128KB 块）')).toEqual({ value: 3500, unit: 'MB/s' });
  });

  it('无匹配行 / 数值为 0 / 非法单位 → null', () => {
    expect(parseScoreLine('CPU 基准结果：\n线程：8', '整数（质数筛）')).toBeNull();
    expect(parseScoreLine('整数（质数筛）：0 ops/s', '整数（质数筛）')).toBeNull();
    expect(parseScoreLine('镀金：10 分', '整数（质数筛）')).toBeNull();
    expect(parseScoreLine('', '读带宽')).toBeNull();
    // dry-run 计划文本没有分数行 → null（不误出图）
    expect(parseScoreLine('磁盘基准计划（dry-run）：\n目录：C:\\TEMP', '顺序读（128KB 块）')).toBeNull();
  });
});

describe('buildScoreSeries（历次跑分 → 对比序列）', () => {
  const cpu1 = 'CPU 基准结果：\n整数（质数筛）：10000000 ops/s';
  const cpu2 = 'CPU 基准结果：\n整数（质数筛）：12000000 ops/s';
  const cpuFail = 'CPU 基准结果：\n线程：8 · 实测时长：0.1s\n整数（质数筛）：0 ops/s';
  const dryRun = '磁盘基准计划（dry-run）\n目录：C:\\TEMP';

  it('旧→新序，跳过无有效分数条目', () => {
    const series = buildScoreSeries(
      [
        { at: 1000, label: '首次', result: cpu1 },
        { at: 2000, label: '失败', result: cpuFail },
        { at: 3000, label: '最佳', result: cpu2 },
        { at: 4000, label: '计划', result: dryRun },
      ],
      '整数（质数筛）',
    );
    expect(series).toHaveLength(2);
    // 旧→新
    expect(series[0].value).toBe(10000000);
    expect(series[1].value).toBe(12000000);
    expect(series[1].at).toBe(3000);
    expect(series[1].unit).toBe('ops/s');
  });

  it('全无有效分数 → 空数组（不渲染图）', () => {
    expect(buildScoreSeries([{ at: 1, label: 'a', result: dryRun }], '顺序读（128KB 块）')).toEqual([]);
    expect(buildScoreSeries([], '整数（质数筛）')).toEqual([]);
  });
});

// R116：温度分布色阶条——锁定 TEMP_RAMP 分层口径（<50/50-60/60-70/70-80/≥80℃）
// 与摘要统计（只统计数字温度，非有限值跳过不猜 0）。
import { buildTempBuckets, tempSummary } from './charts';

describe('buildTempBuckets（温度分布桶）', () => {
  it('按区间分层，边界值归入下一档', () => {
    const buckets = buildTempBuckets([45, 50, 59.9, 60, 69, 70, 79, 80, 85]);
    expect(buckets.map((b) => b.count)).toEqual([1, 2, 2, 2, 2]); // <50 | 50-60 | 60-70 | 70-80 | ≥80
  });

  it('非有限值跳过（不猜 0），全空输入 → 各桶 0', () => {
    const buckets = buildTempBuckets([55, NaN, Infinity, 65]);
    expect(buckets.map((b) => b.count)).toEqual([0, 1, 1, 0, 0]);
    expect(buildTempBuckets([]).every((b) => b.count === 0)).toBe(true);
  });

  it('负值是有限数、合法落入第一桶（-1℃ 记 <50℃ 而非丢弃）', () => {
    expect(buildTempBuckets([-1])[0].count).toBe(1);
  });

  it('桶顺序与颜色来自 TEMP_RAMP（色阶单源）', () => {
    const buckets = buildTempBuckets([50]);
    expect(buckets[0].label).toBe('<50℃');
    expect(buckets[1].label).toBe('50-60℃');
    expect(buckets[4].label).toBe('≥80℃');
    // 颜色已从 colors.ts 注入，非空即可
    expect(buckets.every((b) => b.color.length > 0)).toBe(true);
  });
});

describe('tempSummary（温度摘要）', () => {
  it('峰值/均值/样本数/高温占比', () => {
    const s = tempSummary([50, 60, 70, 80]);
    expect(s).not.toBeNull();
    expect(s!.peak).toBe(80);
    expect(s!.mean).toBe(65);
    expect(s!.n).toBe(4);
    expect(s!.hotRatio).toBe(0.5); // ≥70 两个
  });

  it('无有效样本 → null（调用方不渲染摘要）', () => {
    expect(tempSummary([])).toBeNull();
    expect(tempSummary([NaN, Infinity])).toBeNull();
  });

  it('全部低温 → hotRatio 0', () => {
    const s = tempSummary([40, 45, 49]);
    expect(s!.hotRatio).toBe(0);
    expect(s!.mean).toBe(44.666666666666664);
  });
});