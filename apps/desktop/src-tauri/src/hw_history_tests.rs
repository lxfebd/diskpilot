//! hw_history.rs 单元测试（从 hw_history.rs 拆分，2026-10-12）。
//! 时间戳归档命名 / 字段差值提取 / 30 份上限修剪 / 路径穿越拒绝。

use super::*;

fn tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hw-hist-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn mk_snapshot(dir: &Path, file: &str, at: i64, power_on: f64, temp: f64, wear: f64, device: &str) {
    let payload = serde_json::json!({
        "info": {
            "disk": [ { "device": device, "Model": device, "Size": 100 } ],
            "system": [ { "Model": "TestBox" } ],
            "os": [ { "Manufacturer": "Test", "Caption": "Windows Test" } ]
        },
        "health": { "disk": [ { "device": device, "power_on_hours": power_on, "temperature": temp, "wear": wear } ] },
        "note": "",
        "at": at,
        "machine": "TestBox · Windows Test",
    });
    std::fs::write(
        dir.join(file),
        serde_json::to_string_pretty(&payload).unwrap(),
    )
    .unwrap();
}

#[test]
fn archive_names_are_timestamped_and_listed_desc() {
    let dir = tmp_dir("list");
    mk_snapshot(
        &dir,
        "hw-20260910-100000.json",
        1_700_000_000,
        100.0,
        40.0,
        5.0,
        "Disk A",
    );
    mk_snapshot(
        &dir,
        "hw-20260911-100000.json",
        1_700_086_400,
        130.0,
        45.0,
        7.0,
        "Disk A",
    );
    let metas = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter_map(|p| parse_meta(&p))
        .collect::<Vec<_>>();
    assert_eq!(metas.len(), 2);
    // get_hw_history 的排序逻辑：按 at 倒序（最新的在前）。read_dir 本身
    // 顺序任意，这里直接验证排序后结果。
    let mut sorted = metas.clone();
    sorted.sort_by_key(|a| std::cmp::Reverse(a.at));
    let ats = sorted.iter().map(|m| m.at).collect::<Vec<_>>();
    assert_eq!(ats, vec![1_700_086_400, 1_700_000_000]);
    assert_eq!(sorted[0].machine, "TestBox · Windows Test");
    assert_eq!(sorted[0].disk_count, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compare_extracts_field_deltas() {
    let dir = tmp_dir("cmp");
    // 两次归档：通电 100→130h、温度 40→45℃、磨损 5→7%
    mk_snapshot(
        &dir,
        "hw-old.json",
        1_700_000_000,
        100.0,
        40.0,
        5.0,
        "Disk A",
    );
    mk_snapshot(
        &dir,
        "hw-new.json",
        1_700_086_400,
        130.0,
        45.0,
        7.0,
        "Disk A",
    );
    // 直接调用底层逻辑：读两份 → 对齐 → 算差值
    let va: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("hw-old.json")).unwrap()).unwrap();
    let vb: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("hw-new.json")).unwrap()).unwrap();
    let disks_of = |v: &serde_json::Value| -> Vec<serde_json::Value> {
        v.get("health")
            .and_then(|h| h.get("disk"))
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default()
    };
    let da = disks_of(&va);
    let db = disks_of(&vb);
    let new_d = &db[0];
    let old_d = da
        .iter()
        .find(|d| v_str(d, "device").eq_ignore_ascii_case(&v_str(new_d, "device")))
        .unwrap();
    let f_new = disk_fields(new_d);
    let f_old = disk_fields(old_d);
    assert!(f_new.len() >= 3);
    let hours = f_new
        .iter()
        .find(|(n, _)| n.contains("通电"))
        .unwrap()
        .1
        .unwrap();
    assert_eq!(hours, 130.0);
    let temp = f_new
        .iter()
        .find(|(n, _)| n.contains("温度"))
        .unwrap()
        .1
        .unwrap();
    assert_eq!(temp, 45.0);
    let wear = f_new
        .iter()
        .find(|(n, _)| n.contains("磨损"))
        .unwrap()
        .1
        .unwrap();
    assert_eq!(wear, 7.0);
    // 旧值对比正确
    let old_hours = f_old
        .iter()
        .find(|(n, _)| n.contains("通电"))
        .unwrap()
        .1
        .unwrap();
    assert_eq!(old_hours, 100.0);
    assert_eq!(fmt_field("通电时间(小时)", hours - old_hours), "+30 小时");
    assert_eq!(fmt_field("温度(℃)", 5.0), "+5 ℃");
    assert_eq!(fmt_field("磨损度(%)", 2.0), "+2.0 %");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prune_keeps_latest_30() {
    let dir = tmp_dir("prune");
    for i in 0..35 {
        let file = format!("hw-20260910-{:06}.json", i);
        mk_snapshot(&dir, &file, 1_700_000_000 + i, 10.0, 30.0, 1.0, "Disk A");
    }
    prune(&dir);
    let count = std::fs::read_dir(&dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .map(|x| {
                    x.file_name()
                        .to_str()
                        .map(|s| s.starts_with("hw-"))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        })
        .count();
    assert_eq!(count, MAX_SNAPSHOTS, "裁剪后应保留最近 30 份");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v_num_takes_first_token_not_concatenated_digits() {
    // CDI 风格字符串值：只取第一个数字 token。
    let v = serde_json::json!({
        "通电时间": "1,234 小时（约 51 天 / 0 年）",
        "温度": "45 ℃",
        "磨损": "90 %（2021 年上报）",
        "power_on_hours": 130,
        "bad": "无",
    });
    // 旧实现 filter 拼接会得到 12345100 / 902021 这类天文数字差值。
    assert_eq!(v_num(&v, "通电时间"), Some(1234.0));
    assert_eq!(v_num(&v, "温度"), Some(45.0));
    assert_eq!(v_num(&v, "磨损"), Some(90.0));
    assert_eq!(v_num(&v, "power_on_hours"), Some(130.0));
    assert_eq!(v_num(&v, "bad"), None);
}

#[test]
fn parse_meta_falls_back_to_filename_datetime() {
    let dir = tmp_dir("meta-fallback");
    // 手改/损坏的归档：JSON 里没有 "at"，只能靠文件名 hw-YYYYMMDD-HHMMSS.json。
    std::fs::write(
        dir.join("hw-20260910-153000.json"),
        r#"{ "info": {}, "note": "", "machine": "X" }"#,
    )
    .unwrap();
    let m = parse_meta(&dir.join("hw-20260910-153000.json")).unwrap();
    let expect = chrono::NaiveDateTime::parse_from_str("20260910-153000", "%Y%m%d-%H%M%S")
        .unwrap()
        .and_utc()
        .timestamp() as u64;
    assert_eq!(m.at, expect);
    // 旧实现「把文件名数字拼起来」会得到 20260910153000 秒（公元 640 万年），
    // 按 at 倒序排序时这份快照会排到最前、UI 显示荒谬日期。
    assert!(m.at < 4_000_000_000, "at 应是合理 Unix 秒，got {}", m.at);

    // 文件名不是时间格式：回退 0（排到最后），不产生垃圾时间戳。
    std::fs::write(dir.join("hw-manual.json"), r#"{ "machine": "X" }"#).unwrap();
    assert_eq!(parse_meta(&dir.join("hw-manual.json")).unwrap().at, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn path_traversal_rejected() {
    // read_snapshot 的文件名白名单：只允许 hw-*.json 且不含 ..
    assert!(!safe_snapshot_file("..\\evil.json"));
    assert!(!safe_snapshot_file("hw-evil.txt"));
    assert!(safe_snapshot_file("hw-20260910-153000.json"));
    assert!(safe_snapshot_file("hw-20260910-153000.json"));
    // 组合场景：白名单本身也拒绝带 .. 的合法前缀
    assert!(!safe_snapshot_file("hw-..\\x.json"));
}
