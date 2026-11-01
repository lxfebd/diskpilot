//! 磁盘枚举与体积估算命令（list_drives / volume_info / estimate_size）。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：盘符枚举、单盘空间查询（Win32
//! GetDiskFreeSpaceExW）、进度分母用的快速体积估算。无 AppState 依赖。

#[derive(serde::Serialize, Clone)]
pub(crate) struct VolumeInfo {
    /// 盘符，如 "C:\"。仅 list_drives 填充；volume_info 单查时不带（历史兼容）。
    #[serde(default)]
    pub(crate) path: String,
    pub(crate) total_bytes: u64,
    pub(crate) used_bytes: u64,
    pub(crate) free_bytes: u64,
    /// 映射盘（`subst D: J:\D`）的目标路径；普通盘为空。
    /// 映射盘没有自己的分区与空间——不说清楚，首页就是两块一模一样的盘。
    #[serde(default)]
    pub(crate) mapped_to: String,
}

/// `subst` 的一行输出 → (盘符, 目标路径)。本机实测格式：`D:\: => J:\D`。
/// 认不出的一律 None，不猜。
#[cfg_attr(not(windows), allow(dead_code))] // 仅 Windows subst 路径调用；单测全平台跑
pub(crate) fn parse_subst_line(line: &str) -> Option<(char, String)> {
    let (left, right) = line.split_once("=>")?;
    let letter = left.trim().trim_end_matches(['\\', ':']);
    let target = right.trim();
    let mut chars = letter.chars();
    let c = chars.next()?;
    if chars.next().is_some() || !c.is_ascii_alphabetic() || target.is_empty() {
        return None;
    }
    Some((c.to_ascii_uppercase(), target.to_string()))
}

/// 映射盘表。`Get-Partition` / `Get-Volume` 里没有它们，但 `GetLogicalDrives`
/// 会数成一个盘符——不识别就会把同一份空间当成两块盘。
#[cfg(windows)]
pub(crate) fn collect_substs() -> Vec<(char, String)> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(out) = std::process::Command::new("subst")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    // subst.exe 按控制台代码页输出：路径含中文时 UTF-8 解码会出替换符。
    // 这里只要把「这个盘符是谁的映射」说清楚，解码不出完整路径也照样能用。
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().filter_map(parse_subst_line).collect()
}

#[cfg(not(windows))]
pub(crate) fn collect_substs() -> Vec<(char, String)> {
    Vec::new()
}

/// 映射目标所在的卷根：`J:\D` → `J:\`；不是盘符路径（UNC 等）返回 None，不猜。
pub(crate) fn target_volume_root(target: &str) -> Option<String> {
    let b = target.as_bytes();
    if b.len() < 2 || !b[0].is_ascii_alphabetic() || b[1] != b':' {
        return None;
    }
    Some(format!("{}:\\", (b[0] as char).to_ascii_uppercase()))
}

/// 这个盘符映射到的**另一块本机卷的卷根**；不是映射盘、或目标不是盘符路径、
/// 或映射回自己，都返回 None。纯函数，可测。
pub(crate) fn mapped_target_local_root(letter: char, substs: &[(char, String)]) -> Option<String> {
    let (_, target) = substs
        .iter()
        .find(|(l, _)| l.eq_ignore_ascii_case(&letter))?;
    let troot = target_volume_root(target)?;
    if troot == format!("{}:\\", letter.to_ascii_uppercase()) {
        return None;
    }
    Some(troot)
}

/// 这个盘符是不是「映射到本机另一块可读卷」的映射盘。
///
/// 按 C..Z 逐盘求和的地方（周报、清理提醒）会把目标卷自己也算一遍：
/// 映射盘再算一次就把同一块盘的空间与可清量数两遍。目标卷不在本机（UNC 等）时，
/// 映射盘是碰到那份空间的唯一入口，要算它。
pub(crate) fn mapped_into_local_volume(letter: char, substs: &[(char, String)]) -> bool {
    match mapped_target_local_root(letter, substs) {
        Some(troot) => volume_info(troot).is_ok(),
        None => false,
    }
}

/// 枚举本机所有可用磁盘，返回盘符与空间信息。供"全盘扫描"首页直出，
/// 长辈不必再手动选文件夹。映射盘（subst）也列出来——用户确实在用那个盘符，
/// 但带上 `mapped_to` 说明它落在哪块真盘上。
#[tauri::command]
pub(crate) fn list_drives() -> Vec<VolumeInfo> {
    let substs = collect_substs();
    let mut out = Vec::new();
    for letter in b'C'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if let Ok(mut info) = volume_info(root.clone()) {
            info.path = root;
            if let Some((_, target)) = substs.iter().find(|(l, _)| *l == letter as char) {
                info.mapped_to = target.clone();
            }
            // used_bytes == 0 且经 GetDiskFreeSpaceExW 成功 -> 普通可读盘
            out.push(info);
        }
    }
    out
}

