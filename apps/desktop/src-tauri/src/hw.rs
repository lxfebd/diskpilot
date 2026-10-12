//! 硬件检测/受控压测/报告（Tauri 命令层）。
//!
//! 2026-10-12 拆分：采集内核 → `hw_core`，磁盘 → `hw_disk`，压测 → `hw_test`，
//! 报告/电源 → `hw_report_cmd`，风扇 → `hw_fan`。本文件保留单测。

#[path = "hw_core.rs"]
pub(crate) mod hw_core;
#[path = "hw_disk.rs"]
pub(crate) mod hw_disk;
#[path = "hw_fan.rs"]
pub(crate) mod hw_fan;
#[path = "hw_report_cmd.rs"]
pub(crate) mod hw_report_cmd;
#[path = "hw_test.rs"]
pub(crate) mod hw_test;

pub(crate) use hw_core::ps_capture;
pub(crate) use hw_fan::fan_curve_advice;

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[cfg(test)]
    use crate::hw::hw_core::pick_gpu_live;
    #[cfg(test)]
    use crate::hw::hw_fan::speed_pct_for;

    // GPU 实时双通道决策（pick_gpu_live）：NVIDIA 优先，AMD ADL 兜底。
    // 抽纯函数规避真 spawn nvidia-smi / PowerShell——这儿 mock 两通道结果锁矩阵。
    #[test]
    fn pick_gpu_live_nvidia_wins_when_both_available() {
        let nv = json!({ "name": "NVIDIA GeForce RTX 5090 D v2", "temperature_c": 38 });
        let adl = json!({ "name": "AMD 显卡（ADL 适配器 1）", "temperature_c": 58 });
        let got =
            pick_gpu_live(Ok(Some(nv.clone())), Ok(Some(adl.clone()))).expect("应取到 NVIDIA");
        assert_eq!(got["name"], "NVIDIA GeForce RTX 5090 D v2");
    }

    #[test]
    fn pick_gpu_live_amd_when_nvidia_absent() {
        // 无 N 卡（nvidia-smi 返回空行 → Ok(None)）→ 退 AMD ADL 兜底
        let adl = json!({ "name": "AMD 显卡（ADL 适配器 1）", "temperature_c": 58 });
        let got = pick_gpu_live(Ok(None), Ok(Some(adl.clone()))).expect("应取到 AMD ADL");
        assert!(got["name"].as_str().unwrap().contains("AMD"));
    }

    #[test]
    fn pick_gpu_live_amd_when_nvidia_spawn_fails() {
        // nvidia-smi 不存在 → Err → AMD 兜底照常生效
        let adl = json!({ "name": "AMD 显卡（ADL 适配器 1）", "temperature_c": 58 });
        let got = pick_gpu_live(
            Err("启动 nvidia-smi 失败：NotFound".into()),
            Ok(Some(adl.clone())),
        )
        .expect("应取到 AMD ADL");
        assert_eq!(got["temperature_c"], 58);
    }

    #[test]
    fn pick_gpu_live_none_when_both_channels_dead() {
        assert!(pick_gpu_live(Ok(None), Ok(None)).is_none());
        assert!(pick_gpu_live(Err("nvidia-smi 失败".into()), Ok(None)).is_none());
        assert!(pick_gpu_live(Ok(None), Err("ADL 不可用".into())).is_none());
        assert!(pick_gpu_live(Err("a".into()), Err("b".into())).is_none());
    }

    #[test]
    fn speed_pct_for_levels_and_custom() {
        assert_eq!(speed_pct_for(Some("quiet"), None), Ok(35));
        assert_eq!(speed_pct_for(Some("balanced"), None), Ok(55));
        assert_eq!(speed_pct_for(Some("balanced_plus"), None), Ok(70));
        assert_eq!(speed_pct_for(Some("high"), None), Ok(85));
        assert_eq!(speed_pct_for(Some("urgent"), None), Ok(100));
        // 默认（无档位）走 balanced。
        assert_eq!(speed_pct_for(None, None), Ok(55));
        // custom：锚点 fan_pct 取均值。
        let anchors = json!([
            { "load_pct": 20, "fan_pct": 30 },
            { "load_pct": 60, "fan_pct": 70 },
            { "load_pct": 100, "fan_pct": 90 },
        ]);
        let arr = anchors.as_array().unwrap();
        assert_eq!(speed_pct_for(Some("custom"), Some(arr)), Ok(63)); // (30+70+90)/3
                                                                      // custom 空锚点报错。
        assert!(speed_pct_for(Some("custom"), Some(&[])).is_err());
        // 未知档位报错。
        assert!(speed_pct_for(Some("turbo"), None).is_err());
    }
}
