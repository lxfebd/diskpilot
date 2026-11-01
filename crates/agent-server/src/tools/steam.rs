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

// SystemTime/UNIX_EPOCH 与 fmt_bytes 仅供 cfg(windows) 的 collect_steam_games
//（Linux 仅返回降级文案），随 Windows 实现一起编译。
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};

use crate::tools::fmt_bytes;

/// 卸载残留的逐行文本（纯格式化，跨平台可单测）。
///
/// 每处必须带上**完整路径**：只报「有 N 处残留」等于把能力卡在报告里——AI 拿不到
/// 路径就没法在用户确认后调 file_recycle 真的回收，用户也没法照着描述去找目录。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows 工具路径调用；单测全平台跑
fn residue_lines(residue: &[diskpilot_steam_inspector::SteamResidue], limit: usize) -> String {
    let mut out = String::new();
    for r in residue.iter().take(limit) {
        let kind = match r.kind {
            diskpilot_steam_inspector::SteamResidueKind::Install => "安装目录残留",
            diskpilot_steam_inspector::SteamResidueKind::Workshop => "创意工坊残留",
        };
        let what = match r.appid_hint {
            Some(id) => format!("{kind}（appid {id}）"),
            None => format!("{kind}（{}）", r.dir_name),
        };
        out.push_str(&format!(
            "- {} · {} · 路径：{}\n",
            what,
            fmt_bytes(r.size_bytes),
            r.path
        ));
    }
    // 与逐游戏列出同样的诚实口径：截断了就说截断了多少，别让模型以为列全了。
    if residue.len() > limit {
        out.push_str(&format!(
            "…（本库共 {} 处残留，只列出最大的 {} 处）\n",
            residue.len(),
            limit
        ));
    }
    out
}

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
    let mut total_residue: u64 = 0;

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

        // 卸载残留：有目录无 ACF——游戏列表里完全看不到的真实占用。
        if !lib.residue.is_empty() {
            out.push_str(&format!(
                "⚠ 卸载残留 {}（{} 处，不在游戏列表里）：\n",
                fmt_bytes(lib.residue_size_bytes),
                lib.residue.len()
            ));
            out.push_str(&residue_lines(&lib.residue, 10));
            total_residue = total_residue.saturating_add(lib.residue_size_bytes);
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
    if total_residue > 0 {
        out.push_str(&format!(
            "\n另有卸载残留 {}：目录还在但没有任何游戏记录，通常可直接删除（Steam 卸载没清干净的部分）。\n\
             上面每处都给了完整路径：用户明确要求清理某一处时，可用 file_recycle 把它移入回收站\
             （写操作，会先弹用户确认门，且只进回收站可还原）；未经用户要求不要主动回收。",
            fmt_bytes(total_residue)
        ));
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn collect_steam_games(_top_n: usize) -> Result<String, String> {
    Ok("当前平台不是 Windows，Steam 游戏库盘点不可用".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diskpilot_steam_inspector::{SteamResidue, SteamResidueKind};

    fn residue(
        kind: SteamResidueKind,
        dir: &str,
        path: &str,
        size: u64,
        appid: Option<u32>,
    ) -> SteamResidue {
        SteamResidue {
            kind,
            dir_name: dir.into(),
            path: path.into(),
            size_bytes: size,
            appid_hint: appid,
            last_modified_ts: 0,
        }
    }

    /// 残留行必须带完整路径：没路径 = AI 只能转述、无法在用户确认后真的回收。
    #[test]
    fn residue_lines_carry_path_kind_and_size() {
        let lines = residue_lines(
            &[
                residue(
                    SteamResidueKind::Install,
                    "Leftover Game",
                    "D:\\SteamLibrary\\steamapps\\common\\Leftover Game",
                    12 * 1024 * 1024 * 1024,
                    None,
                ),
                residue(
                    SteamResidueKind::Workshop,
                    "241100",
                    "E:\\SteamLibrary\\steamapps\\workshop\\content\\241100",
                    1024 * 1024,
                    Some(241100),
                ),
            ],
            10,
        );
        assert!(lines.contains("安装目录残留（Leftover Game）"), "{lines}");
        assert!(
            lines.contains("路径：D:\\SteamLibrary\\steamapps\\common\\Leftover Game"),
            "{lines}"
        );
        assert!(lines.contains("12.0 GB"), "{lines}");
        assert!(lines.contains("创意工坊残留（appid 241100）"), "{lines}");
        assert!(
            lines.contains("路径：E:\\SteamLibrary\\steamapps\\workshop\\content\\241100"),
            "{lines}"
        );
        assert!(lines.contains("1.0 MB"), "{lines}");
    }

    #[test]
    fn residue_lines_respect_the_limit() {
        let many: Vec<SteamResidue> = (0..15)
            .map(|i| {
                residue(
                    SteamResidueKind::Install,
                    &format!("dir_{i}"),
                    &format!("C:\\steam\\steamapps\\common\\dir_{i}"),
                    1024,
                    None,
                )
            })
            .collect();
        assert_eq!(
            residue_lines(&many, 10)
                .lines()
                .filter(|l| l.starts_with("- "))
                .count(),
            10
        );
        assert_eq!(residue_lines(&many, 100).lines().count(), 15);
        assert!(residue_lines(&[], 10).is_empty());
    }

    /// 截断必须说出来：否则模型会把「列出 10 处」当成「一共 10 处」。
    #[test]
    fn residue_lines_announce_truncation() {
        let many: Vec<SteamResidue> = (0..16)
            .map(|i| {
                residue(
                    SteamResidueKind::Install,
                    &format!("dir_{i}"),
                    &format!("C:\\steam\\steamapps\\common\\dir_{i}"),
                    1024,
                    None,
                )
            })
            .collect();
        let lines = residue_lines(&many, 10);
        assert!(
            lines.contains("…（本库共 16 处残留，只列出最大的 10 处）"),
            "{lines}"
        );
        // 没截断时不多嘴。
        assert!(!residue_lines(&many[..5], 10).contains('…'));
    }
}
