//! Safety test for engine-caches scaffold — 红线断言保证只清理引擎级缓存。
//!
//! 跑：`cargo test -p diskpilot-scaffold --test engine_caches_safety`
//!
//! 本测试覆盖三家游戏引擎：
//!   - Unity: 全局 cache/Caches/Editor 日志 + UnityHub logs；红线是任何项目
//!     目录下的 Library/Assets/ProjectSettings、.sln 文件。
//!   - Unreal Engine: Common/DerivedDataCache（DDC）+ DLC Partial；红线是
//!     任何 .uproject 所在的项目目录整体、Engine 安装目录、Saved/Config。
//!   - Godot: %APPDATA%/Godot 下的 editor_cache/export_cache；红线是
//!     editor_settings.cfg、editor_layout.cfg、app_userdata 下的每项目数据、
//!     项目 .godot/editor 关键文件。
//! 同时包含通用红线样本（.db / config / login / key / crypto / Favorite 等）。

use std::path::PathBuf;

const SCAFFOLD_FILE: &str = "scaffolds/engine-caches.toml";

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}

fn load_scaffold() -> diskpilot_scaffold::Scaffold {
    let path = workspace_root().join(SCAFFOLD_FILE);
    let text = std::fs::read_to_string(&path).expect("read scaffold toml");
    toml::from_str(&text).expect("parse scaffold toml")
}

fn build_set(pattern: &str) -> globset::GlobSet {
    let g = globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .case_insensitive(true)
        .build()
        .unwrap_or_else(|e| panic!("bad glob `{pattern}`: {e}"));
    let mut b = globset::GlobSetBuilder::new();
    b.add(g);
    b.build().unwrap()
}

