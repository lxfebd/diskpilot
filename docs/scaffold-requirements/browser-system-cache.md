# 浏览器 / 系统缓存（Windows）scaffold 需求

> 分类：浏览器缓存、系统临时文件、崩溃转储。
> 目标用户：Windows 日常使用电脑（缓存越积越多、磁盘越来越满）。

## browser-cache（Chrome / Edge 缓存）

**清理对象**

* `%LOCALAPPDATA%\Google\Chrome\User Data\<Profile>\` 下的：`Cache`、`Code Cache`、`GPUCache`、`GrShaderCache`、`DawnGraphiteCache`、`DawnWebGPUCache`、`ShaderCache`、`Service Worker/CacheStorage`

* Edge（同为 Chromium）`%LOCALAPPDATA%\Microsoft\Edge\User Data\<Profile>\` 下相同目录。

**为什么要整目录 recycle**
这些目录是浏览器运行时的可再生缓存，删除后浏览器自动重建；其中包含成千上万个小文件，file-by-file recycle 会产生海量回收站条目、极慢。用 `recycle_granularity = "directory"` 把每个缓存目录作为一个整体进回收站。

**红线（绝不触碰）**

* 书签：`<Profile>/Bookmarks`、`Bookmarks.bak`

* 密码 / 登录态：`<Profile>/Login Data*`、`<Profile>/Account*`、`<Profile>/key`、`<Profile>/crypto`

* 历史 / Cookie：`<Profile>/History`、`<Profile>/Cookies`

* 偏好 / 扩展：`<Profile>/Preferences`、`<Profile>/Secure Preferences`、`<Profile>/Extensions/**`

* 站点数据：`<Profile>/Local Storage/**`、`<Profile>/IndexedDB/**`、`<Profile>/Session Storage/**`

* 用户下载 / 桌面 / 文档：一律不在 glob 范围内。

这些红线以 `crates/scaffold/tests/browser_cache_safety.rs` 的**红线断言**形式固化。

**注意事项**

* 清理前需先关闭浏览器；否则部分正在被占用的文件会被自动跳过（这是执行器的预期行为，不报错）。

## system-temp（系统临时文件）

**清理对象**

* `%TEMP%`（即 `%LOCALAPPDATA%\Temp`）下的所有文件与子目录，默认只清 7 天前的（保留最近 7 天，避免命中正在使用的文件）。

**红线**

* `C:\Windows\Temp`（系统级，通常需管理员且可能正在使用）——不在 glob 内。

* 其他 AppData 目录（`Roaming`、`Local` 下除 Temp 外的目录）——不在 glob 内。

* 用户 Documents / Downloads / Desktop 及项目源码里的 `temp/` 目录——不在 glob 内。

红线断言：`crates/scaffold/tests/system_temp_safety.rs`。

## crash-dumps（崩溃转储 / 错误报告）

**清理对象**

* `%LOCALAPPDATA%\CrashDumps`（应用崩溃自动生成的 `.dmp`，默认保留 30 天）

* `%LOCALAPPDATA%\Microsoft\Windows\WER\{ReportQueue,ReportArchive}`（Windows 错误报告，默认保留 30 天）

* `C:\Windows\Minidump`（系统崩溃内核转储，默认保留 30 天；需要管理员权限）

**红线**

* `C:\Windows\System32\...`（包括 System32 下的 config/systemprofile 等）——不在 glob 内。

* 用户在 Documents 等位置**手动保存**的 dump 分析文件——不在 glob 内。

* `%ProgramData%\Microsoft\Windows\WER` 等非 `%LOCALAPPDATA%` 的 WER 副本——不在 glob 内。

红线断言：`crates/scaffold/tests/crash_dumps_safety.rs`。

## firefox-cache（Firefox 浏览器缓存）

**清理对象**

* `%LOCALAPPDATA%\Mozilla\Firefox\Profiles\<profile>\` 下的 `cache2/`、`Cache/`、`startupCache/`、`shader-cache/`。

* Unix 路径：`~/.mozilla/firefox/<profile>/` 下相同目录。

**红线（绝不触碰）**

* 书签 / 历史：`places.sqlite`（及其 `-wal/-shm`）

* 密码：`logins.json`、`key4.db`、`key3.db`

* Cookie：`cookies.sqlite`

* 表单历史：`formhistory.sqlite`

* 扩展：`extensions/**`

* 网站数据：`storage/**`、`containers.json`

* 用户下载 / 项目源码。

红线断言：`crates/scaffold/tests/firefox_cache_safety.rs`。

## dev-caches（开发缓存 npm / pnpm / Yarn / pip / Cargo）

**清理对象**

* npm：`%LOCALAPPDATA%\npm-cache`、`~/.npm/_cacache`

* pnpm：`%LOCALAPPDATA%\pnpm-cache`

* Yarn：`%LOCALAPPDATA%\Yarn\Cache`

* pip：`%LOCALAPPDATA%\pip\Cache`、`~/.cache/pip`

* Cargo：`~/.cargo/registry`（下载的 .crate 缓存）

**红线（绝不触碰）**

* `node_modules/**`（项目依赖）、`package.json`、任何项目源码

* Python 已安装环境：`site-packages/**`、`conda envs/**`

* `~/.cargo/bin`（已安装的可执行程序）、`.npmrc`、`.cargo/config.toml`

* 用户文档 / 下载。

红线断言：`crates/scaffold/tests/dev_caches_safety.rs`。

## steam-shadercache（Steam 着色器缓存）

**清理对象**

* `<任意 Steam 库>/steamapps/shadercache/**`（游戏运行时自动重建的着色器/管线缓存）

**红线（绝不触碰）**

* `steamapps/common/**`（游戏本体）

* `steamapps/workshop/**`（创意工坊内容）

* `userdata/**`（存档、截图、云同步、设置）

* `steamapps/appmanifest*.acf`（安装信息）

* Steam 自身的 `config/**`、`logs/**`、`htmlcache/**`

红线断言：`crates/scaffold/tests/steam_shadercache_safety.rs`。

## 验证命令

```bash
cargo run -p diskpilot-scaffold-lint -- scaffolds/*.toml
cargo test -p diskpilot-scaffold
```

