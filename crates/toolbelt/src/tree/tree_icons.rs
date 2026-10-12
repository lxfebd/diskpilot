//! exe 图标抽取（从 tree.rs 拆分，2026-10-12）。
//! base64_encode + exe_icon_data_uri：抽取真实图标为 PNG data URI。

use std::path::Path;

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
pub(crate) fn is_promotion_bat(launchable: &Path) -> bool {
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
pub(crate) fn exe_icon_data_uri(exe: &Path) -> Option<String> {
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
