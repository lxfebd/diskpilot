//! Steam 游戏库只读盘点（复用 `diskpilot-steam-inspector`）。
//!
//! **纯工具文件**：不接路由，由 `tools/mod.rs` 薄壳对接 MCP tool。
//! **全部只读**：读注册表定位 Steam 根 + 读 ACF 清单 + read_dir 数创意工坊目录，
//! 不写任何文件。
//!
//! ## 本文件导出函数清单
//!
//! 1. `pub fn collect_steam_games(top_n: usize) -> Result<String, String>`
//!    盘点本机 Steam 游戏库：库根/每库游戏数/总占用 + 各游戏
//!    （appid/名称/安装目录/大小/上次游玩/是否幽灵/是否建议清理 + 原因）。
//!    建议逻辑来自 steam-inspector 内置规则（≥30GB 且 ≥6 个月未玩 /
//!    ≥50GB 且 ≥3 个月未玩；Steam 本体/SteamVR 等系统组件绝不建议）。
//!    找不到 Steam / 没有任何游戏库时返回中文兜底（不是 Err）。

use std::time::{SystemTime, UNIX_EPOCH};

/// 盘点本机 Steam 游戏库（只读）。
///
/// `top_n`：每库最多列多少个游戏（按大小降序截断），默认 20、钳 1..=100。
/// 全库游戏数/总占用始终统计完整（截断只影响「逐游戏列出」）。
#[cfg(windows)]
pub fn collect_steam_games(top_n: usize) -> Result<String, String> {
    let n = if top_n == 0 { 20 } else { top_n.clamp(1, 100) };
    // 用系统时钟对齐「上次游玩」建议逻辑（与 inspect() 内部一致）。
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let inv = match diskpilot_steam_inspector::inspect() {
        Ok(inv) => inv,
        Err(e) => return Ok(format!("读取 Steam 游戏库失败：{e}")),
    };

    let Some(_root) = &inv.steam_root else {
        return Ok(format!(
            "未检测到 Steam 安装（按候选路径逐个找过：{}）。\n\
             如果 Steam 装在非常规位置，可安装后在 Steam 设置里确认库目录再试。",
            if inv.candidates_checked.is_empty() {
                "无候选".to_string()
            } else {
                inv.candidates_checked.join("；")
            }
        ));
    };

    if inv.libraries.is_empty() {
        return Ok("检测到 Steam，但没有任何游戏库（steamapps 目录为空或不可读）。".into());
    }

    let mut out = String::new();
    let mut total_games = 0usize;
    let mut total_size: u64 = 0;

    for lib in &inv.libraries {
        total_games += lib.games.len();
        total_size = total_size.saturating_add(lib.total_size_bytes);
        out.push_str(&format!(
            "\n## 库：{}（{} 个游戏，共 {}）\n",
            lib.root,
            lib.games.len(),
            fmt_bytes(lib.total_size_bytes)
        ));

        // 游戏按大小降序列出（截断到 n）。
        let mut games: Vec<&diskpilot_steam_inspector::SteamGame> = lib.games.iter().collect();
        games.sort_by_key(|g| std::cmp::Reverse(g.size_bytes));
        let truncated = games.len() > n;
        for g in games.iter().take(n) {
            let name = if g.name_en.is_empty() {
                g.install_dir_name.clone()
            } else {
                g.name_en.clone()
            };
            let mut line = format!(
                "- {}（appid {}）· {}",
                name,
                g.appid,
                fmt_bytes(g.size_bytes)
            );
            if g.is_ghost {
                line.push_str(" · ⚠ 幽灵安装（ACF 存在但目录缺失）");
            }
            match g.last_played_ts {
                Some(ts) if ts > 0 => {
                    let days = (now_secs.saturating_sub(ts)) / 86400;
                    line.push_str(&format!(" · 上次游玩约 {days} 天前"));
                }
                _ => line.push_str(" · 从未启动"),
            }
            if g.bytes_to_download > 0 {
                line.push_str(&format!(
                    " · 待更新 {} / 已下载 {}",
                    fmt_bytes(g.bytes_to_download),
                    fmt_bytes(g.bytes_downloaded)
                ));
            }
            if let Some(reason) = &g.recommendation_reason {
                line.push_str(&format!(" · 💡 建议清理：{reason}"));
            }
            out.push_str(&line);
            out.push('\n');
        }
        if truncated {
            out.push_str(&format!(
                "…（本库共 {} 个游戏，只列出最大的 {n} 个）\n",
                games.len()
            ));
        }
    }

    out.push_str(&format!(
        "\n合计 {} 个游戏，共占用 {}。\n\
         建议清理的判断标准：≥30GB 且 ≥6 个月未启动，或 ≥50GB 且 ≥3 个月未启动；\
         Steam 客户端/SteamVR 等系统组件永不建议。\
         幽灵安装（ACF 在但目录缺失）也标为建议清理。",
        total_games,
        fmt_bytes(total_size)
    ));
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_steam_games(_top_n: usize) -> Result<String, String> {
    Ok("当前平台不是 Windows，Steam 游戏库盘点不可用".into())
}

/// 字节数人类可读（同 `mod.rs::fmt_bytes` 语义；rustc 1.98 用 `{:.*}`）。
fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{:.*} {}", 1, v, UNITS[u])
    }
}
