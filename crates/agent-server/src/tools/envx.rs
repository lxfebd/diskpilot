//! 系统环境查询工具：回收站状态 / 环境变量。
//!
//! **纯工具文件**：不接路由，由 `tools/mod.rs` 薄壳对接 MCP tool。
//! **全部只读**：不写注册表、不改文件、不动回收站内容；只读状态与变量值。

/// 查询本机回收站状态：逐卷统计文件数与总大小（Windows 只读 API）。
/// 返回每卷一行 + 合计；非 Windows 或读取失败时返回错误说明。
#[cfg(windows)]
pub fn collect_recycle_bin_stats() -> Result<String, String> {
    use windows_sys::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};

    // 逐盘符查询：A-Z 中存在的卷（QueryDosDevice 探测或直接试 SHQuery）。
    let mut lines: Vec<String> = Vec::new();
    let mut total_files: u64 = 0;
    let mut total_bytes: u64 = 0;

    for letter in b'A'..=b'Z' {
        let mut root: [u16; 4] = [0; 4];
        root[0] = letter as u16;
        root[1] = b':' as u16;
        root[2] = b'\\' as u16;
        root[3] = 0;
        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            i64Size: 0,
            i64NumItems: 0,
        };
        // 该盘没有回收站（不可读/不存在）时跳过，不报错。
        let hr = unsafe { SHQueryRecycleBinW(root.as_ptr(), &mut info) };
        if hr != 0 {
            continue;
        }
        let drive = String::from_utf16_lossy(&root[0..2]);
        let items = info.i64NumItems;
        let bytes = info.i64Size.max(0) as u64;
        if items > 0 || bytes > 0 {
            lines.push(format!(
                "{}：{} 项，共 {}",
                drive,
                items,
                super::fmt_bytes(bytes)
            ));
            total_files += items.max(0) as u64;
            total_bytes += bytes;
        }
    }

    if lines.is_empty() {
        return Ok("回收站为空（各卷都没有可回收项目）".into());
    }
    lines.push(format!(
        "合计：{} 项，共 {}",
        total_files,
        super::fmt_bytes(total_bytes)
    ));
    Ok(lines.join("\n"))
}

/// 非 Windows 平台降级提示。
#[cfg(not(windows))]
pub fn collect_recycle_bin_stats() -> Result<String, String> {
    Ok("回收站状态查询仅支持 Windows。".into())
}

/// 列出指定环境变量（或全部）的值。name 为空时返回常用子集
/// （PATH / TEMP / USERPROFILE / APPDATA / LOCALAPPDATA / PROGRAMFILES 等），
/// 避免把几百个变量全倒出来。
pub fn collect_env_vars(name: Option<String>) -> Result<String, String> {
    use std::env;
    let key = name
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty());
    match key {
        Some(k) => match env::var(&k) {
            Ok(v) => Ok(format!("{k}={v}")),
            Err(e) => Err(format!("环境变量 {k} 不存在：{e:?}")),
        },
        None => {
            const COMMON: [&str; 8] = [
                "PATH",
                "TEMP",
                "TMP",
                "USERPROFILE",
                "APPDATA",
                "LOCALAPPDATA",
                "PROGRAMFILES",
                "PROGRAMFILES(X86)",
            ];
            let mut lines: Vec<String> = Vec::new();
            for c in COMMON {
                if let Ok(v) = env::var(c) {
                    lines.push(format!("{c}={v}"));
                }
            }
            if lines.is_empty() {
                return Err("未读取到任何常用环境变量。".into());
            }
            Ok(lines.join("\n"))
        }
    }
}
