//! 基准与压测（bench.rs）：复刻 AIDA64 的 Cache & Memory / CPU / FPU / Disk
//! 基准 + System Stability Test，全部做成 AI 可操作工具。
//!
//! - `bench_cpu`（L0 只读）：多线程整数/浮点基准。纯计算，不写盘、不改系统。
//! - `bench_memory`（L0 只读）：读/写/复制带宽 + 延迟（指针追逐）。
//! - `bench_disk`（L0 只读为主）：读基准 + 写基准（写用系统临时目录一次性文件，
//!   默认 dry_run 只出计划；执行后立即删除，不碰用户数据）。
//! - `stress_test`（L2 写，confirmed 门）：CPU 多线程压测，期间实时采样 CPU 温度，
//!   超过阈值自动熔断停机。纯计算不写盘，默认短时（5s）让 AI 演示，超时上限 600s。
//!
//! 安全设计：
//! - 全部基准只消耗 CPU/内存/磁盘临时文件，绝不写用户数据目录；
//! - stress_test 超温熔断（默认 95℃，可配）+ 定长上限 + L2 确认门；
//! - bench_disk 写基准的临时文件创建即删，用 `dry_run` 语义让 AI 先出计划；
//! - 与 AIDA64 基准对齐口径：ops/s（Queen/Julia/Primes）、MB/s（内存/磁盘）、
//!   纳秒（内存延迟）。
//!
//! 2026-10-12 拆分：主函数域 → `bench_core`，内存 pattern 辅助 → `bench_pattern`，
//! 内部工具（停止/运行标志文件、并行基准、温度探测等）→ `bench_util`。

mod bench_core;
mod bench_pattern;
mod bench_stress;
mod bench_util;

pub use bench_core::{bench_cpu, bench_disk, bench_memory};
pub use bench_stress::{mem_test, stress_test, stress_test_gpu};
pub use bench_util::stress_cancel;

// 供回归测试引用（pattern 校验与线程数选择辅助）。
#[cfg(test)]
pub(crate) use bench_pattern::{fill_pattern, verify_pattern};
#[cfg(test)]
pub(crate) use bench_util::pick_threads;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_cpu_short_run() {
        let r = bench_cpu(Some(1), Some(2)).unwrap();
        assert!(r.contains("CPU 基准结果"));
        assert!(r.contains("ops/s"));
    }

    #[test]
    fn bench_memory_short_run() {
        let r = bench_memory(Some(1)).unwrap();
        assert!(r.contains("内存基准结果"));
        assert!(r.contains("MB/s"));
    }

    #[test]
    fn bench_disk_dry_run_plan() {
        let r = bench_disk(None, Some(64), Some(true), None).unwrap();
        assert!(r.contains("dry-run"));
        assert!(r.contains("临时文件"));
    }

    #[test]
    fn bench_disk_real_run_rejects_system_dir() {
        // dry_run=false 真跑前必须过写守卫：系统目录（C:\Windows）直接拒绝，
        // 防止 AI 把磁盘基准目标指到系统目录写临时文件。
        let sys = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let r = bench_disk(Some(sys), Some(16), Some(false), None);
        let e = r.expect_err("系统目录必须被写守卫拒绝");
        assert!(
            e.contains("安全守卫") || e.contains("系统目录不可操作"),
            "拒绝原因应含守卫文案，got: {e}"
        );
    }

    #[test]
    fn bench_disk_real_run_rejects_drive_root() {
        // 盘根（C:\）也应拒绝
        let root = if cfg!(windows) { "C:\\" } else { "/" };
        let r = bench_disk(Some(root.to_string()), Some(16), Some(false), None);
        let e = r.expect_err("盘根必须被写守卫拒绝");
        assert!(
            e.contains("安全守卫") || e.contains("盘根目录不可操作"),
            "got: {e}"
        );
    }

    #[test]
    fn stress_test_short_run() {
        let r = stress_test(Some(1), Some(60.0), Some(2), None).unwrap();
        assert!(r.contains("稳定性压测结果"));
        assert!(r.contains("峰值 CPU 温度"));
        // 温度曲线行：有温度读时输出「温度曲线：」行，格式 秒数s=温度°C
        if r.contains("温度曲线：") {
            let line = r.lines().find(|l| l.starts_with("温度曲线：")).unwrap();
            assert!(line.contains("°C"), "曲线行应带 °C 单位: {line}");
            assert!(line.contains("s="), "曲线行应含秒数: {line}");
        }
    }

    #[test]
    fn mem_test_short_run() {
        // 64MB 最小尺寸，1 轮：应通过（无位翻转），校验 MB 计数出现
        let r = mem_test(Some(64), Some(1), None).unwrap();
        assert!(r.contains("内存稳定性测试结果"));
        assert!(r.contains("校验通过") || r.contains("校验失败"));
        assert!(r.contains("MB"));
    }

    #[test]
    fn mem_test_patterns_self_consistent() {
        // fill+verify 自洽：5 种 pattern 都应在无干扰时 0 错误
        let n = 1 << 20; // 1MB
        for kind in 0..5u8 {
            let mut buf = vec![0u8; n];
            fill_pattern(&mut buf, kind);
            let (errs, first) = verify_pattern(&buf, kind);
            assert_eq!(errs, 0, "pattern kind={kind} 应自洽");
            assert!(first.is_none());
        }
    }

    #[test]
    fn stress_test_gpu_short_run() {
        // 1s 守门短跑：无 N 卡/无 nvidia-smi 时如实降级，不 panic
        let r = stress_test_gpu(Some(1), Some(90.0), None).unwrap();
        assert!(r.contains("GPU 稳定性压测结果"));
        assert!(r.contains("显卡"));
    }

    #[test]
    fn pick_threads_clamped() {
        assert_eq!(pick_threads(Some(0)), 1);
        let available = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        assert!(pick_threads(Some(10000)) <= available.saturating_mul(2).max(1));
    }
}