#[tauri::command]
pub(crate) fn volume_info(path: String) -> Result<VolumeInfo, String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut wide: Vec<u16> = std::ffi::OsStr::new(&path).encode_wide().collect();
        // Trim to drive root for the Win32 API.
        let root: Vec<u16> = if wide.len() >= 2 && wide[1] == b':' as u16 {
            vec![wide[0], wide[1], b'\\' as u16, 0]
        } else {
            wide.push(0);
            wide
        };
        let mut free_to_caller: u64 = 0;
        let mut total: u64 = 0;
        let mut total_free: u64 = 0;
        // SAFETY: calling documented Win32 API with valid wide-char buffers.
        let ok = unsafe {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetDiskFreeSpaceExW(
                    lpDirectoryName: *const u16,
                    lpFreeBytesAvailable: *mut u64,
                    lpTotalNumberOfBytes: *mut u64,
                    lpTotalNumberOfFreeBytes: *mut u64,
                ) -> i32;
            }
            GetDiskFreeSpaceExW(
                root.as_ptr(),
                &mut free_to_caller,
                &mut total,
                &mut total_free,
            )
        };
        if ok == 0 {
            return Err("GetDiskFreeSpaceExW failed".into());
        }
        Ok(VolumeInfo {
            path,
            total_bytes: total,
            used_bytes: total.saturating_sub(total_free),
            free_bytes: total_free,
            // 单查不判映射盘（subst 表只在 list_drives 里查一次）；list_drives 会填上。
            mapped_to: String::new(),
        })
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("volume_info only implemented on Windows".into())
    }
}

/// Fast size-only walk used to seed the progress-bar denominator before the
/// real scan starts. Single jwalk pass, sums file sizes, no other state.
#[tauri::command]
pub(crate) async fn estimate_size(path: String) -> Result<u64, String> {
    let p = std::path::PathBuf::from(&path);
    tokio::task::spawn_blocking(move || -> u64 {
        let mut total: u64 = 0;
        for entry in diskpilot_scanner::diskpilot_walker(&p)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_file() {
                if let Ok(md) = entry.metadata() {
                    total = total.saturating_add(md.len());
                }
            }
        }
        total
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subst_line_parses_real_output_shape() {
        // 本机实测：`D:\: => J:\D`
        assert_eq!(
            parse_subst_line("D:\\: => J:\\D"),
            Some(('D', "J:\\D".to_string()))
        );
        assert_eq!(
            parse_subst_line("x:\\: => F:\\x"),
            Some(('X', "F:\\x".to_string()))
        );
        // 认不出的不猜。
        assert_eq!(parse_subst_line(""), None);
        assert_eq!(parse_subst_line("No Subst drive entries."), None);
        assert_eq!(parse_subst_line("D:\\: => "), None);
        assert_eq!(parse_subst_line("DD:\\: => J:\\D"), None);
    }

    #[test]
    fn target_volume_root_only_accepts_drive_paths() {
        assert_eq!(target_volume_root("J:\\D"), Some("J:\\".to_string()));
        assert_eq!(target_volume_root("j:\\d"), Some("J:\\".to_string()));
        assert_eq!(target_volume_root("J:"), Some("J:\\".to_string()));
        assert_eq!(target_volume_root("J:\\"), Some("J:\\".to_string()));
        // UNC / 相对路径：认不出就 None，不猜它在哪块盘上。
        assert_eq!(target_volume_root("\\\\nas\\share"), None);
        assert_eq!(target_volume_root(""), None);
    }

    #[test]
    fn mapped_target_local_root_skips_self_and_non_drive() {
        let substs = vec![
            ('D', "J:\\D".to_string()),
            ('X', "\\\\nas\\share".to_string()),
        ];
        assert_eq!(
            mapped_target_local_root('D', &substs),
            Some("J:\\".to_string())
        );
        // 大小写不敏感（subst 输出是大写，调用方可能传小写）。
        assert_eq!(
            mapped_target_local_root('d', &substs),
            Some("J:\\".to_string())
        );
        // 映射到 UNC：没有本机目标卷，映射盘是唯一的入口，不能跳过它。
        assert_eq!(mapped_target_local_root('X', &substs), None);
        // 不是映射盘。
        assert_eq!(mapped_target_local_root('C', &substs), None);
        // 映射回自己的根：跳过就会把这块盘算成零，不跳。
        let self_map = vec![('D', "D:\\x".to_string())];
        assert_eq!(mapped_target_local_root('D', &self_map), None);
    }
}
