use std::collections::HashMap;
pub(crate) mod agent;
pub(crate) mod ai;
mod ai_net;
mod cleanup;
mod cleanup_logic;
mod conda;
mod drivers;
mod dups;
mod executor;
mod general_tray;
pub(crate) mod hw;
mod hw_history;
mod hw_report;
pub(crate) mod mcp;
mod mcp_bridge;
mod net_proxy;
mod plugin_registry;
mod plugin_registry_types;
mod plugin_remote;
mod reminder;
mod scaffold_registry;
mod scan;
mod space_history;
pub(crate) mod startup;
pub(crate) mod steam;
pub(crate) mod system;
pub(crate) mod toolbelt;
pub(crate) mod updates;
mod webview;
mod weekly_report;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use diskpilot_scaffold::{parse_toml, Risk, Scaffold};
use diskpilot_scanner::Node;
use include_dir::{include_dir, Dir};
use tauri::{AppHandle, Manager, State};

pub(crate) use cleanup_logic::CleanupCacheEntry;

// 进程级共享 HTTP 客户端：网络搜索 / Steam 标题 / AI 代理都复用同一份
// 连接池与 TLS 会话缓存，避免每个请求重新握手、连接池打不满。
static SHARED_HTTP: OnceLock<reqwest::Client> = OnceLock::new();
/// AI 代理专用：禁用自动重定向。默认跟随会把「首跳合法公网 → 302 到内网」
/// 的 SSRF 绕过漏掉，ai::ai_proxy 用手动跟随并逐个校验跳转目标。
static SHARED_HTTP_NO_REDIRECT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client_base() -> reqwest::ClientBuilder {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        );
    if let Some(proxy_str) = net_proxy::system_https_proxy() {
        if let Ok(p) = reqwest::Proxy::all(&proxy_str) {
            builder = builder.proxy(p);
        }
    }
    builder
}

fn shared_http() -> &'static reqwest::Client {
    SHARED_HTTP.get_or_init(|| {
        http_client_base()
            .build()
            .expect("reqwest client build cannot fail")
    })
}

pub(crate) fn http_no_redirect() -> &'static reqwest::Client {
    SHARED_HTTP_NO_REDIRECT.get_or_init(|| {
        http_client_base()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client build cannot fail")
    })
}

/// SSRF 防护下最多读多少字节的响应体：插件包/索引/脚本都远小于此，
/// 防止恶意服务器用无限响应打爆本进程内存。
const SSRF_MAX_BODY: u64 = 256 * 1024 * 1024;

/// SSRF 安全的 GET 下载：禁自动重定向 + 逐跳校验私网 + 响应体上限。
///
/// 插件/DiskPilot 语义区别于 AI 代理——这是「下载资产」而非「转发用户内容」，
/// 所以不做任意 host 的代理转发，只做受控 GET。默认共享 client 跟随重定向的
/// 「首跳合法公网 → 302 到内网」绕过在这里被堵死：每个 Location 都过一遍
/// `ai::blocked_private_target`（与 AI 代理同一把尺），最多 5 跳防环。
/// 返回 200 响应体；失败删除半截下载物（调用方传 out 时由调用方负责，便于
/// 渐进式进度上报——本 helper 只负责网络层，落盘由调用方做）。
///
/// `on_chunk` 可选回调（下载进度上报）。
pub(crate) async fn ssrf_safe_get(
    url: &str,
    max_bytes: u64,
    mut on_chunk: Option<Box<dyn FnMut(u64, u64) + Send>>,
) -> Result<Vec<u8>, String> {
    let parsed = url::Url::parse(url.trim()).map_err(|e| format!("URL 非法：{e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!(
            "下载地址必须使用 https（当前协议：{}）。为防中间人篡改，http 与自定义协议一律拒绝。",
            parsed.scheme()
        ));
    }
    if let Some(blocked) = crate::ai::blocked_private_target(parsed.as_str()) {
        return Err(format!("下载目标不可用（{blocked}）：{parsed}"));
    }
    let client = http_no_redirect();
    let mut url = parsed.as_str().to_string();
    let mut hops = 0;
    loop {
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("下载失败（网络错误)：{e}"))?;
        let status = resp.status();
        if status.is_redirection() {
            let loc = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| "下载重定向缺少 Location 头".to_string())?
                .to_string();
            let next = resp
                .url()
                .join(&loc)
                .map_err(|e| format!("重定向目标不合法: {e}"))?;
            hops += 1;
            if hops > 5 {
                return Err("下载重定向次数过多（超过 5 跳）".to_string());
            }
            if let Some(blocked) = crate::ai::blocked_private_target(next.as_str()) {
                return Err(format!("下载重定向目标不可用（{blocked}）：{next}"));
            }
            url = next.to_string();
            continue;
        }
        if !status.is_success() {
            return Err(format!(
                "下载失败：HTTP {}（{}）",
                status.as_u16(),
                status.canonical_reason().unwrap_or("")
            ));
        }
        let total = resp.content_length().unwrap_or(0);
        if total > max_bytes {
            return Err(format!(
                "响应体过大（{} bytes，上限 {} MB）",
                total,
                max_bytes / (1024 * 1024)
            ));
        }
        let mut body = Vec::new();
        let mut downloaded: u64 = 0;
        let mut stream = resp;
        while let Some(chunk) = stream.chunk().await.map_err(|e| format!("下载中断：{e}"))? {
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            if downloaded > max_bytes {
                return Err(format!(
                    "响应体超过上限（{} MB），已中断",
                    max_bytes / (1024 * 1024)
                ));
            }
            if let Some(cb) = on_chunk.as_mut() {
                cb(downloaded, total);
            }
            body.extend_from_slice(&chunk);
        }
        return Ok(body);
    }
}

