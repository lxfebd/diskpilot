//! 端口驱动动态加载域（从 superio.rs 拆分，2026-10-12）。
//! kernel32 extern 加载 inpoutx64/WinRing0/WinIo，解析 Inp32/Out32 入口点。

use std::sync::OnceLock;

// ── 端口驱动动态加载（kernel32 extern，fancmd 同款，不依赖 windows-sys feature）──

#[cfg(windows)]
#[allow(non_snake_case)]
mod ffi {
    use std::os::raw::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn LoadLibraryW(lpLibFileName: *const u16) -> *mut c_void;
        pub fn FreeLibrary(hModule: *mut c_void) -> i32;
        pub fn GetProcAddress(hModule: *mut c_void, lpProcName: *const u8) -> *mut c_void;
    }

    pub type InpFn = unsafe extern "system" fn(port: u16) -> u8;
    pub type OutFn = unsafe extern "system" fn(port: u16, val: u8);

    /// 把 GetProcAddress 返回的裸指针转成函数指针。
    pub unsafe fn to_fn<T>(ptr: *mut c_void) -> Option<T> {
        if ptr.is_null() {
            None
        } else {
            Some(std::mem::transmute_copy(&ptr))
        }
    }
}

/// 已加载的驱动句柄 + 入口点（进程内缓存，OnceLock 初始化）。
///
/// `*mut c_void` 裸指针不是 Send/Sync，包一层标记类型显式声明安全：
/// 指针只在加载驱动时写入一次，之后只读使用，且进程内所有线程共享同一驱动句柄
/// （inpoutx64 端口读写本身是原子的，单字节端口访问无需锁）。
#[cfg(windows)]
pub(crate) struct Driver {
    _lib: *mut std::os::raw::c_void,
    inp: ffi::InpFn,
    out: ffi::OutFn,
    pub(crate) name: &'static str,
}

// Safety: 指针在 init 后不可变，端口读写是单字节原子操作，跨线程共享安全。
#[cfg(windows)]
unsafe impl Send for Driver {}
#[cfg(windows)]
unsafe impl Sync for Driver {}

#[cfg(windows)]
static DRIVER: OnceLock<Option<Driver>> = OnceLock::new();

/// 候选驱动 DLL 名（优先 inpoutx64——本机已装内核服务，普通权限可用）。
#[cfg(windows)]
pub(crate) const DRIVER_CANDIDATES: [&str; 6] = [
    "inpoutx64.dll",
    "inpout32.dll",
    "WinRing0x64.dll",
    "WinRing0.dll",
    "WinIo64.dll",
    "WinIo32.dll",
];

/// 候选驱动目录：exe 目录 → 上溯 Tools 布局 → System32（fancmd CandidateDriverDirs 的 Rust 版）。
#[cfg(windows)]
fn candidate_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    let mut seen: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();
    let mut push = |p: std::path::PathBuf| {
        if p.is_dir() && seen.insert(p.clone()) {
            dirs.push(p);
        }
    };

    if let Ok(cur) = std::env::current_exe() {
        if let Some(parent) = cur.parent() {
            push(parent.to_path_buf());
        }
        // 上溯 6 层，找 Tools / fancontrol / fan 相关目录（fancmd 布局）
        let mut probe = cur.clone();
        for _ in 0..6 {
            if !probe.pop() {
                break;
            }
            if let Ok(rd) = std::fs::read_dir(&probe) {
                for ent in rd.flatten() {
                    let name = ent.file_name().to_string_lossy().to_lowercase();
                    if name.contains("tools") || name.contains("fancontrol") || name.contains("fan")
                    {
                        push(ent.path());
                        if let Ok(sub) = std::fs::read_dir(ent.path()) {
                            for s in sub.flatten() {
                                push(s.path());
                            }
                        }
                    }
                }
            }
        }
    }
    if let Ok(sys) = std::env::var("SystemRoot") {
        push(std::path::PathBuf::from(&sys).join("System32"));
    }
    // DISKPILOT_TOOLS_DIR（fancmd 同款：正式打包时驱动 DLL 随 Tools 目录分发）
    if let Ok(tools) = std::env::var("DISKPILOT_TOOLS_DIR") {
        push(std::path::PathBuf::from(tools));
    }
    dirs
}

/// 加载第一个可用的端口驱动（进程内缓存，只成功一次）。
#[cfg(windows)]
pub(crate) fn load_driver() -> Option<&'static Driver> {
    DRIVER
        .get_or_init(|| {
            unsafe fn try_load(full: &str, name: &'static str) -> Option<Driver> {
                let wide: Vec<u16> = full.encode_utf16().chain(std::iter::once(0)).collect();
                let h = unsafe { ffi::LoadLibraryW(wide.as_ptr()) };
                if h.is_null() {
                    return None;
                }
                let inp_ptr = unsafe { ffi::GetProcAddress(h, c"Inp32".as_ptr().cast()) };
                let out_ptr = unsafe { ffi::GetProcAddress(h, c"Out32".as_ptr().cast()) };
                let (inp, out) = if inp_ptr.is_null() || out_ptr.is_null() {
                    // 回退 ReadPort/WritePort（部分驱动命名不同）
                    let i2 = unsafe { ffi::GetProcAddress(h, c"ReadPort".as_ptr().cast()) };
                    let o2 = unsafe { ffi::GetProcAddress(h, c"WritePort".as_ptr().cast()) };
                    (unsafe { ffi::to_fn::<ffi::InpFn>(i2) }, unsafe {
                        ffi::to_fn::<ffi::OutFn>(o2)
                    })
                } else {
                    (unsafe { ffi::to_fn::<ffi::InpFn>(inp_ptr) }, unsafe {
                        ffi::to_fn::<ffi::OutFn>(out_ptr)
                    })
                };
                match (inp, out) {
                    (Some(inp), Some(out)) => Some(Driver {
                        _lib: h,
                        inp,
                        out,
                        name,
                    }),
                    _ => {
                        unsafe { ffi::FreeLibrary(h) };
                        None
                    }
                }
            }

            // 1) 候选目录逐一尝试
            for dir in candidate_dirs() {
                for name in DRIVER_CANDIDATES {
                    let full = dir.join(name);
                    if full.is_file() {
                        if let Some(d) = unsafe { try_load(&full.to_string_lossy(), name) } {
                            return Some(d);
                        }
                    }
                }
            }
            // 2) 系统搜索路径（DLL 目录 / System32 / PATH）
            for name in DRIVER_CANDIDATES {
                if let Some(d) = unsafe { try_load(name, name) } {
                    return Some(d);
                }
            }
            None
        })
        .as_ref()
}

/// 端口读（调用方必须先做驱动预检；这里 debug_assert 兜底，
/// 未加载时读回 0xFF 而不是 panic——防未来调用路径绕过预检把主进程打崩）。
#[cfg(windows)]
#[inline]
pub(crate) fn inp(port: u16) -> u8 {
    let d = match load_driver() {
        Some(d) => d,
        None => {
            debug_assert!(false, "superio: 驱动未加载——调用方应先行预检");
            return 0xFF;
        }
    };
    unsafe { (d.inp)(port) }
}

/// 端口写（只用于 SuperIO 地址口/数据口寻址，不写任何设备配置）。
#[cfg(windows)]
#[inline]
pub(crate) fn outp(port: u16, val: u8) {
    let d = match load_driver() {
        Some(d) => d,
        None => {
            debug_assert!(false, "superio: 驱动未加载——调用方应先行预检");
            return;
        }
    };
    unsafe { (d.out)(port, val) }
}
