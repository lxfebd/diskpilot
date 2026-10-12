//! 权限快照 + scaffold 命令（sync_perms / scaffold 安装卸载与装载）。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：受控权限白名单与前端权限快照同步，
//! 社区 scaffold 的解析/红线校验/安装/卸载/来源查询，以及全部 scaffold
//! 装载管线（load_all_scaffolds + 内嵌打包的 EMBEDDED_SCAFFOLDS）。

use std::collections::HashMap;
use std::path::PathBuf;

use diskpilot_scaffold::{parse_toml, Risk, Scaffold};
use include_dir::{include_dir, Dir};
use tauri::{AppHandle, Manager, State};

use crate::AppState;

// Compile-time embed of repo-root scaffolds/. Used as the lowest-priority
// fallback in load_all_scaffolds so a portable raw exe (no resource_dir,
// arbitrary cwd) still ships with all packaged scaffolds.
static EMBEDDED_SCAFFOLDS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../../scaffolds");

/// 与前端 permissions.ts PERMS 表对齐的受控权限 id 白名单（L3 永不在列）。
/// localStorage 被篡改时前端仍只可能同步这些 id，未知字符串与 L3 一律丢弃。
const CONTROLLED_PERMS: &[&str] = &[
    "hw.read",
    "hw.disk_health",
    "hw.report",
    "hw.stress",
    "hw.export",
    "cli.run",
    "power.plan",
    "cleanup.execute",
    "plugin.manage",
    "scaffold.manage",
    "startup.manage",
    "fan.adjust",
    "fan.control",
    "driver.check",
    "mcp.manage",
    "sys.control",
    "file.recycle",
    "app.uninstall",
];

pub(crate) fn is_controlled_perm(id: &str) -> bool {
    CONTROLLED_PERMS.contains(&id)
}

/// 前端把 AI 权限快照同步到后端（权限中心改动/应用启动时调用）。
/// 后端写命令（execute_ai_plan / toolbelt_run / plugin_*）据此做纵深校验：
/// 即使被绕过前端直接 invoke，未授权的 L2 写操作也会被后端拒绝。
#[tauri::command]
pub(crate) fn sync_perms(state: State<'_, AppState>, enabled: Vec<String>) {
    let mut grants = state.perm_grants.lock().unwrap();
    grants.clear();
    grants.extend(enabled.into_iter().filter(|s| is_controlled_perm(s)));
}

#[tauri::command]
pub(crate) fn list_scaffolds(state: State<'_, AppState>) -> Vec<Scaffold> {
    state.scaffolds.lock().unwrap().clone()
}

// 曾经这里有个 `detect_scaffold(path)` 命令：前端零调用，且扫描期
// `scan/tag.rs` 已给每个节点打上 scaffold_id，命令是重复入口——删掉，
// 少一个「注册了但没人用」的假功能面。

/// scaffold id 合法性校验（防路径穿越：id 会拼进文件名）。
/// 只允许小写字母/数字/连字符，禁止空串。
pub(crate) fn is_valid_scaffold_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 社区 scaffold 一键安装：接收一份 toml 文本，校验通过后写入
/// `app_data_dir/scaffolds/{id}.toml` 并热重载，前端立即可用。
/// 只写文件、不执行任何清理动作；与内嵌/资源目录里的同名脚本不冲突
/// （用户目录优先级最高，覆盖同名内嵌脚本——见 load_all_scaffolds 注释）。
#[tauri::command]
pub(crate) fn install_scaffold(
    app: AppHandle,
    state: State<'_, AppState>,
    toml: String,
) -> Result<String, String> {
    install_scaffold_inner(&app, &state, toml)
}