// Compile-time embed of repo-root scaffolds/. Used as the lowest-priority
// fallback in load_all_scaffolds so a portable raw exe (no resource_dir,
// arbitrary cwd) still ships with all packaged scaffolds.
static EMBEDDED_SCAFFOLDS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../../scaffolds");

struct AppState {
    scaffolds: Mutex<Vec<Scaffold>>,
    quarantine_root: PathBuf,
    undo_log: PathBuf,
    /// Shared abort flag for in-flight scans. Reset to false at the start of
    /// each `scan_path`, flipped by `cancel_scan` so a long walk/MFT pass can
    /// be interrupted from the frontend instead of hanging until it finishes.
    scan_cancel: Arc<AtomicBool>,
    /// 最近一次 `scan_path` 完成后、基于全量扫描树（非截断文件列表）算出的
    /// 各 scope 可清理字节缓存。`cleanup_suggestions` 优先命中这里秒回，
    /// 避免每次进盘详情页都重新全盘 jwalk 一遍（实测 C 盘 500s+）。
    /// key = 归一化后的盘根路径（如 `C:\` → `c:\`）。
    cleanup_cache: Mutex<HashMap<String, CleanupCacheEntry>>,
    /// 每盘（归一化盘根 key）最近一次扫描后的完整 Node 树。AI 上下文/总览、
    /// 清理建议秒算、USN 增量合并、tree_subtree 展开都从对应 key 的缓存树读，
    /// 避免前端把整棵树序列化回来、或每次切盘都要重新全盘遍历。
    /// key = 归一化后的盘根路径（如 `C:\` → `c:\`），与 cleanup_cache / usn_cursors 同 key。
    scan_tree: Mutex<HashMap<String, Node>>,
    /// 每盘（归一化盘根 key）最近一次扫描后的 USN Journal 游标。`scan_path_usn`
    /// 用它在增量可用时只回放变更日志并 merge 进 `scan_tree` 缓存树，把重扫
    /// 从分钟级降到亚秒级。游标与 scan_tree 生命周期一致：树被覆盖时同步更新。
    /// 非 Windows 无 journal，用 `()` 占位（`commit_scan` 泛型参数退化，行为不变）。
    #[cfg(windows)]
    usn_cursors: Mutex<HashMap<String, diskpilot_scanner::usn::UsnCursor>>,
    #[cfg(not(windows))]
    usn_cursors: Mutex<HashMap<String, ()>>,
    /// 进行中的硬件压测「用户停止」开关列表：每个测试实例注册自己的
    /// Arc<AtomicBool>，hw_stop_test 遍历全部置 true。用多槽而非单槽：
    /// 单槽全局 flag 会被后启动的测试 reset，导致先启动的测试停不下来。
    hw_stops: Mutex<Vec<Arc<AtomicBool>>>,
    /// 后端视角的 AI 权限快照（前端 sync_perms 同步，key 为权限 id 如
    /// `cleanup.execute` / `plugin.manage`）。前端权限中心改动后调 sync_perms
    /// 推一份到后端，`execute_ai_plan` / `toolbelt_run` 等写命令据此做纵深校验
    /// （防「绕过前端直调 Tauri」）。默认空 = 无 L2 授权，写操作一律拒绝。
    perm_grants: Mutex<std::collections::HashSet<String>>,
    /// 前端 WebView2 心跳：`webview_heartbeat` 命令每次调用记录当前时间戳。
    /// watchdog 线程检测到长时间无心跳（> 12s）判定 WebView 渲染进程已崩溃，
    /// 触发 reload 自愈。None = 从启动到首次心跳（或 reload 后）之间的宽限期，
    /// 期间不判定超时，避免「应用刚起来前端还没就绪」被误杀。见
    /// `spawn_webview_watchdog`。
    last_heartbeat: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// 进行中的 AI 代理请求「用户停止」开关表：key = 前端生成的 cancel_key
    /// （UUID），ai_proxy 注册自己的 Arc<AtomicBool>，ai_cancel(key) 按 key 置
    /// true，ai_proxy 的发送循环轮询到 true 即中断返回「已取消」。请求结束
    /// 无论成功失败都移除 key，防止表无限膨胀。
    ai_cancels: Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>,
    /// WebView2 重建门闩：`recreate_webview` 开始重建时置位，重建完成（或
    /// 超时放弃）后清除。watchdog 在门闩置位期间不触发新的重建/心跳 reload，
    /// 防止「重建进行中又下一 tick 触发」叠枪，以及「新进程组还没起来就被
    /// 探测为死亡 → 无限重建循环」。同时用作恢复期保护：重建成功后会短暂
    /// 保持置位（process alive 探活兜底），让新 WebView2 进程组有时间起来。
    /// 超时兜底：超过 30s 强制清除（Alive 过 8s 超时 + build 几秒 + 进程起
    /// 需要时间，正常情况下远小于 30s）。
    webview_busy: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
}

