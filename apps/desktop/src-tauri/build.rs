fn main() {
    // 关键：监听前端产物，dist 变化时强制重新嵌入。否则改完前端只跑 cargo build
    // 不会重跑 generate_context!()，部署的 exe 仍是旧/空前端（白屏"打不开"）。
    println!("cargo:rerun-if-changed=../dist");
    println!("cargo:rerun-if-changed=../dist/index.html");

    // agent-server 是 externalBin（tauri.conf.json 指向 `binaries/agent-server`）。
    // tauri-build 会校验 `<name>-<target-triple>[.exe]` 必须存在，缺了就 panic，
    // 所以这里兜底把 cargo 产物复制成 triple 后缀名。
    //
    // 正式打包路径是 `scripts/prepare-sidecar.mjs`（beforeBuildCommand）——它会先
    // 按应用的 target 编译再落地，那时目标文件已存在，这里直接跳过。
    // 这里的兜底服务 dev / cargo check / cargo test：没有它，本机改一行 Rust 就
    // 连编译都进不去（旧实现把这段写死了 `.exe`，macOS/Linux 上必然失效）。
    stage_sidecar_fallback();

    // 磁盘清理涉及系统级目录（回收站、应用缓存、WinSxS 邻近数据），
    // 默认以管理员权限启动才能读取/清理受保护路径，并走 MFT 快速通道。
    // 开发调试时若不想每次弹 UAC，可设 DISKPILOT_NO_ADMIN=1 走普通权限
    // （仅影响 dev 构建的 exe，不影响正式打包的安装包）。
    // 不声明这行的话，切换 DISKPILOT_NO_ADMIN 不会触发 build script 重跑，
    // 已嵌入的旧 manifest（requireAdministrator）会一直粘着测试/开发 exe，
    // 非提权 shell 里 cargo test 直接 os error 740 拉不起来。
    println!("cargo:rerun-if-env-changed=DISKPILOT_NO_ADMIN");
    let level = if std::env::var_os("DISKPILOT_NO_ADMIN").is_some() {
        "asInvoker"
    } else {
        "requireAdministrator"
    };
    let manifest = format!(
        r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="{level}" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#
    );
    let mut windows = tauri_build::WindowsAttributes::new();
    windows = windows.app_manifest(&manifest);
    let attrs = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}

/// 把 workspace 里已编译的 agent-server 产物复制成 tauri 要求的
/// `binaries/agent-server-<triple>[.exe]`。目标已存在（prepare-sidecar.mjs 跑过）
/// 或找不到产物（还没编过 agent-server）都静默跳过 —— 后者交给 tauri-build 报错，
/// 错误信息本身就会说清「先跑 cargo build -p agent-server」。
fn stage_sidecar_fallback() {
    let Ok(triple) = std::env::var("TARGET") else {
        return;
    };
    let bin_name = if cfg!(windows) {
        "agent-server.exe"
    } else {
        "agent-server"
    };
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_root = std::env::var("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("../../../target"));

    let dst_dir = manifest_dir.join("binaries");
    let dst = dst_dir.join(format!(
        "agent-server-{triple}{}",
        if cfg!(windows) { ".exe" } else { "" }
    ));
    if dst.exists() {
        return;
    }
    // --target 编译产物在 <target>/<triple>/<profile>，不带 --target 在 <target>/<profile>。
    let sources = [
        target_root.join(&triple).join("release").join(bin_name),
        target_root.join("release").join(bin_name),
        target_root.join(&triple).join("debug").join(bin_name),
        target_root.join("debug").join(bin_name),
    ];
    let Some(src) = sources.into_iter().find(|p| p.is_file()) else {
        return;
    };
    if std::fs::create_dir_all(&dst_dir).is_ok() {
        let _ = std::fs::copy(&src, &dst);
    }
}
