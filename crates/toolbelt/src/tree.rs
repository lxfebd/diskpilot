//! 工具墙全目录扫描（对齐 tubatools ToolCatalog）。
//!
//! 从 `lib.rs` 拆分（2026-10-12）：遍历 Tools/ 根为主面板提供完整工具墙：
//! 分类子目录去重（仅架构后缀不同）、主启动文件判定、link.json 跨分类软链 +
//! builtin 占位、Metadata/tools.json 元数据叠加、exe 真实图标抽取。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::catalog::ToolPluginMeta;
use crate::risk::{default_risk, Risk};
use crate::root::find_tools_root;
use crate::run::tracing_warn;

/// 可双击启动的文件扩展名（同 tubatools LaunchableExtensions）。
const LAUNCHABLE_EXTS: &[&str] = &[".exe", ".bat", ".cmd", ".lnk", ".msc", ".ps1", ".vbs"];

/// 目录名/文件名中的架构后缀（同 C# ArchSuffixes，补 `_x32` 且按**长度降序**）。
/// C# 里 `cpuz_x32` 会被裸 `32` 抢先截成 `cpuz_x`、`cpuz_x64` 截成 `cpuz_`，
/// 归档比较全部落空——这里让 `_x64`/`_x32`/`_ARM64` 这类更具体的后缀先命中，
/// `cpuz_x32` → `cpuz`、`cpuz_x64` → `cpuz`，都能对上目录名 `CPUZ`。
const ARCH_SUFFIXES: &[&str] = &[
    "_Win64", "_Win32", "_ARM64", "ARM64", "_x64", "_x86", "_x32", "_64", "_32", "x64", "x86",
    "w64", "w32", "64", "32",
];

/// 宿主架构优先级（ARM64 > x64 > x86，与 C# PreferredArchPriority 对齐）。
fn preferred_arch_priority() -> &'static [&'static str] {
    #[cfg(target_arch = "aarch64")]
    return &["ARM64", "x64", "x86"];
    #[cfg(target_arch = "x86")]
    return &["x86"];
    #[cfg(target_arch = "x86_64")]
    return &["x64", "x86"];
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86", target_arch = "x86_64")))]
    return &["x64", "x86"];
}

/// 单条工具墙条目（序列化给前端面板渲染）。
#[derive(Clone, Debug, Serialize)]
pub struct ToolbeltCatalogItem {
    /// 展示名：主启动文件去掉扩展名；`start.exe` 则用父目录名（同 C# GetDisplayName）。
    pub name: String,
    /// 所在分类（Tools 根下的一级子目录名）。
    pub category: String,
    /// 工具目录相对 Tools 根（正斜杠）。
    pub dir_rel: String,
    /// 主启动文件相对 Tools 根（正斜杠）；找不到可执行时为 None（如 builtin 软链）。
    pub exe_rel: Option<String>,
    /// 主启动文件扩展名（小写，无点）；builtin 软链为 "内置"。
    pub extension: String,
    /// 主启动文件（exe）的真实图标，编码为 `data:image/png;base64,...`；
    /// 抽取失败（非 exe / 无图标资源）时为 None，前端回退到 Lucide 图标。
    pub icon: Option<String>,
    /// tools.json 叠来的简介；未收录则空串。
    pub description: String,
    /// tools.json 叠来的标签。
    pub tags: Vec<String>,
    /// 是否为 link.json 跨分类软链。
    pub is_linked: bool,
    /// 是否为 link.json builtin 占位（点开走内置逻辑，无本地文件）。
    pub is_builtin_link: bool,
    /// 主启动文件检测到的架构（"x64"/"x86"/"ARM64"，未知为空）。
    pub arch: String,
    /// 同目录/跨目录检测到的其他架构版本（文件名 → 架构）。
    pub arch_variants: Vec<ArchVariantOut>,
    /// 反向引用：哪些分类的 link.json 链到了本工具（跨分类复用的「主分类」标注）。
    pub linked_from: Vec<String>,
    /// 展示用风险级（"low"/"medium"/"high"；tools.json risk 优先，否则按名默认）。
    pub risk: String,
    /// tools.json 的 launchTarget（覆盖主启动文件，原版语义：指定入口 exe）。
    pub launch_target: Option<String>,
    /// tools.json 透传字段（详情卡「关于」区）。
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub tutorial_url: Option<String>,
    pub download_url: Option<String>,
    /// 是否为推广项：启动脚本（.bat）只做 `start <推广URL>` 跳转（流量卡/加速器
    /// 返利链等），不是真正可用的工具软件。前端据此灰化并给「推广」徽标，
    /// 提供一键隐藏；官网下载/在线工具跳转不算推广（is_promotion=false）。
    pub is_promotion: bool,
    /// 目录内 `tool.plugin.json` 的插件元数据（dsh 万物皆插件模式的本地落地）：
    /// 有则说明该工具是一个可装卸的插件单元；无则 null（传统工具照旧）。
    pub plugin: Option<ToolPluginMeta>,
}

/// 读取工具目录的 `tool.plugin.json`；缺失/解析失败返回 None（传统工具不是插件）。
fn load_plugin_meta(tool_dir: &Path) -> Option<ToolPluginMeta> {
    let p = tool_dir.join("tool.plugin.json");
    let text = std::fs::read_to_string(&p).ok()?;
    let meta: ToolPluginMeta = serde_json::from_str(&text).ok()?;
    if meta.id.trim().is_empty() || meta.entry.trim().is_empty() {
        return None;
    }
    Some(meta)
}