#[derive(serde::Serialize, Clone)]
struct VolumeInfo {
    /// 盘符，如 "C:\"。仅 list_drives 填充；volume_info 单查时不带（历史兼容）。
    #[serde(default)]
    path: String,
    total_bytes: u64,
    used_bytes: u64,
    free_bytes: u64,
}

/// 枚举本机所有可用磁盘，返回盘符与空间信息。供"全盘扫描"首页直出，
/// 长辈不必再手动选文件夹。
#[tauri::command]
fn list_drives() -> Vec<VolumeInfo> {
    let mut out = Vec::new();
    for letter in b'C'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if let Ok(mut info) = volume_info(root.clone()) {
            info.path = root;
            // used_bytes == 0 且经 GetDiskFreeSpaceExW 成功 -> 普通可读盘
            out.push(info);
        }
    }
    out
}

#[tauri::command]
fn volume_info(path: String) -> Result<VolumeInfo, String> {
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
async fn estimate_size(path: String) -> Result<u64, String> {
    let p = PathBuf::from(&path);
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

fn is_controlled_perm(id: &str) -> bool {
    CONTROLLED_PERMS.contains(&id)
}

/// 前端把 AI 权限快照同步到后端（权限中心改动/应用启动时调用）。
/// 后端写命令（execute_ai_plan / toolbelt_run / plugin_*）据此做纵深校验：
/// 即使被绕过前端直接 invoke，未授权的 L2 写操作也会被后端拒绝。
#[tauri::command]
fn sync_perms(state: State<'_, AppState>, enabled: Vec<String>) {
    let mut grants = state.perm_grants.lock().unwrap();
    grants.clear();
    grants.extend(enabled.into_iter().filter(|s| is_controlled_perm(s)));
}

#[tauri::command]
fn list_scaffolds(state: State<'_, AppState>) -> Vec<Scaffold> {
    state.scaffolds.lock().unwrap().clone()
}

#[tauri::command]
fn detect_scaffold(state: State<'_, AppState>, path: String) -> Option<String> {
    diskpilot_scaffold::detect_for(
        &state.scaffolds.lock().unwrap(),
        std::path::Path::new(&path),
    )
}

/// scaffold id 合法性校验（防路径穿越：id 会拼进文件名）。
/// 只允许小写字母/数字/连字符，禁止空串。
fn is_valid_scaffold_id(id: &str) -> bool {
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
fn install_scaffold(
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
fn parse_winapp2(content: String) -> Result<Vec<ParseWinapp2Item>, String> {
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
struct ParseWinapp2Item {
    id: String,
    name: String,
    risk: Risk,
    scope_count: usize,
    scopes: Vec<String>,
    disclaimer: String,
    /// 序列化好的 toml（勾选后直接 installScaffold 入库）。红线被拒项为 None。
    toml: Option<String>,
    red_line: Option<String>,
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
fn uninstall_scaffold(
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
fn scaffold_source(app: AppHandle, id: String) -> String {
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

// ── 硬件检测与受控压测（hw_detect）────────────────────────────────────
// 对齐 tubatools 硬件助手设计：
//   L0 只读：hw_info / hw_disk_health / hw_report —— 零确认，随时可调
//   L1 受控：hw_run_test —— 永远要 user_confirmed=true，超时强杀 + 温度熔断
//   L2 授权：power_plan —— 每次仍需 confirmed=true，用户可在权限中心开启
// 所有工具调用只走 toolbelt 白名单解析出的 exe，路径不由 AI 提供。
// 读不到的字段一律优雅降级为 null，绝不编造数值。

/// 在 run() 开头（没有 AppHandle 时）推算 app_data_dir：Windows 上是
/// %APPDATA%\{bundle_identifier}。与 Tauri 的默认 app_data_dir 保持一致。
fn app_data_dir_for_env() -> Option<PathBuf> {
    let base = std::env::var("APPDATA")
        .ok()
        .map(PathBuf::from)
        .or_else(dirs::data_dir)?;
    Some(base.join("dev.diskpilot.app"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // 硬件加速开关：关闭时给 WebView2 加 --disable-gpu。WebView2 环境在
    // Builder.run() 创建窗口时建立，所以必须在这之前设环境变量。
    // 读不到配置/读取出错时保持默认（硬件加速开）。
    let hw_accel = std::env::var("DISKPILOT_SKIP_HW").is_err()
        && app_data_dir_for_env()
            .map(|dir| {
                std::fs::read_to_string(dir.join("general.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str::<general_tray::GeneralConfig>(&s).ok())
                    .map(|c| c.hardware_accel)
                    .unwrap_or(true)
            })
            .unwrap_or(true);
    if !hw_accel {
        let existing = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGS").unwrap_or_default();
        let merged = if existing.trim().is_empty() {
            "--disable-gpu".to_string()
        } else {
            format!("{existing} --disable-gpu")
        };
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGS", merged);
        tracing::info!("hardware acceleration: DISABLED (--disable-gpu)");
    } else {
        tracing::info!("hardware acceleration: enabled (default)");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from(".diskpilot"));
            std::fs::create_dir_all(&data_dir).ok();
            let undo_log = data_dir.join("undo.jsonl");
            let quarantine_root = data_dir.join("quarantine");

            let scaffolds = load_all_scaffolds(app.handle());
            tracing::info!("loaded {} scaffolds", scaffolds.len());

            app.manage(AppState {
                scaffolds: Mutex::new(scaffolds),
                quarantine_root,
                undo_log,
                scan_cancel: Arc::new(AtomicBool::new(false)),
                cleanup_cache: Mutex::new(HashMap::new()),
                scan_tree: Mutex::new(HashMap::new()),
                usn_cursors: Mutex::new(HashMap::new()),
                hw_stops: Mutex::new(Vec::new()),
                perm_grants: Mutex::new(std::collections::HashSet::new()),
                last_heartbeat: Mutex::new(None),
                ai_cancels: Mutex::new(std::collections::HashMap::new()),
                webview_busy: Mutex::new(None),
            });

            // 平台支持时建托盘（取不到图标/无 appindicator 则跳过，非致命）。
            general_tray::spawn_tray(app);
            // WebView2 崩溃自愈 watchdog：后台线程周期检查前端心跳。
            webview::spawn_webview_watchdog(app.handle().clone());
            // 清理提醒后台线程：配置开启后定时算建议 + emit（只出清单，不删）。
            reminder::spawn_cleanup_reminder(app.handle().clone());
            // 插件 update/rollback 崩溃残留的 *.dp-old-tmp 目录启动清扫。
            plugin_registry::sweep_orphaned_tmp_dirs(app.handle());
            Ok(())
        })
        // 托盘常驻：点窗口关闭按钮时查 close_to_tray 配置，开 → 隐藏到托盘。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let close_to_tray =
                    general_tray::general_config_at(window.app_handle()).close_to_tray;
                if close_to_tray {
                    api.prevent_close();
                    window.hide().ok();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            webview::webview_heartbeat,
            scan::scan_path,
            scan::cancel_scan,
            scan::tree_subtree,
            #[cfg(windows)]
            scan::scan_path_usn,
            estimate_size,
            sync_perms,
            list_scaffolds,
            detect_scaffold,
            install_scaffold,
            parse_winapp2,
            uninstall_scaffold,
            scaffold_source,
            scaffold_registry::scaffold_registry_list,
            scaffold_registry::scaffold_registry_install,
            cleanup::scope_sizes_batch,
            cleanup::cleanup_suggestions,
            cleanup::chat_scan_context,
            executor::execute_scope,
            conda::list_conda_envs,
            conda::inspect_path,
            conda::reveal_in_explorer,
            executor::recycle_paths,
            executor::execute_ai_plan,
            executor::list_undo,
            executor::undo,
            executor::cleanup_preflight,
            dups::scan_duplicate_files_cmd,
            volume_info,
            reminder::get_reminder_config,
            reminder::set_reminder_config,
            reminder::run_reminder_check_cmd,
            weekly_report::generate_weekly_report_cmd,
            weekly_report::get_weekly_reports,
            space_history::get_space_history,
            hw_history::get_hw_history,
            hw_history::compare_hw_snapshots,
            list_drives,
            steam::list_steam_games,
            steam::list_steam_workshop_items,
            steam::fetch_workshop_titles,
            steam::translate_steam_names,
            steam::translate_steam_names_fetch,
            steam::open_steam_url,
            ai_net::web_search,
            ai::ai_proxy,
            ai::ai_cancel,
            agent::agent_list_tools,
            agent::agent_call_tool,
            agent::agent_tool_cancel,
            agent::agent_server_ready,
            mcp::mcp_list_servers,
            mcp::mcp_test_server,
            mcp::mcp_add_server,
            mcp::mcp_update_server,
            mcp::mcp_remove_server,
            mcp::mcp_list_tools,
            mcp::mcp_call_tool,
            mcp::mcp_audit_tail,
            general_tray::general_config,
            general_tray::set_general,
            general_tray::set_tools_root,
            general_tray::set_close_to_tray,
            general_tray::tray_sync,
            toolbelt::toolbelt_status,
            toolbelt::toolbelt_catalog,
            toolbelt::toolbelt_usage,
            toolbelt::toolbelt_run,
            toolbelt::toolbelt_manifests,
            toolbelt::toolbelt_launch,
            toolbelt::plugin_uninstall,
            toolbelt::plugin_install,
            toolbelt::plugin_market,
            toolbelt::plugin_activate,
            toolbelt::plugin_deactivate,
            toolbelt::plugin_export,
            plugin_remote::plugin_install_url,
            plugin_registry::plugin_registry_list,
            plugin_registry::plugin_registry_search,
            plugin_registry::plugin_registry_install,
            plugin_registry::plugin_registry_verify,
            plugin_registry::plugin_registry_refresh,
            plugin_registry::plugin_registry_update,
            plugin_registry::plugin_registry_rollback,
            plugin_registry::plugin_list_installed,
            plugin_registry::plugin_registry_config,
            plugin_registry::plugin_set_registry_url,
            toolbelt::toolbelt_recycle,
            updates::check_update,
            hw::hw_info,
            hw::hw_disk_health,
            hw::hw_run_test,
            hw::hw_stop_test,
            hw::hw_report,
            hw::power_plan,
            hw::fan_curve_advice,
            hw::fan_control,
            hw::fan_status,
            hw::fan_curve_apply,
            system::run_system_probe,
            system::network_probe,
            startup::list_startup_items,
            startup::set_startup_item,
            startup::remove_startup_item,
            drivers::list_installed_drivers,
            drivers::check_driver_updates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn load_all_scaffolds(handle: &AppHandle) -> Vec<Scaffold> {
    use std::collections::HashMap;
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
    use std::fs;

    /// winapp2 社区规则导入预览：危险条目（Chrome Autofill → `*Web Data`）
    /// 必须被红线标注拒绝，干净缓存条目不误伤。钉死「社区规则红线标注真实
    /// 生效」，防止以后加样本/改 canonicalize 把这条缝口堵死又松开。
    #[test]
    fn parse_winapp2_marks_red_line_and_passes_clean() {
        let ini = r#"
[Chrome Autofill Community *]
LangSecRef=3029
DetectFile=%LocalAppData%\Google\Chrome*
FileKey1=%LocalAppData%\Google\Chrome*\User Data\*|*Web Data

[Clean Temp Cache *]
LangSecRef=3025
DetectFile=%LocalAppData%\Temp
FileKey1=%LocalAppData%\Temp|*.tmp
"#;
        let items = parse_winapp2(ini.to_string()).expect("样例应能解析出条目");
        assert_eq!(items.len(), 2, "2 条文件规则都要落地");

        let autofill = items
            .iter()
            .find(|i| i.name.contains("Autofill"))
            .expect("Chrome Autofill 条目存在");
        assert!(
            autofill.red_line.is_some(),
            "`*Web Data`（表单自动填充库）必须被红线拒绝：{:?}",
            autofill.red_line
        );

        let clean = items
            .iter()
            .find(|i| i.name.contains("Temp Cache"))
            .expect("Temp Cache 条目存在");
        assert!(
            clean.red_line.is_none(),
            "纯临时文件缓存不该被误伤：{:?}",
            clean.red_line
        );
    }

    /// 运行时红线闸必须与 CI lint 同尺。旧实现把未展开的原始 glob 送去比样本，
    /// 而样本全锚在 `C:/Users/test`，于是 `%USERPROFILE%/**`（开发机上展开成真实
    /// home）能装进用户脚本目录，执行期才展开变量 → 红线实际只在 CI 生效。
    #[test]
    fn runtime_red_line_gate_rejects_wide_home_scope() {
        let evil = r#"
id = "evil"
name = "Evil"
risk = "low"
disclaimer = "probe"
detect = ["%USERPROFILE%"]
[[scope]]
id = "wipe-home"
label = "Wide"
glob = "%USERPROFILE%/**"
mode = "delete"
"#;
        let sc = diskpilot_scaffold::parse_toml(evil).expect("evil 应能解析");
        let reason = scaffold_red_line_reject(&sc).expect("覆盖整个 home 的 scope 必须被拒绝");
        assert!(reason.contains("红线"), "原因文案要能直接进 UI：{reason}");

        // 干净缓存脚本不许被误伤。
        let clean = evil.replace(
            "glob = \"%USERPROFILE%/**\"",
            "glob = \"%LOCALAPPDATA%/Company/Cache/**\"",
        );
        let sc = diskpilot_scaffold::parse_toml(&clean).expect("clean 应能解析");
        assert!(
            scaffold_red_line_reject(&sc).is_none(),
            "缓存 scope 不该被拒：{:?}",
            scaffold_red_line_reject(&sc)
        );
    }

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

    /// Verifies `diskpilot_walker` skips system trash / volume metadata directories
    /// at directory-read time. Without this prune a scope glob with a leading
    /// `**` (literal_separator=false makes `**` cross `/`) can match files
    /// inside `$Recycle.Bin/<SID>/$R*/...` because Windows preserves the
    /// original directory tree there — meaning a "clean WeChat cache" preview
    /// would list files the user already chose to put in the trash.
    #[test]
    fn diskpilot_walker_skips_system_trash_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let normal = root.join("normal_dir");
        fs::create_dir_all(&normal).unwrap();
        fs::write(normal.join("keep.txt"), b"x").unwrap();

        for trashy in &[
            "$RECYCLE.BIN",
            "System Volume Information",
            ".Trash",
            ".Trashes",
        ] {
            let d = root.join(trashy);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("inside.txt"), b"x").unwrap();
        }

        let mut files: Vec<String> = diskpilot_scanner::diskpilot_walker(root)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_string_lossy().replace('\\', "/"))
            .collect();
        files.sort();

        assert!(
            files.iter().any(|p| p.ends_with("/normal_dir/keep.txt")),
            "walker must still visit non-system dirs, got: {files:?}"
        );
        for trashy in &[
            "$RECYCLE.BIN",
            "System Volume Information",
            ".Trash",
            ".Trashes",
        ] {
            assert!(
                !files.iter().any(|p| p.contains(trashy)),
                "walker leaked into pruned dir `{trashy}`: {files:?}"
            );
        }
    }

    #[test]
    fn diskpilot_walker_prune_is_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let lower = root.join("$recycle.bin");
        fs::create_dir_all(&lower).unwrap();
        fs::write(lower.join("inside.txt"), b"x").unwrap();

        let leaked = diskpilot_scanner::diskpilot_walker(root)
            .into_iter()
            .flatten()
            .any(|e| e.file_type().is_file());
        assert!(
            !leaked,
            "lowercase $recycle.bin variant must also be pruned"
        );
    }

    /// 真实接口验证：cleanup_suggestions 同款核心逻辑（build_scope_builds +
    /// tally_scope_sizes）对真实 C 盘用户目录扫描，看能否算出可清理字节。
    /// 仅本机手动运行（-- --ignored --nocapture）。
    #[test]
    #[ignore]
    fn real_cleanup_suggestions_on_local_appdata() {
        // DP_TEST_ROOT 可覆盖扫描根（复现 UI 传 C:\ 整盘的场景）。
        let root = std::env::var("DP_TEST_ROOT")
            .map(PathBuf::from)
            .or_else(|_| {
                std::env::var("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .or_else(|_| {
                        std::env::var("USERPROFILE")
                            .map(|u| PathBuf::from(u).join("AppData").join("Local"))
                    })
            })
            .expect("需要 DP_TEST_ROOT 或 LOCALAPPDATA");
        if !root.is_dir() {
            panic!("目录不存在: {}", root.display());
        }
        println!("ROOT = {}", root.display());

        let scaffolds_dir =
            std::path::Path::new("j:\\xiangm_transfer\\xiangm\\tools\\diskpilot_src\\scaffolds");
        let scaffolds = diskpilot_scaffold::load_dir(scaffolds_dir).expect("加载真实 scaffolds");
        println!("scaffolds = {}", scaffolds.len());

        // 与 cleanup_suggestions 相同：所有 scaffold 的 scope 合并，
        // 文件 scope 一次 walk，目录 scope 一次 walk（tally_dir_scopes）。
        struct Row {
            scaffold: String,
            id: String,
            granularity: diskpilot_scaffold::RecycleGranularity,
            set: globset::GlobSet,
        }
        let mut rows: Vec<Row> = Vec::new();
        for sc in &scaffolds {
            for b in crate::cleanup_logic::build_scope_builds(sc).expect("glob 编译") {
                rows.push(Row {
                    scaffold: sc.id.clone(),
                    id: b.id,
                    granularity: b.granularity,
                    set: b.set,
                });
            }
        }
        println!("scopes = {}", rows.len());

        let mut tally: Vec<(u64, u64)> = vec![(0, 0); rows.len()];
        let mut total: Vec<(u64, u64)> = vec![(0, 0); rows.len()];
        let file_idx: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.granularity == diskpilot_scaffold::RecycleGranularity::File)
            .map(|(i, _)| i)
            .collect();
        let dir_idx: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.granularity == diskpilot_scaffold::RecycleGranularity::Directory)
            .map(|(i, _)| i)
            .collect();
        println!(
            "file scopes = {}, dir scopes = {}",
            file_idx.len(),
            dir_idx.len()
        );

        let t0 = std::time::Instant::now();
        if !file_idx.is_empty() {
            for entry in diskpilot_scanner::diskpilot_walker(&root)
                .into_iter()
                .flatten()
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                let md = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let path_str = path.to_string_lossy().replace('\\', "/");
                let size = md.len();
                for &i in &file_idx {
                    if rows[i].set.is_match(&path_str) {
                        total[i].0 += size;
                        total[i].1 += 1;
                        tally[i].0 += size;
                        tally[i].1 += 1;
                    }
                }
            }
        }
        println!("file walk done in {:?}", t0.elapsed());

        if !dir_idx.is_empty() {
            let t1 = std::time::Instant::now();
            let (d_total, d_tally) = diskpilot_scanner::tally_dir_scopes(
                &root,
                |i| &rows[i].set,
                |i| &rows[i].id,
                &dir_idx,
                None,
                None,
                None,
            );
            for (pos, &i) in dir_idx.iter().enumerate() {
                total[i] = d_total[pos];
                tally[i] = d_tally[pos];
            }
            println!("dir walk done in {:?}", t1.elapsed());
        }

        let mut grand_total: u64 = 0;
        for (i, r) in rows.iter().enumerate() {
            if total[i].0 > 0 || tally[i].0 > 0 {
                grand_total += tally[i].0;
                println!(
                    "  {:<16} {:<26} tally={} MB (n {})  total={} MB (n {})",
                    r.scaffold,
                    r.id,
                    tally[i].0 / 1_000_000,
                    tally[i].1,
                    total[i].0 / 1_000_000,
                    total[i].1,
                );
            }
        }
        println!(
            "== 建议清理合计 ≈ {} MB，全程 {:?} ==",
            grand_total / 1_000_000,
            t0.elapsed()
        );
        assert!(
            grand_total > 0,
            "对 {} 竟然一个字节都算不出来",
            root.display()
        );
    }

    fn test_scope(id: &str, glob: &str) -> diskpilot_scaffold::Scope {
        diskpilot_scaffold::Scope {
            id: id.into(),
            label: id.into(),
            glob: glob.into(),
            mode: diskpilot_scaffold::Mode::Recycle,
            prompt: None,
            category: None,
            variant: None,
            recycle_granularity: Default::default(),
            recommended_selected: None,
            required_stopped_processes: None,
        }
    }

    fn test_scaffold(scopes: Vec<diskpilot_scaffold::Scope>) -> Scaffold {
        Scaffold {
            id: "test-scaffold".into(),
            name: "test".into(),
            homepage: None,
            risk: diskpilot_scaffold::Risk::Low,
            disclaimer: "test".into(),
            detect: vec![],
            matcher: Default::default(),
            tags: Vec::new(),
            scopes,
        }
    }

    #[test]
    fn build_scope_builds_compiles_valid_globs() {
        let s = test_scaffold(vec![
            test_scope("pycache", "**/__pycache__"),
            test_scope("conda_pkgs", "**/pkgs/*"),
        ]);
        let builds = crate::cleanup_logic::build_scope_builds(&s).unwrap();
        assert_eq!(builds.len(), 2);
        assert_eq!(builds[0].id, "pycache");
        // 默认 File 粒度
        assert_eq!(
            builds[0].granularity,
            diskpilot_scaffold::RecycleGranularity::File
        );
        // 大小写不敏感 + `*` 跨分隔符（literal_separator=false）
        assert!(builds[0].set.is_match("C:/Users/a/Projects/x/__PYCACHE__"));
        assert!(builds[1].set.is_match("D:/envs/py3/pkgs/numpy/info"));
    }

    #[test]
    fn build_scope_builds_rejects_invalid_glob() {
        let s = test_scaffold(vec![test_scope("bad", "[")]);
        let err = crate::cleanup_logic::build_scope_builds(&s).unwrap_err();
        assert!(err.contains("scope `bad`"), "err: {err}");
        assert!(err.contains("invalid glob"), "err: {err}");
    }

    #[test]
    fn build_scope_builds_expands_env_vars() {
        // scope glob 里的 ${USERPROFILE} 先展开再编译，命中展开后的真实路径
        let s = test_scaffold(vec![test_scope("env", "${USERPROFILE}/tmpcache/**")]);
        let builds = crate::cleanup_logic::build_scope_builds(&s).unwrap();
        // USERPROFILE 是 Windows 变量；非 Windows 上它不被展开（glob 原样编译），
        // 该用例只验证「展开后命中」这一行为，非 Windows 直接跳过断言。
        let Ok(up_raw) = std::env::var("USERPROFILE") else {
            return;
        };
        let up = up_raw.replace('\\', "/");
        assert!(
            builds[0].set.is_match(format!("{up}/tmpcache/sub/f.txt")),
            "展开后的 glob 应命中 USERPROFILE 下的路径"
        );
    }

    #[test]
    fn scaffold_id_validation_blocks_path_traversal() {
        // 安全关键：id 会拼进文件名，路径穿越字符必须全拒。
        assert!(is_valid_scaffold_id("wechat-pc"));
        assert!(is_valid_scaffold_id("my-tool2"));
        assert!(!is_valid_scaffold_id(""), "空 id 拒绝");
        assert!(!is_valid_scaffold_id("../evil"), "路径穿越拒绝");
        assert!(!is_valid_scaffold_id("a/b"), "分隔符拒绝");
        assert!(!is_valid_scaffold_id("a\\b"), "反斜杠拒绝");
        assert!(!is_valid_scaffold_id("UPPER"), "大写拒绝");
        assert!(!is_valid_scaffold_id("has space"), "空格拒绝");
        assert!(!is_valid_scaffold_id("a.toml"), "点拒绝");
    }

    // ── ssrf_safe_get：SSRF 防线回归 ──────────────────────────────

    /// 公网 302 → 内网（127.0.0.1）的「首跳合法 → 重定向绕过」必须被逐跳
    /// 校验拦下，即使首跳本身是公网域名。
    #[tokio::test]
    async fn ssrf_redirect_to_loopback_is_blocked() {
        use std::io::{Read, Write};
        // 本地起一个 HTTP 服务，把 /ok 重定向到 http://127.0.0.1:9（无效端口，
        // 若被跟随会连接失败而非「已拒绝」——断言错误信息里的私网字样）
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let body = "redirect";
                let resp = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        // 注意：首跳目标是 127.0.0.1（本地测试服务），ssrf_safe_get 第一步
        // 就会因为私网拒绝——这验证的是「私网首跳拒绝」。重定向绕过验证
        // 依赖公网首跳，单测里无法真打公网；这里验证核心语义：任何私网
        // 目标（含 302 后跳向私网）都无法到达。
        let err = ssrf_safe_get(&format!("http://127.0.0.1:{port}/ok"), 1024, None)
            .await
            .expect_err("私网目标必须拒绝");
        assert!(
            err.contains("https") || err.contains("不可用"),
            "拒绝信息应说明协议/私网原因: {err}"
        );
    }

    /// http 协议被白名单拦下（私网校验在 ai.rs 已全覆盖，这里验证 ssrf_safe_get
    /// 本身不放开协议白名单）。
    #[tokio::test]
    async fn ssrf_http_scheme_is_rejected() {
        let err = ssrf_safe_get("http://127.0.0.1:1/a", 10, None)
            .await
            .expect_err("http 私网必须拒绝");
        assert!(err.contains("https"), "http 应被协议白名单拦下: {err}");
    }

    /// 公网域名（字面量不在私网段）放行第一步——不做 DNS 反查（避免
    /// 代理 DNS 重绑定），这里只验证 blocked_private_target 判定不误伤公网。
    #[test]
    fn ssrf_public_host_literal_passes() {
        assert_eq!(
            crate::ai::blocked_private_target("https://example.com/a"),
            None
        );
        assert_eq!(
            crate::ai::blocked_private_target("https://plugins.example.org/pack.zip"),
            None
        );
    }

    /// sync_perms 白名单：合法受控权限 id 通过，L3 与未知字符串一律丢弃。
    #[test]
    fn sync_perms_whitelist_filters_l3_and_unknown() {
        let good = ["plugin.manage", "cli.run", "cleanup.execute", "fan.control"];
        for id in good {
            assert!(is_controlled_perm(id), "{id} 应在白名单内");
        }
        let bad = [
            "disk.format",
            "bios.flash",
            "hw.overclock",
            "system.files",
            "",
            "..",
            "plugin.manage; rm -rf",
        ];
        for id in bad {
            assert!(!is_controlled_perm(id), "{id} 必须被白名单拒绝");
        }
    }
}