fn expand(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(end) = bytes[i + 1..].iter().position(|&b| b == b'%') {
                let var = std::str::from_utf8(&bytes[i + 1..i + 1 + end]).unwrap_or("");
                if let Ok(v) = std::env::var(var) {
                    out.push_str(&v.replace('\\', "/"));
                    i += end + 2;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn matching_scopes<'a>(scopes: &'a [(String, globset::GlobSet)], path: &str) -> Vec<&'a str> {
    scopes
        .iter()
        .filter_map(|(id, gs)| {
            if gs.is_match(path) {
                Some(id.as_str())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn engine_caches_globs_are_safe() {
    std::env::set_var("USERPROFILE", "C:/Users/test");
    std::env::set_var("LOCALAPPDATA", "C:/Users/test/AppData/Local");
    std::env::set_var("APPDATA", "C:/Users/test/AppData/Roaming");
    std::env::set_var("HOME", "/home/test");

    let scaffold = load_scaffold();
    let scopes: Vec<(String, globset::GlobSet)> = scaffold
        .scopes
        .iter()
        .map(|s| (s.id.clone(), build_set(&expand(&s.glob))))
        .collect();

    // ========================================================================
    // 正向断言：每个 scope id 至少一条命中路径
    // ========================================================================
    let positives: &[(&str, &str)] = &[
        // Unity 包下载缓存（本机实测：cache/npm、cache/packages）
        (
            "unity-package-cache",
            "C:/Users/test/AppData/Local/Unity/cache/packages/packages.unity.cn/com.unity.ugui/1.0.0",
        ),
        (
            "unity-package-cache",
            "C:/Users/test/AppData/Local/Unity/cache/npm/packages.unity.cn/some-pkg.tgz",
        ),
        // Unity 编辑器日志（本机实测：Editor/Editor.log, Editor-prev.log, upm.log）
        (
            "unity-editor-logs",
            "C:/Users/test/AppData/Local/Unity/Editor/Editor.log",
        ),
        (
            "unity-editor-logs",
            "C:/Users/test/AppData/Local/Unity/Editor/Editor-prev.log",
        ),
        (
            "unity-editor-logs",
            "C:/Users/test/AppData/Local/Unity/Editor/upm.log",
        ),
        // Unity Hub 日志（本机实测：Roaming/Unity Hub/logs）
        (
            "unity-hub-logs",
            "C:/Users/test/AppData/Roaming/UnityHub/logs/unityhub-2026.log",
        ),
        (
            "unity-hub-logs",
            "C:/Users/test/AppData/Local/UnityHub/logs/unityhub-2026.log",
        ),
        // Unreal DDC
        (
            "unreal-derived-data-cache",
            "C:/Users/test/AppData/Local/UnrealEngine/Common/DerivedDataCache/Data/DerivedDataCache.utoc",
        ),
        (
            "unreal-derived-data-cache",
            "C:/Users/test/AppData/Local/UnrealEngine/Common/DerivedDataCache/Data/DerivedDataCache.utao",
        ),
        // Unreal DDC Partial
        (
            "unreal-dlc-partial",
            "C:/Users/test/AppData/Local/UnrealEngine/Common/DerivedDataCache_Partial/Data/Partial.utoc",
        ),
        // Godot editor cache
        (
            "godot-editor-cache",
            "C:/Users/test/AppData/Roaming/Godot/editor_cache/cache.bin",
        ),
        // Godot export cache
        (
            "godot-export-cache",
            "C:/Users/test/AppData/Roaming/Godot/export_cache/preset1/pack.zip",
        ),
    ];
    for (expected_id, p) in positives {
        let hits = matching_scopes(&scopes, p);
        assert!(
            hits.contains(expected_id),
            "expected scope `{expected_id}` to match `{p}`, got {hits:?}",
        );
    }

    // ========================================================================
    // 红线断言：以下路径必须不被任何 scope 命中
    // ========================================================================
    let red_lines: &[&str] = &[
        // ---- Unity 项目目录红线（新手最常误删的 Library/）----
        // 项目 Library/：资源导入数据库，删了强制全量重导且可能丢编辑器状态
        "C:/code/MyUnityGame/Library/ArtifactDB",
        "C:/code/MyUnityGame/Library/ArtifactURLToHashCache",
        "C:/code/MyUnityGame/Library/BurstCache/Jobs/Job0.dat",
        "C:/code/MyUnityGame/Library/ShaderCache/shader.bin",
        "C:/code/MyUnityGame/Library/ScriptAssemblies/Assembly-CSharp.dll",
        "C:/code/MyUnityGame/Library/EditorOnlyVirtualTextAssets/asset",
        // 项目 Assets/：项目源码
        "C:/code/MyUnityGame/Assets/Scenes/Main.unity",
        "C:/code/MyUnityGame/Assets/Scripts/Player.cs",
        "C:/code/MyUnityGame/Assets/Plugins/AssetBundle",
        // 项目 ProjectSettings/：项目编辑器配置（红线）
        "C:/code/MyUnityGame/ProjectSettings/ProjectSettings.asset",
        "C:/code/MyUnityGame/ProjectSettings/QualitySettings.asset",
        // 项目 .git
        "C:/code/MyUnityGame/.git/HEAD",
        // 项目 .sln
        "C:/code/MyUnityGame/MyUnityGame.sln",
        // 项目 Temp/：本任务拿不准就不动
        "C:/code/MyUnityGame/Temp/UnityTempFile-a1b2c3.dat",
        "C:/code/MyUnityGame/Temp/obj/Debug/Player.cs",
        // 用户文档/下载里手动建的"看起来像"的目录（重要的假阳性防线）
        "C:/Users/test/Documents/Unity/cache/foo",
        "C:/Users/test/Documents/UnrealEngine/Common/DerivedDataCache/foo",
        "C:/Users/test/Downloads/Godot/editor_cache/notes.txt",
        "C:/Users/test/Desktop/UnityHub/logs/readme.md",

        // ---- Unity 全局红线：config / licenses / 顶层 log 文件 ----
        // %LOCALAPPDATA%/Unity/config/：全局引擎配置（红线）
        "C:/Users/test/AppData/Local/Unity/config/production.json",
        // %LOCALAPPDATA%/Unity/licenses/：授权文件（红线）
        "C:/Users/test/AppData/Local/Unity/licenses/packages/lic.lic",
        // %LOCALAPPDATA%/Unity/Unity.*.log 顶层 log 文件：本 scaffold 只清 Editor/ 子目录，
        // 顶层的 Licensing/Entitlements log 属于配置/授权状态，不碰
        "C:/Users/test/AppData/Local/Unity/Unity.Licensing.Client.log",
        "C:/Users/test/AppData/Local/Unity/Unity.Entitlements.Audit.log",

        // ---- Unity Hub 红线：凭据 / 用户配置 / 项目列表 ----
        // Unity Hub 的 CloudConfig、Settings、Local Storage 等
        "C:/Users/test/AppData/Roaming/UnityHub/cloudConfig.json",
        "C:/Users/test/AppData/Roaming/UnityHub/Settings",
        "C:/Users/test/AppData/Roaming/UnityHub/favoriteProjects.json",
        "C:/Users/test/AppData/Roaming/UnityHub/projectsArchitecture.json",
        "C:/Users/test/AppData/Roaming/UnityHub/hubConfig.json",
        "C:/Users/test/AppData/Roaming/UnityHub/Cookies",
        "C:/Users/test/AppData/Roaming/UnityHub/Cookies-journal",
        "C:/Users/test/AppData/Roaming/UnityHub/Network Persistent State",
        "C:/Users/test/AppData/Roaming/UnityHub/Local Storage/leveldb/000005.log",
        "C:/Users/test/AppData/Roaming/UnityHub/Session Storage/000005.log",
        "C:/Users/test/AppData/Roaming/UnityHub/blob_storage/abc",
        "C:/Users/test/AppData/Roaming/UnityHub/Cache/js/foo",
        "C:/Users/test/AppData/Roaming/UnityHub/Code Cache/js/foo",
        "C:/Users/test/AppData/Roaming/UnityHub/GPUCache/data",
        "C:/Users/test/AppData/Roaming/UnityHub/TransportSecurity",
        // Unity Hub 顶层 log 文件（非 logs/ 子目录）：属配置状态，不碰
        "C:/Users/test/AppData/Roaming/UnityHub/000005.log",
        "C:/Users/test/AppData/Roaming/UnityHub/CURRENT",
        "C:/Users/test/AppData/Roaming/UnityHub/MANIFEST-000004",

        // ---- Unreal Engine 项目目录红线（.uproject 整体）----
        // 任何 .uproject 所在目录整体都不允许碰
        "C:/code/MyUEProject/MyUE.uproject",
        "C:/code/MyUEProject/Content/Levels/Main.umap",
        "C:/code/MyUEProject/Content/Blueprints/BP_Player.uasset",
        "C:/code/MyUEProject/Source/MyUE/MyUE.Build.cs",
        "C:/code/MyUEProject/Config/DefaultGame.ini",
        "C:/code/MyUEProject/Saved/Config/WindowsEditor/Game.ini",
        "C:/code/MyUEProject/Saved/Autosaves/Game/Levels/Main_Auto1.umap",
        "C:/code/MyUEProject/Intermediate/Build/Windows/MyUEEditor.xcodeproj",
        "C:/code/MyUEProject/.git/HEAD",

        // ---- Unreal Engine 引擎安装目录红线 ----
        "C:/Program Files/Epic Games/UE_5.3/Engine/Binaries/Win64/UnrealEditor.exe",
        "C:/Program Files/Epic Games/UE_5.3/Engine/Source/Runtime/Core/Private/Class.cpp",
        "C:/Program Files/Epic Games/UE_5.3/Engine/Content/Engine/Fonts/Roboto.uasset",
        "C:/Program Files/Epic Games/UE_5.3/Feature Packs/Getting Started/Content/Actor/Geometry/Plane.uasset",

        // ---- Unreal Engine Saved/Config 红线（用户配置）----
        // 本机实测：UnrealEngine/<version>/Saved/Config 是引擎自身配置，红线
        "C:/Users/test/AppData/Local/UnrealEngine/5.3/Saved/Config/WindowsEditor/EditorPerProjectUserSettings.ini",
        "C:/Users/test/AppData/Local/UnrealEngine/5.3/Saved/Config/WindowsEditor/GameUserSettings.ini",
        "C:/Users/test/AppData/Local/UnrealEngine/4.27/Saved/Config/WindowsEditor/Game.ini",
        // Unreal Common 下非 DDC 目录：Intermediate 谨慎——本 scaffold 不收
        "C:/Users/test/AppData/Local/UnrealEngine/Common/Intermediate/Build/foo",
        // Unreal 版本目录其它内容
        "C:/Users/test/AppData/Local/UnrealEngine/5.6/Saved/Logs/UE5.log",

        // ---- Godot 红线 ----
        // 编辑器全局配置：editor_settings.cfg、editor_layout.cfg 是红线
        "C:/Users/test/AppData/Roaming/Godot/editor_settings.cfg",
        "C:/Users/test/AppData/Roaming/Godot/editor_layout.cfg",
        "C:/Users/test/AppData/Roaming/Godot/editor_recent_fs",
        // Godot app_userdata/<project_name>/ 是每项目的用户数据，红线
        "C:/Users/test/AppData/Roaming/Godot/app_userdata/MyGame/save.slots",
        "C:/Users/test/AppData/Roaming/Godot/app_userdata/MyGame/settings.cfg",
        "C:/Users/test/AppData/Roaming/Godot/app_userdata/MyGame/screenshot.png",
        "C:/Users/test/AppData/Local/Godot/app_userdata/MyGame/save.slots",
        // 项目 .godot/ 关键文件（脚本缓存、导入资源元数据等）
        "C:/code/MyGodotProject/project.godot",
        "C:/code/MyGodotProject/.godot/editor/script_cache.bin",
        "C:/code/MyGodotProject/.godot/editor/scene_groups_cache.cfg",
        "C:/code/MyGodotProject/.godot/imported/texture.png-abc123.ctex",
        "C:/code/MyGodotProject/.godot/global_script_class_cache.cfg",
        "C:/code/MyGodotProject/.godot/uid_cache.bin",
        "C:/code/MyGodotProject/.godot/editor_state",
        // Godot 项目内资源
        "C:/code/MyGodotProject/res/scenes/main.tscn",
        "C:/code/MyGodotProject/res/scripts/player.gd",
        "C:/code/MyGodotProject/.git/HEAD",

        // ---- 通用红线样本（继承 CLAUDE.md）----
        "C:/Users/test/AppData/Roaming/Company/App/data.db",
        "C:/Users/test/AppData/Roaming/Company/App/data.db-wal",
        "C:/Users/test/AppData/Roaming/Company/App/data.db-shm",
        "C:/Users/test/AppData/Roaming/Tencent/WeChat/xwechat_files/wxid_abc/db_storage/MMKV/mmkv.db",
        "C:/Users/test/Documents/WeChat Files/wxid_abc/Msg/FileStorage/file.dat",
        "C:/Users/test/Documents/WeChat Files/wxid_abc/MultiMsg/msg.dat",
        "C:/Users/test/AppData/Roaming/Tencent/QQ/Accounts/12345/config.dat",
        "C:/Users/test/AppData/Roaming/Company/All Users/config.dat",
        "C:/Users/test/AppData/Roaming/Company/login/auth.dat",
        "C:/Users/test/AppData/Roaming/Company/config/settings.ini",
        "C:/Users/test/AppData/Roaming/Company/Favorite/1/file.dat",
        "C:/Users/test/AppData/Roaming/Company/Fav/item.dat",
        "C:/Users/test/AppData/Roaming/Company/key/secret.key",
        "C:/Users/test/AppData/Roaming/Company/crypto/secret.bin",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Login Data",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Cookies",
        "C:/Users/test/AppData/Local/Google/Chrome/User Data/Default/Bookmarks",
        "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc.default/cookies.sqlite",
        "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc.default/places.sqlite",
        "C:/Users/test/AppData/Roaming/Mozilla/Firefox/Profiles/abc.default/logins.json",
        "C:/Users/test/.ssh/id_rsa",
        "C:/Users/test/.ssh/id_ed25519",
        "C:/Users/test/.aws/credentials",
        "C:/Users/test/.config/gcloud/credentials.db",
        "C:/pagefile.sys",
        "C:/hiberfil.sys",
        "C:/$Recycle.Bin/S-1-5-21-1/file.dat",
        "C:/System Volume Information/catalog.dat",
        "C:/Users/test/AppData/Local/Docker/wsl/docker-desktop-data/data/ext4.vhdx",
        "C:/Users/test/AppData/Local/Packages/ubuntu/vhdx",
    ];

    let mut violations = Vec::new();
    for p in red_lines {
        let hits = matching_scopes(&scopes, p);
        if !hits.is_empty() {
            violations.push(format!("`{p}` -> {hits:?}"));
        }
    }
    assert!(
        violations.is_empty(),
        "engine-caches.toml glob hit red lines:\n  {}",
        violations.join("\n  ")
    );
}