/// 一个架构变体（面板「其他版本」下拉用）。
#[derive(Clone, Debug, Serialize)]
pub struct ArchVariantOut {
    pub name: String,
    pub file_rel: String,
    pub arch: String,
}

/// 全目录扫描结果。
#[derive(Clone, Serialize)]
pub struct ToolbeltCatalog {
    pub tools_root: Option<String>,
    /// 分类 → 工具条目（按原版 12 分类顺序，未命中按目录名字母序排后）。
    pub categories: Vec<CategoryOut>,
    /// 工具总数（去重后）。
    pub total: usize,
    /// 原版分类显示顺序（前端左栏导航按此渲染）。
    pub category_order: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct CategoryOut {
    pub name: String,
    pub tools: Vec<ToolbeltCatalogItem>,
}

fn is_launchable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            LAUNCHABLE_EXTS
                .iter()
                .any(|x| e.eq_ignore_ascii_case(&x[1..]))
        })
        .unwrap_or(false)
}

// ─══════════════════════════════════════════════════════════════════════
// 抽取 exe 真实图标（Windows）：ExtractIconExW 取 HICON → GetIconInfo 拿
// 颜色位图 → GetDIBits 读 BGRA 像素 → 转 RGBA → PNG 编码 → base64 data URI。
// 非 Windows / 非 exe / 无图标资源时返回 None，前端回退 Lucide 图标。
// ═══════════════════════════════════════════════════════════════════════