/// Winapp2.ini 规则导入预览（情报取长补短：借 BleachBit 社区规则生态）。
/// 把社区 winapp2.ini 文本解析成可清理 scaffold 列表，**只预览不写盘**——
/// 用户看完（哪些可装、哪些被红线拒绝）再逐条走 install_scaffold 入库。
/// 纯解析，不触碰任何系统状态，安全铁律通过。
#[tauri::command]
pub(crate) fn parse_winapp2(content: String) -> Result<Vec<ParseWinapp2Item>, String> {
    let entries = diskpilot_scaffold::winapp2::parse_winapp2(&content);
    let scaffolds = diskpilot_scaffold::winapp2::winapp2_to_scaffolds(&entries);
    if scaffolds.is_empty() {
        return Err(
            "未能从该 Winapp2.ini 解析出任何文件清理规则（可能只含注册表项，或内容为空）。".into(),
        );
    }
    Ok(scaffolds
        .into_iter()
        .map(|sc| {
            let red_line = scaffold_red_line_reject(&sc);
            // 序列化成 toml 文本：UI 勾选后直接走 install_scaffold 入库（同一
            // 条解析→入库管线，无二次转换漂移；未勾选的不产生任何 syscall）。
            let toml_text = toml::to_string(&sc).unwrap_or_default();
            ParseWinapp2Item {
                id: sc.id,
                name: sc.name,
                risk: sc.risk,
                scope_count: sc.scopes.len(),
                scopes: sc
                    .scopes
                    .iter()
                    .map(|s| format!("{} → {}", s.label, s.glob))
                    .collect(),
                disclaimer: sc.disclaimer,
                toml: if toml_text.is_empty() {
                    None
                } else {
                    Some(toml_text)
                },
                red_line,
            }
        })
        .collect())
}

/// Winapp2 预览项（前端展示用，含红线标注）。
#[derive(serde::Serialize)]
pub(crate) struct ParseWinapp2Item {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) risk: Risk,
    pub(crate) scope_count: usize,
    pub(crate) scopes: Vec<String>,
    pub(crate) disclaimer: String,
    /// 序列化好的 toml（勾选后直接 installScaffold 入库）。红线被拒项为 None。
    pub(crate) toml: Option<String>,
    pub(crate) red_line: Option<String>,
}

/// 红线拒绝理由：任一 scope 命中红线样本即返回可读原因（中文，直接进 UI）。
/// 与 CI 的 `scaffold-lint` 共用 `scaffold_red_line_violations`，也就是共用同一
/// 个**锚定空间**（`C:/Users/test`）——旧实现把未展开的原始 glob 送去比样本，
/// `%USERPROFILE%/**` 这类宽到覆盖整个 home 的 scope 在运行时能装进来，而执行期
/// 才展开变量，等于红线只在 CI 生效。
pub(crate) fn scaffold_red_line_reject(sc: &diskpilot_scaffold::Scaffold) -> Option<String> {
    let violations = diskpilot_scaffold::scaffold_red_line_violations(sc);
    violations.first().map(|(scope_id, hits)| {
        let shown: Vec<&str> = hits.iter().map(String::as_str).take(3).collect();
        format!(
            "脚本 `{}` 的清理项 `{}` 命中了红线路径族 `{}`{}，拒绝安装。\
             DiskPilot 红线：绝不可清理数据库/聊天记录/账号状态/收藏/加密物料等。",
            sc.id,
            scope_id,
            shown.join("、"),
            if hits.len() > 3 { " 等" } else { "" }
        )
    })
}

/// 安装核心：解析 → id 校验 → 运行时红线校验 → 写入用户目录 → 热重载。
/// 命令与社区仓库安装共用同一管线（保证验证一致性）。
pub(crate) fn install_scaffold_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    toml: String,
) -> Result<String, String> {
    let scaffold =
        diskpilot_scaffold::parse_toml(&toml).map_err(|e| format!("脚本格式无效：{e}"))?;
    if !is_valid_scaffold_id(&scaffold.id) {
        return Err(format!(
            "脚本 id 非法（仅允许小写字母/数字/连字符）：{}",
            scaffold.id
        ));
    }
    // 运行时红线校验：见 `scaffold_red_line_reject` 注释，口径与 CI lint 一致。
    if let Some(reason) = scaffold_red_line_reject(&scaffold) {
        return Err(reason);
    }

    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("scaffolds");
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建脚本目录：{e}"))?;
    let path = dir.join(format!("{}.toml", scaffold.id));
    std::fs::write(&path, toml).map_err(|e| format!("写入失败：{e}"))?;

    // 热重载：用户目录脚本覆盖内嵌同名脚本。
    *state.scaffolds.lock().unwrap() = load_all_scaffolds(app);
    tracing::info!("scaffold installed: {} -> {:?}", scaffold.id, path);
    Ok(scaffold.id)
}

