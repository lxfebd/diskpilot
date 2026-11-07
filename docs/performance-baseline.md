# 扫描性能基线（R511 建立）

压测入口（跨平台，bench feature 下构建）：

```bash
cargo run -p diskpilot-scanner --features bench --release -- walk <目录> [重复次数]
```

输出每次 run 的 wall 时间、files/s、GB/s、phase 分解（walk / build_tree / total），最后一行 BEST/AVG 汇总。重复默认 3 次取最佳（冷/热缓存差异）。

## 本机基线（2026-11-08，24 核，Windows 11）

| 目标 | 文件数 | 字节 | BEST wall | 吞吐 | walk 占比 | 介质 |
|---|---|---|---|---|---|---|
| C:\Windows | 73,216 | 19.55 GB | 0.86 s | 84.7k files/s · 22.6 GB/s | 97% (1088ms walk / 25ms tree) | SSD |
| C:\Program Files | 75,630 | 31.89 GB | 0.83 s | 90.8k files/s · 38.3 GB/s | 96% | SSD |
| J:\xiangm_transfer | 903,957 | 70.80 GB | 13.91 s | 65.0k files/s · 5.1 GB/s | 97% (13.2s walk / 0.46s tree) | HDD（WD20EZRX 机械盘）|

## 结论

- **瓶颈在 walk（IO），不在树构建**：build_tree 恒定 < 4%（R 之前已消除二次 read_dir，子目录名在 walk 时一次性收集）。
- SSD 上 22–38 GB/s、HDD 上 5 GB/s ≈ 介质物理极限，软件层无明显优化空间。
- `parallelism = available_parallelism`（独立 rayon 池，防多盘并行扫描互相饿死）策略有效，90 万文件扫描无全空/卡死。
- 回归对比：改动 walker 后用同一命令跑基线，BEST wall 不应回退超过 10%。

## 已知限制

- MFT 快路径（`bench C`，需管理员）只在 Windows 有效；walk 基线跨平台。
- bench feature 曾有编译断裂（mft_parse 未引入 fixup_all/parse_record），R511 修复。