/// 小型 base64 编码（避免再引一个 crate）。
fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// 判断某启动文件是否为「纯推广跳转」：只做 `start <URL>` 打开网页、
/// 且 URL 命中推广模式（图吧 `tbtool.cn` 推广/更新/皮肤页、`inviteCode`
/// 返利链）——不是可用的工具软件。加速器/游戏官网下载页（fnjiasu 等）、
/// 在线测试工具（testufo、pcbox、bmcx 等）是真实功能入口，不算推广。
///
/// 识别不依赖文件名，只读文件内容，避免误伤名字带「加速器/领卡」的真工具：
/// 一个 .bat 内容是多行脚本（有 @echo/if/%~dp0 等）就直接放行。
fn is_promotion_bat(launchable: &Path) -> bool {
    let Some(ext) = launchable.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    if !ext.eq_ignore_ascii_case("bat") && !ext.eq_ignore_ascii_case("cmd") {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(launchable) else {
        return false;
    };
    let body = content.trim().to_ascii_lowercase();
    // 多行脚本（含可执行逻辑）不是纯跳转，放行。
    if body.lines().count() > 2 {
        return false;
    }
    let url: String = body
        .lines()
        .find_map(|l| {
            let t = l.trim_start_matches('@').trim();
            t.strip_prefix("start")
                .or_else(|| t.strip_prefix("open"))
                .map(|r| r.trim().to_string())
        })
        .unwrap_or_default();
    if url.is_empty() {
        return false;
    }
    let is_tbtool = url.contains("tbtool.cn"); // 图吧官方域名：/links/* 推广页 + /Version/ 更新页 + /skin/ 皮肤页
    let is_invite = url.contains("invitecode") || url.contains("invite=");
    let is_promo_host = ["wanjiadongli.com", "fxzj.com", "go.funyouyou"]
        .iter()
        .any(|h| url.contains(h));
    is_tbtool || is_invite || is_promo_host
}

/// 抽取某 exe 的真实图标为 PNG data URI。失败返回 None。
fn exe_icon_data_uri(exe: &Path) -> Option<String> {
    let ext = exe
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if ext.as_deref() != Some("exe") {
        return None;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Graphics::Gdi::{
            CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
            BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
        };
        use windows_sys::Win32::UI::Shell::ExtractIconExW;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DestroyIcon, GetIconInfo, HICON, ICONINFO,
        };

        let wide: Vec<u16> = exe
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut h_large: HICON = std::ptr::null_mut();
        let mut h_small: HICON = std::ptr::null_mut();
        let n = unsafe { ExtractIconExW(wide.as_ptr(), 0, &mut h_large, &mut h_small, 1) };
        let _ = n;
        if h_large.is_null() {
            if !h_small.is_null() {
                unsafe { DestroyIcon(h_small) };
            }
            return None;
        }

        let result = (|| -> Option<Vec<u8>> {
            let mut ii: ICONINFO = unsafe { std::mem::zeroed() };
            if unsafe { GetIconInfo(h_large, &mut ii) } == 0 {
                return None;
            }
            let hbm: HBITMAP = if ii.hbmColor.is_null() {
                ii.hbmMask
            } else {
                ii.hbmColor
            };
            if hbm.is_null() {
                return None;
            }
            let mut bm: BITMAP = unsafe { std::mem::zeroed() };
            unsafe {
                GetObjectW(
                    hbm as HGDIOBJ,
                    std::mem::size_of::<BITMAP>() as i32,
                    &mut bm as *mut _ as *mut core::ffi::c_void,
                );
            }
            let w = bm.bmWidth;
            let h = bm.bmHeight;
            if w <= 0 || h <= 0 {
                return None;
            }
            let stride = (bm.bmWidthBytes.max(0) as usize).max((w as usize) * 4);
            let mut buf = vec![0u8; stride * (h as usize)];
            let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -h; // 负值 = 自上而下的像素行
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let hdc: HDC = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
            if hdc.is_null() {
                return None;
            }
            let got = unsafe {
                GetDIBits(
                    hdc,
                    hbm,
                    0,
                    h as u32,
                    buf.as_mut_ptr() as *mut core::ffi::c_void,
                    &mut bmi as *mut BITMAPINFO,
                    DIB_RGB_COLORS,
                )
            };
            unsafe { DeleteDC(hdc) };
            if got == 0 {
                return None;
            }

            // ── 透明度来源：mask 位图（AND mask）──────────────────────
            // 老图标（24bpp）转 32bpp 后 alpha 通道是垃圾/全 0 → 整张透明
            // 不可见（前端 <img> 不触发 onError，fallback 失效）。
            // 现代图标（32bpp 真 alpha）颜色位图自带渐变 alpha，直接采用；
            // 老图标透明形状可靠存放在 hbmMask（1 = 透明，0 = 不透明），
            // 用 mask 二值化重建 alpha。真 alpha 图标的抗锯齿边缘会变硬边，
            // 但形状清晰可见，远胜透明空白。若 mask 读不出（异常图标），
            // 退化为整帧不透明。
            // 真 alpha 判定：颜色位图里出现「中间 alpha」（渐变必有）。
            let mut color_has_real_alpha = false;
            if !ii.hbmColor.is_null() {
                for row in 0..(h as usize) {
                    let src = &buf[row * stride..(row * stride + (w as usize) * 4).min(buf.len())];
                    for c in src.chunks(4) {
                        let a = c[3];
                        if a != 0 && a != 255 {
                            color_has_real_alpha = true;
                            break;
                        }
                    }
                    if color_has_real_alpha {
                        break;
                    }
                }
            }
            let mut mask_bits: Vec<u8> = Vec::new(); // 每像素 1 bit（1=透明）
            if !color_has_real_alpha {
                let mut mb: BITMAP = unsafe { std::mem::zeroed() };
                if !ii.hbmMask.is_null()
                    && unsafe {
                        GetObjectW(
                            ii.hbmMask as HGDIOBJ,
                            std::mem::size_of::<BITMAP>() as i32,
                            &mut mb as *mut _ as *mut core::ffi::c_void,
                        )
                    } != 0
                {
                    let mw = mb.bmWidth;
                    let mh = mb.bmHeight;
                    // monochrome mask 高 = 2h（图标区 + AND/OR 压平区），只取上 h 行
                    if mw >= w && mh >= h {
                        // 关键：GetDIBits 对 1bpp DIB 每行按 32-bit 对齐写入
                        // （stride = ((mw+31)/32)*4），行字节只由我传入的
                        // BITMAPINFO 决定，**与源位图 bmWidthBytes 无关**——
                        // 老位图该字段是未对齐的 div_ceil(mw,8)（如 48px 图标
                        // bmWidthBytes=6，而 GetDIBits 实际写 8），若按它分配
                        // 缓冲区或做行步进，会每行越界写 → STATUS_HEAP_CORRUPTION
                        // 崩溃，读行也会错位。所以分配与读取统一用 stride32。
                        let stride32 = (mw as usize).div_ceil(32) * 4;
                        let mut mbuf = vec![0u8; stride32 * (mh as usize)];
                        let mut mbi: BITMAPINFO = unsafe { std::mem::zeroed() };
                        mbi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                        mbi.bmiHeader.biWidth = mw;
                        mbi.bmiHeader.biHeight = -mh; // 负值 = 自上而下
                        mbi.bmiHeader.biPlanes = 1;
                        mbi.bmiHeader.biBitCount = 1;
                        mbi.bmiHeader.biCompression = BI_RGB;
                        let hdc2: HDC = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
                        if !hdc2.is_null() {
                            let mgot = unsafe {
                                GetDIBits(
                                    hdc2,
                                    ii.hbmMask as HBITMAP,
                                    0,
                                    mh as u32,
                                    mbuf.as_mut_ptr() as *mut core::ffi::c_void,
                                    &mut mbi as *mut BITMAPINFO,
                                    DIB_RGB_COLORS,
                                )
                            };
                            unsafe { DeleteDC(hdc2) };
                            if mgot != 0 && mb.bmBitsPixel <= 1 {
                                // 真 monochrome mask：每行按 stride32（与 GetDIBits 写入一致），前 w 位有效
                                for row in 0..(h as usize) {
                                    let r = &mbuf[row * stride32
                                        ..(row * stride32 + stride32).min(mbuf.len())];
                                    for x in 0..(w as usize) {
                                        let byte = if x / 8 < r.len() { r[x / 8] } else { 0 };
                                        let bit = (byte >> (7 - (x % 8))) & 1;
                                        mask_bits.push(bit);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ── BGRA → RGBA ──────────────────────────────────────────
            let px = (w as usize) * (h as usize);
            let mut rgba = Vec::with_capacity(px * 4);
            if color_has_real_alpha {
                // 真 alpha 图标：直接用颜色位图的 alpha（保留渐变边缘）
                for row in 0..(h as usize) {
                    let src = &buf[row * stride..(row * stride + (w as usize) * 4).min(buf.len())];
                    for c in src.chunks(4) {
                        rgba.push(c[2]);
                        rgba.push(c[1]);
                        rgba.push(c[0]);
                        rgba.push(c[3]);
                    }
                }
            } else if mask_bits.len() == px {
                // mask 二值化 alpha：bit=1 → 透明
                for row in 0..(h as usize) {
                    let src = &buf[row * stride..(row * stride + (w as usize) * 4).min(buf.len())];
                    for (x, c) in src.chunks(4).enumerate() {
                        let a = if mask_bits[row * (w as usize) + x] == 1 {
                            0
                        } else {
                            255
                        };
                        rgba.push(c[2]);
                        rgba.push(c[1]);
                        rgba.push(c[0]);
                        rgba.push(a);
                    }
                }
            } else {
                // 无 mask 可读：整帧不透明（至少不再空白）
                for row in 0..(h as usize) {
                    let src = &buf[row * stride..(row * stride + (w as usize) * 4).min(buf.len())];
                    for c in src.chunks(4) {
                        rgba.push(c[2]);
                        rgba.push(c[1]);
                        rgba.push(c[0]);
                        rgba.push(255);
                    }
                }
            }

            // 位图句柄用完即释放（上面已用毕）
            unsafe {
                DeleteObject(ii.hbmColor as HGDIOBJ);
                DeleteObject(ii.hbmMask as HGDIOBJ);
            }
            let mut out: Vec<u8> = Vec::new();
            {
                let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                let mut writer = enc.write_header().ok()?;
                writer.write_image_data(&rgba).ok()?;
            }
            Some(out)
        })();

        unsafe {
            DestroyIcon(h_large);
            if !h_small.is_null() {
                DestroyIcon(h_small);
            }
        }
        result.map(|bytes| format!("data:image/png;base64,{}", base64_encode(&bytes)))
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        None
    }
}

/// 去掉文件/目录名尾部的架构后缀（同 C# StripArchSuffix）。
/// 注意：不能按字节切片——中文工具名是多字节 UTF-8，切片可能落在字符中间
/// panic。转小写后直接用 `ends_with` 比较（ASCII 后缀大小写不敏感）。
fn strip_arch_suffix(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    for suffix in ARCH_SUFFIXES {
        let sfx = suffix.to_ascii_lowercase();
        if lower.len() > sfx.len() && lower.ends_with(&sfx) {
            return name[..name.len() - suffix.len()].to_string();
        }
    }
    name.to_string()
}

/// 去架构后缀后再去掉残留的 `_`（如 `cpuz_x64` → `cpuz`，`WizTree64` → `WizTree`），
/// 用于与目录名/彼此比较：C# 里 `StripArchSuffix("cpuz_x64")` 得 `cpuz_`，与目录
/// `CPUZ` 比不相等，导致主文件判定/变体收集落空——这里把尾 `_` 也剥掉再比。
fn strip_arch_suffix_trim(name: &str) -> String {
    strip_arch_suffix(name).trim_end_matches('_').to_string()
}

/// 从文件名末尾识别架构（同 C# DetectArch + FormatArchDisplay）。
fn detect_arch(file_stem: &str) -> Option<&'static str> {
    let arm = ["ARM64", "_ARM64", "arm64", "_arm64"];
    let x64 = ["x64", "_x64", "64", "_64", "w64", "_Win64"];
    let x86 = ["x86", "_x86", "32", "_32", "w32", "_Win32"];
    for p in arm {
        if file_stem.ends_with(p) {
            return Some("ARM64");
        }
    }
    for p in x64 {
        if file_stem.ends_with(p) {
            return Some("x64");
        }
    }
    for p in x86 {
        if file_stem.ends_with(p) {
            return Some("x86");
        }
    }
    None
}

/// 已知多架构工具的文件名模式（跨目录关联，同 C# KnownMultiArchTools）。
const KNOWN_MULTI_ARCH: &[(&str, &[&str])] = &[
    (
        "cpuz",
        &[
            "cpuz_x64.exe",
            "cpuz_x32.exe",
            "cpuz_arm64.exe",
            "cpuz64.exe",
            "cpuz32.exe",
        ],
    ),
    (
        "hwinfo",
        &[
            "HWiNFO64.exe",
            "HWiNFO32.exe",
            "HWiNFO_ARM64.exe",
            "HWiNFO.exe",
        ],
    ),
    (
        "hwmonitor",
        &[
            "HWMonitor_x64.exe",
            "HWMonitor_x32.exe",
            "hwmonitor_arm64.exe",
            "HWMonitor.exe",
        ],
    ),
    (
        "dism++",
        &[
            "Dism++x64.exe",
            "Dism++x86.exe",
            "Dism++ARM64.exe",
            "Dism++.exe",
        ],
    ),
    (
        "coretemp",
        &["Core Temp x64.exe", "Core Temp x86.exe", "Core Temp.exe"],
    ),
    (
        "crystaldiskinfo",
        &[
            "DiskInfo64S.exe",
            "DiskInfo32S.exe",
            "DiskInfo.exe",
            "DiskInfo64.exe",
            "DiskInfo32.exe",
        ],
    ),
    (
        "bootice",
        &["BOOTICEx64.exe", "BOOTICEx86.exe", "BOOTICE.exe"],
    ),
    (
        "bluescreenview",
        &[
            "BlueScreenViewx64.exe",
            "BlueScreenViewx86.exe",
            "BlueScreenView.exe",
        ],
    ),
    ("aida64", &["aida64.exe", "aida64.exe.manifest"]),
    (
        "linx",
        &[
            "linpack_xeon64.exe",
            "linpack_xeon32.exe",
            "linpack_xeon.exe",
        ],
    ),
];

/// 收集一个工具目录内的全部可执行文件（递归；同 C# FindAllArchVariants 第一步）。
fn all_launchables(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                stack.push(p);
            } else if is_launchable(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// 主启动文件判定（同 C# FindPrimaryLaunchable，去掉 launchTarget 元数据分支，
/// 保留：精确名 → 去架构后缀 → 任意兜底）。`all` 已由调用方收集（catalog_tree 去重 IO）。
fn find_primary_launchable_with(
    tool_dir: &Path,
    dir_name: &str,
    all: &[PathBuf],
) -> Option<PathBuf> {
    if all.is_empty() {
        return None;
    }
    if all.len() == 1 {
        return all.first().cloned();
    }

    let direct: Vec<PathBuf> = std::fs::read_dir(tool_dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| is_launchable(p))
                .collect()
        })
        .unwrap_or_default();

    let stem_eq = |p: &Path, n: &str| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case(n))
            .unwrap_or(false)
    };
    let stem_stripped_eq = |p: &Path, n: &str| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| strip_arch_suffix_trim(s).eq_ignore_ascii_case(&strip_arch_suffix_trim(n)))
            .unwrap_or(false)
    };

    // 顶层精确名
    if let Some(m) = direct.iter().find(|p| stem_eq(p, dir_name)) {
        return Some(m.clone());
    }
    // 顶层去架构后缀
    let arch_cand: Vec<PathBuf> = direct
        .iter()
        .filter(|p| stem_stripped_eq(p, dir_name))
        .cloned()
        .collect();
    if !arch_cand.is_empty() {
        return Some(pick_preferred_arch(&arch_cand));
    }
    // 递归精确名
    if let Some(m) = all.iter().find(|p| stem_eq(p, dir_name)) {
        return Some(m.clone());
    }
    // 递归去架构后缀
    let arch_cand: Vec<PathBuf> = all
        .iter()
        .filter(|p| stem_stripped_eq(p, dir_name))
        .cloned()
        .collect();
    if !arch_cand.is_empty() {
        return Some(pick_preferred_arch(&arch_cand));
    }
    // 兜底
    if !direct.is_empty() {
        return Some(direct[0].clone());
    }
    all.first().cloned()
}

/// 从候选里按宿主架构优先级挑一个（同 C# PickPreferredArch）。
fn pick_preferred_arch(candidates: &[PathBuf]) -> PathBuf {
    for arch in preferred_arch_priority() {
        let patterns: &[&str] = match *arch {
            "ARM64" => &["ARM64", "_ARM64", "arm64", "_arm64"],
            "x64" => &["x64", "_x64", "64", "_64", "w64", "_Win64"],
            _ => &["x86", "_x86", "32", "_32", "w32", "_Win32"],
        };
        if let Some(m) = candidates.iter().find(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| patterns.iter().any(|pat| s.ends_with(pat)))
                .unwrap_or(false)
        }) {
            return m.clone();
        }
    }
    candidates[0].clone()
}