/// 卸载社区 scaffold：只删除用户目录 `app_data_dir/scaffolds/{id}.toml`，
/// 绝不触碰内嵌/资源目录（安全铁律：只能删我们写入的东西）。删除后热重载，
/// 同名内嵌脚本自动回归生效。
#[tauri::command]
pub(crate) fn uninstall_scaffold(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    if !is_valid_scaffold_id(&id) {
        return Err("脚本 id 非法".into());
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("scaffolds");
    let path = dir.join(format!("{id}.toml"));
    if !path.exists() {
        return Err(format!("未找到用户安装的脚本 `{id}`（内嵌脚本不可卸载）"));
    }
    std::fs::remove_file(&path).map_err(|e| format!("删除失败：{e}"))?;
    *state.scaffolds.lock().unwrap() = load_all_scaffolds(&app);
    tracing::info!("scaffold uninstalled: {id}");
    Ok(())
}

/// 查询脚本来源："embedded"（内嵌打包）| "user"（用户目录，可卸载）。
/// 前端据此决定是否显示「卸载」按钮——内嵌脚本永远不显示。
#[tauri::command]
pub(crate) fn scaffold_source(app: AppHandle, id: String) -> String {
    let user_path = app
        .path()
        .app_data_dir()
        .map(|d| d.join("scaffolds").join(format!("{id}.toml")))
        .unwrap_or_default();
    if user_path.exists() {
        "user".into()
    } else {
        "embedded".into()
    }
}

pub(crate) fn load_all_scaffolds(handle: &AppHandle) -> Vec<Scaffold> {
    let mut by_id: HashMap<String, Scaffold> = HashMap::new();
    let mut candidates: Vec<PathBuf> = Vec::new();
    // Resource dir takes precedence — packaged scaffolds. Then user override
    // (app_data_dir) — community-contributed scaffolds win over bundled ones
    // with the same id.
    if let Ok(p) = handle.path().resource_dir() {
        candidates.push(p.join("scaffolds"));
    }
    candidates.push(PathBuf::from("scaffolds"));
    candidates.push(PathBuf::from("../../scaffolds"));
    candidates.push(PathBuf::from("../../../scaffolds"));
    if let Ok(p) = handle.path().app_data_dir() {
        candidates.push(p.join("scaffolds"));
    }
    for p in &candidates {
        if !p.exists() {
            continue;
        }
        match diskpilot_scaffold::load_dir(p) {
            Ok(v) => {
                for s in v {
                    // 装载闸：命中红线的脚本直接拒收（不进 map，因此也不会覆盖
                    // 同名内嵌脚本）。用户手工往 scaffolds 目录塞文件同样拦得住——
                    // 与 install_scaffold_inner 共用 `scaffold_red_line_reject`。
                    if let Some(reason) = scaffold_red_line_reject(&s) {
                        tracing::warn!("scaffold 拒绝装载：{reason}");
                        continue;
                    }
                    by_id.insert(s.id.clone(), s);
                }
            }
            Err(e) => tracing::warn!("could not load scaffolds from {:?}: {}", p, e),
        }
    }
    // Lowest-priority fallback: if no external source provided a given id,
    // fill it from the compile-time embed. This is what makes the portable
    // raw exe (cwd=Downloads, no resource_dir/scaffolds) usable.
    for f in EMBEDDED_SCAFFOLDS.files() {
        if f.path().extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Some(text) = f.contents_utf8() else {
            continue;
        };
        match parse_toml(text) {
            Ok(s) => {
                if let Some(reason) = scaffold_red_line_reject(&s) {
                    tracing::error!("内嵌脚本被红线闸拒绝（CI lint 应拦住这条）：{reason}");
                    continue;
                }
                by_id.entry(s.id.clone()).or_insert(s);
            }
            Err(e) => {
                tracing::warn!("embedded scaffold parse error in {:?}: {}", f.path(), e)
            }
        }
    }
    let mut out: Vec<Scaffold> = by_id.into_values().collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全部内嵌脚本必须过装载红线闸：这条钉死「校验口径 == 执行口径」的收敛不会
    /// 把已有功能误杀，也保证 canonicalize 变严后 CI 立刻报警而不是运行时装不上。
    #[test]
    fn embedded_scaffolds_all_pass_red_line_gate() {
        let mut checked = 0usize;
        for f in EMBEDDED_SCAFFOLDS.files() {
            if f.path().extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let Some(text) = f.contents_utf8() else {
                continue;
            };
            let sc = parse_toml(text).unwrap_or_else(|e| panic!("{:?}: {e}", f.path()));
            assert!(
                scaffold_red_line_reject(&sc).is_none(),
                "内置脚本被红线闸拒绝：{}",
                sc.id
            );
            checked += 1;
        }
        assert!(checked >= 18, "内嵌脚本数异常：{checked}");
    }
}