/// 同目录 + 跨目录（KNOWN_MULTI_ARCH）的架构变体收集（对齐 C# FindAllArchVariants）。
/// `all` 已由调用方收集（catalog_tree 去重 IO）。
fn find_arch_variants_with(
    tools_root: &Path,
    tool_dir: &Path,
    category_root: &Path,
    primary: &Path,
    dir_name: &str,
    all: &[PathBuf],
) -> Vec<ArchVariantOut> {
    let mut variants: Vec<ArchVariantOut> = Vec::new();
    let primary_ext = primary.extension().and_then(|e| e.to_str());

    // 1. 同目录内
    let dir_stripped = strip_arch_suffix_trim(dir_name);
    for f in all {
        if f == primary {
            continue;
        }
        if let Some(ext) = primary_ext {
            if !f
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case(ext))
                .unwrap_or(false)
            {
                continue;
            }
        }
        let Some(stem) = f.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(arch) = detect_arch(stem) else {
            continue;
        };
        let stripped = strip_arch_suffix_trim(stem);
        if !stripped.eq_ignore_ascii_case(&dir_stripped) && !stripped.eq_ignore_ascii_case(dir_name)
        {
            continue;
        }
        let file_rel = normalize_rel(tools_root, f);
        if !variants.iter().any(|v| v.file_rel == file_rel) {
            variants.push(ArchVariantOut {
                name: clean_name(&stripped),
                file_rel,
                arch: arch.to_string(),
            });
        }
    }

    // 2. 跨目录（已知多架构工具映射）
    let primary_file = primary.file_name().and_then(|s| s.to_str()).unwrap_or("");
    for (_, patterns) in KNOWN_MULTI_ARCH {
        if !patterns
            .iter()
            .any(|p| p.eq_ignore_ascii_case(primary_file))
        {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(category_root) else {
            break;
        };
        for ent in rd.flatten() {
            let other = ent.path();
            if other == *tool_dir || !other.is_dir() {
                continue;
            }
            let Some(other_name) = other.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !strip_arch_suffix_trim(other_name).eq_ignore_ascii_case(&dir_stripped) {
                continue;
            }
            for pattern in *patterns {
                let candidate = other.join(pattern);
                if candidate.is_file() && is_launchable(&candidate) {
                    let Some(stem) = candidate.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let Some(arch) = detect_arch(stem) else {
                        continue;
                    };
                    let file_rel = normalize_rel(tools_root, &candidate);
                    if !variants.iter().any(|v| v.file_rel == file_rel) {
                        variants.push(ArchVariantOut {
                            name: clean_name(&strip_arch_suffix(stem)),
                            file_rel,
                            arch: arch.to_string(),
                        });
                    }
                }
            }
        }
        break;
    }
    variants
}

/// `_x64`/`_x86`/`_arm64` → 空格分隔的展示名（同 C# CleanupName 的前四步）。
fn clean_name(stem: &str) -> String {
    let s = stem
        .replace("_x64", " x64")
        .replace("_x86", " x86")
        .replace("_ARM64", " ARM64")
        .replace("_arm64", " ARM64")
        .replace('_', " ");
    s.trim().to_string()
}

/// 相对 Tools 根的正斜杠路径。
fn normalize_rel(tools_root: &Path, p: &Path) -> String {
    p.strip_prefix(tools_root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 合并仅架构后缀不同的目录（同 C# MergeArchDirectories：保第一个，跳过后续同名 stripped）。
fn merge_arch_dirs(dir_names: &[String]) -> Vec<String> {
    let mut consumed = vec![false; dir_names.len()];
    let mut out = Vec::new();
    for (i, n) in dir_names.iter().enumerate() {
        if consumed[i] {
            continue;
        }
        let stripped_i = strip_arch_suffix_trim(n);
        out.push(n.clone());
        for (j, n2) in dir_names.iter().enumerate().skip(i + 1) {
            if consumed[j] {
                continue;
            }
            if strip_arch_suffix_trim(n2).eq_ignore_ascii_case(&stripped_i) {
                consumed[j] = true;
            }
        }
    }
    out
}

/// link.json 解析结果（对齐 C# LinkInfo）。
struct LinkInfo {
    /// "target" 相对 Tools 根的目标目录；builtin 软链为 None。
    target_rel: Option<String>,
    /// "builtin" 内置工具 id。
    builtin_id: Option<String>,
}

impl LinkInfo {
    fn is_builtin(&self) -> bool {
        self.builtin_id.is_some()
    }
}

/// 解析工具目录里的 link.json：目录必须「只有一个文件且无子目录」才视为软链
/// （同 C# TryResolveLink 的形态校验）。
fn try_resolve_link(tool_dir: &Path) -> Option<LinkInfo> {
    let link_path = tool_dir.join("link.json");
    if !link_path.is_file() {
        return None;
    }
    let files: Vec<_> = std::fs::read_dir(tool_dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let dirs: Vec<_> = files.iter().filter(|p| p.is_dir()).collect();
    if files.len() != 1 || !dirs.is_empty() {
        return None;
    }
    let text = std::fs::read_to_string(&link_path).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    if let Some(b) = doc.get("builtin").and_then(|v| v.as_str()) {
        if !b.trim().is_empty() {
            return Some(LinkInfo {
                target_rel: None,
                builtin_id: Some(b.to_string()),
            });
        }
    }
    if let Some(t) = doc.get("target").and_then(|v| v.as_str()) {
        if !t.trim().is_empty() {
            return Some(LinkInfo {
                target_rel: Some(t.to_string()),
                builtin_id: None,
            });
        }
    }
    None
}

/// tools.json 单条元数据（面板 + 详情卡展示字段；launchTarget 供启动走
/// 指定入口，与原版 TubaWinUi3 的 launchTarget 语义一致）。
#[derive(Debug, Default, Clone, serde::Deserialize)]
struct JsonToolMeta {
    #[serde(rename = "match", default)]
    match_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(rename = "launchTarget", default)]
    launch_target: Option<String>,
    #[serde(default)]
    publisher: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(rename = "tutorialUrl", default)]
    tutorial_url: Option<String>,
    #[serde(rename = "downloadUrl", default)]
    download_url: Option<String>,
    #[serde(rename = "displayName", default)]
    display_name: Option<String>,
    /// 风险覆盖（tools.json 权威度高于默认，toolbelt.toml 仍可再盖）。
    #[serde(default)]
    risk: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct JsonToolsDoc {
    #[serde(default)]
    tools: Vec<JsonToolMeta>,
}

/// 匹配 tools.json 的 `match`（宽松：包含 / 去空格-下划线后包含，同 C# MatchesFlexible）。
fn meta_matches(source: &str, m: &str) -> bool {
    if source
        .to_ascii_lowercase()
        .contains(&m.to_ascii_lowercase())
    {
        return true;
    }
    let norm = |s: &str| s.replace([' ', '-', '_'], "").to_ascii_lowercase();
    norm(source).contains(&norm(m))
}

/// 加载 Tools 根旁（或根内）的 Metadata/tools.json，返回 (match, meta) 列表。
/// 找不到/解析失败返回空，不 panic。
fn load_tools_meta(tools_root: &Path) -> Vec<(String, JsonToolMeta)> {
    let mut cands = Vec::new();
    if let Some(parent) = tools_root.parent() {
        cands.push(parent.join("Metadata").join("tools.json"));
        cands.push(parent.join("tools.json"));
    }
    cands.push(tools_root.join("Metadata").join("tools.json"));
    cands.push(tools_root.join("tools.json"));
    for c in cands {
        let Ok(text) = std::fs::read_to_string(&c) else {
            continue;
        };
        match serde_json::from_str::<JsonToolsDoc>(&text) {
            Ok(doc) => {
                return doc
                    .tools
                    .into_iter()
                    .filter_map(|t| t.match_name.clone().map(|m| (m, t)))
                    .collect();
            }
            Err(e) => tracing_warn(&format!("tools.json 解析失败 {:?}: {}", c, e)),
        }
    }
    Vec::new()
}

/// 查 tools.json 元数据：主文件相对路径（或目录名）优先精确包含 match，
/// 相同匹配长度取最长 match（同 C# FindJsonMetadata 的 OrderByDescending Match.Length）。
fn find_meta<'a>(
    meta: &'a [(String, JsonToolMeta)],
    primary_rel: &str,
    dir_name: &str,
) -> Option<&'a JsonToolMeta> {
    meta.iter()
        .filter(|(m, _)| meta_matches(primary_rel, m) || meta_matches(dir_name, m))
        .max_by_key(|(m, _)| m.chars().count())
        .map(|(_, t)| t)
}

/// **缝口 A 核心出口**：扫描整个 Tools/ 根，返回完整工具墙。
///
/// 不依赖任何嵌入式文档——磁盘上有什么就列什么。每分类下先 `merge_arch_dirs`
/// 去重，再逐目录判主启动文件、收架构变体、解 link.json、叠 tools.json 元数据。
///
/// 重构（全应用重构 W1）：
/// - 分类按原版 12 分类顺序排序（`display_order_rank`），未命中按目录名字母序排后，
///   并在返回值带 `category_order`（按原版顺序排好的分类名，前端左栏导航用）。
/// - 反向收集 link.json 跨分类引用：每个工具条目带 `linked_from`（哪些分类链到了它）。
/// - 每个工具目录只 `all_launchables` 一次（主文件判定 + 架构变体共享），去重 IO。
pub fn catalog_tree(explicit_root: Option<&Path>) -> ToolbeltCatalog {
    let root = match find_tools_root(explicit_root) {
        Ok(r) => r,
        Err(e) => {
            tracing_warn(&e.to_string());
            return ToolbeltCatalog {
                tools_root: None,
                categories: Vec::new(),
                total: 0,
                category_order: Vec::new(),
            };
        }
    };

    let meta = load_tools_meta(&root);

    // 第一趟：列出全部分类 + 每分类目录，并轻量收集 link.json 的跨分类 target。
    // linked_from 表：target 目录相对根（正斜杠，去尾斜杠）→ 链到它的分类名列表。
    let mut cat_dirs: Vec<(String, Vec<String>)> = Vec::new();
    let mut linked_from: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut cat_names: Vec<String> = std::fs::read_dir(&root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    // 原版 12 分类顺序优先，未命中按字母序排后。
    cat_names.sort_by(|a, b| {
        let ra = display_order_rank(a);
        let rb = display_order_rank(b);
        if ra == rb {
            a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
        } else {
            ra.cmp(&rb)
        }
    });

    for cat in &cat_names {
        let category_root = root.join(cat);
        let mut dir_names: Vec<String> = std::fs::read_dir(&category_root)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        dir_names.sort_by_key(|a| a.to_ascii_lowercase());
        cat_dirs.push((cat.clone(), dir_names.clone()));

        for dir_name in merge_arch_dirs(&dir_names) {
            let tool_dir = category_root.join(&dir_name);
            if let Some(link) = try_resolve_link(&tool_dir) {
                if let Some(target) = link.target_rel.as_deref() {
                    if !target.trim().is_empty() {
                        linked_from
                            .entry(target.trim_end_matches('/').to_string())
                            .or_default()
                            .push(cat.clone());
                    }
                }
            }
        }
    }

    let mut categories = Vec::new();
    let mut total = 0usize;

    for (cat, dir_names) in cat_dirs {
        let category_root = root.join(&cat);
        let mut tools = Vec::new();
        for dir_name in merge_arch_dirs(&dir_names) {
            let tool_dir = category_root.join(&dir_name);
            // 本目录真实归属的「目标工具目录」相对根（普通目录=自身；link=target）。
            // 用于查 linked_from 反向引用（排除自身分类）。
            let mut link_cats: Vec<String> = Vec::new();
            if let Some(cats) = linked_from.get(&normalize_rel(&root, &tool_dir)) {
                link_cats = cats.iter().filter(|c| **c != cat).cloned().collect();
            }

            // link.json 软链优先（builtin / 跨分类 target）
            if let Some(link) = try_resolve_link(&tool_dir) {
                if link.is_builtin() {
                    total += 1;
                    tools.push(ToolbeltCatalogItem {
                        name: dir_name.clone(),
                        category: cat.clone(),
                        dir_rel: normalize_rel(&root, &tool_dir),
                        exe_rel: None,
                        extension: "内置".to_string(),
                        icon: None,
                        description: String::new(),
                        tags: Vec::new(),
                        is_linked: true,
                        is_builtin_link: true,
                        arch: String::new(),
                        arch_variants: Vec::new(),
                        linked_from: link_cats,
                        risk: risk_of(&dir_name, None).to_string(),
                        launch_target: None,
                        publisher: None,
                        version: None,
                        tutorial_url: None,
                        download_url: None,
                        is_promotion: false,
                        plugin: load_plugin_meta(&tool_dir),
                    });
                    continue;
                }
                // 跨分类 target：主文件在目标目录里解析，展示分类仍挂在当前分类下
                let Some(target_rel) = link.target_rel.as_deref() else {
                    continue;
                };
                let target_full = root.join(target_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
                if !target_full.is_dir() {
                    continue;
                }
                let Some(target_dir_name) = target_full.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                let target_all = all_launchables(&target_full);
                if let Some(primary) =
                    find_primary_launchable_with(&target_full, target_dir_name, &target_all)
                {
                    let meta_hit =
                        find_meta(&meta, &normalize_rel(&root, &primary), target_dir_name);
                    let (primary_rel, arch) = primary_rel_arch(&root, &primary);
                    let variants = find_arch_variants_with(
                        &root,
                        &target_full,
                        &target_full
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_default(),
                        &primary,
                        target_dir_name,
                        &target_all,
                    );
                    // link 条目的 linked_from 用自身目录查（它本身可能是其他分类链来的）
                    let self_link_cats = linked_from
                        .get(&normalize_rel(&root, &tool_dir))
                        .map(|c| c.iter().filter(|x| **x != cat).cloned().collect())
                        .unwrap_or_default();
                    let (name, meta_disp) = meta_display(&meta_hit, &primary, target_dir_name);
                    total += 1;
                    tools.push(ToolbeltCatalogItem {
                        name,
                        category: cat.clone(),
                        dir_rel: normalize_rel(&root, &tool_dir),
                        exe_rel: Some(primary_rel),
                        extension: ext_of(&primary),
                        icon: exe_icon_data_uri(&primary),
                        description: meta_disp.0,
                        tags: meta_disp.1,
                        is_linked: true,
                        is_builtin_link: false,
                        arch,
                        arch_variants: variants,
                        linked_from: self_link_cats,
                        risk: risk_of(target_dir_name, meta_hit).to_string(),
                        launch_target: meta_hit.and_then(|m| m.launch_target.clone()),
                        publisher: meta_hit.and_then(|m| m.publisher.clone()),
                        version: meta_hit.and_then(|m| m.version.clone()),
                        tutorial_url: meta_hit.and_then(|m| m.tutorial_url.clone()),
                        download_url: meta_hit.and_then(|m| m.download_url.clone()),
                        is_promotion: is_promotion_bat(&primary),
                        plugin: load_plugin_meta(&target_full),
                    });
                }
                continue;
            }

            // 普通工具目录：一次收集，主文件 + 架构变体共享
            let all = all_launchables(&tool_dir);
            if let Some(primary) = find_primary_launchable_with(&tool_dir, &dir_name, &all) {
                let meta_hit = find_meta(&meta, &normalize_rel(&root, &primary), &dir_name);
                let (primary_rel, arch) = primary_rel_arch(&root, &primary);
                let variants = find_arch_variants_with(
                    &root,
                    &tool_dir,
                    &category_root,
                    &primary,
                    &dir_name,
                    &all,
                );
                let (name, meta_disp) = meta_display(&meta_hit, &primary, &dir_name);
                total += 1;
                tools.push(ToolbeltCatalogItem {
                    name,
                    category: cat.clone(),
                    dir_rel: normalize_rel(&root, &tool_dir),
                    exe_rel: Some(primary_rel),
                    extension: ext_of(&primary),
                    icon: exe_icon_data_uri(&primary),
                    description: meta_disp.0,
                    tags: meta_disp.1,
                    is_linked: false,
                    is_builtin_link: false,
                    arch,
                    arch_variants: variants,
                    linked_from: link_cats,
                    risk: risk_of(&dir_name, meta_hit).to_string(),
                    launch_target: meta_hit.and_then(|m| m.launch_target.clone()),
                    publisher: meta_hit.and_then(|m| m.publisher.clone()),
                    version: meta_hit.and_then(|m| m.version.clone()),
                    tutorial_url: meta_hit.and_then(|m| m.tutorial_url.clone()),
                    download_url: meta_hit.and_then(|m| m.download_url.clone()),
                    is_promotion: is_promotion_bat(&primary),
                    plugin: load_plugin_meta(&tool_dir),
                });
            }
        }
        if !tools.is_empty() {
            categories.push(CategoryOut { name: cat, tools });
        }
    }

    ToolbeltCatalog {
        tools_root: Some(root.display().to_string()),
        categories,
        total,
        category_order: cat_names,
    }
}

/// 展示用风险级：tools.json risk 优先，否则按工具名默认（`default_risk`）。
fn risk_of(name: &str, meta: Option<&JsonToolMeta>) -> Risk {
    if let Some(r) = meta.and_then(|m| m.risk.as_deref()).and_then(Risk::parse) {
        return r;
    }
    default_risk(name)
}

/// 展示名（尊重 tools.json displayName）+ (description, tags)。
fn meta_display(
    meta: &Option<&JsonToolMeta>,
    primary: &Path,
    dir_name: &str,
) -> (String, (String, Vec<String>)) {
    let name = meta
        .and_then(|m| m.display_name.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| get_display_name(primary, dir_name));
    let desc = meta.and_then(|m| m.description.clone()).unwrap_or_default();
    let tags = meta.map(|m| m.tags.clone()).unwrap_or_default();
    (name, (desc, tags))
}

/// 原版 12 分类显示顺序的磁盘目录名排序权重（未命中 = usize::MAX 排最后）。
/// 前端另有「磁盘目录名 → 原版显示名」映射表；这里只保证排序语义一致。
fn display_order_rank(cat: &str) -> usize {
    match cat {
        "硬件信息" => 1,
        "处理器工具" => 2,
        "CPU工具" => 2,
        "主板工具" => 3,
        "内存工具" => 4,
        "显卡工具" => 5,
        "磁盘工具" => 6,
        "硬盘工具" => 6,
        "显示器工具" => 7,
        "屏幕工具" => 7,
        "综合检测" => 8,
        "综合工具" => 8,
        "常用工具" => 8,
        "外设工具" => 9,
        "烤鸡工具" => 10,
        "游戏工具" => 11,
        "其他工具" => 12,
        _ => usize::MAX,
    }
}

fn primary_rel_arch(root: &Path, primary: &Path) -> (String, String) {
    let rel = normalize_rel(root, primary);
    let arch = primary
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(detect_arch)
        .unwrap_or("")
        .to_string();
    (rel, arch)
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// 展示名：非 start 文件用主文件名（去扩展名）；start.* 用父目录名（同 C# GetDisplayName）。
fn get_display_name(primary: &Path, dir_name: &str) -> String {
    let stem = primary
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(dir_name);
    if stem.eq_ignore_ascii_case("start") {
        dir_name.to_string()
    } else {
        stem.to_string()
    }
}
