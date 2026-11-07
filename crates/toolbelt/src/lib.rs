//! 图吧工具箱 CLI 工具目录（对齐 luolangaga/tubatools TubaWinUi3 的
//! CliToolboxCatalog + run_cli_tool 设计）。
//!
//! 权威参数表就是《CLI工具使用文档.md》本身（收录 17 个可命令行化调用的
//! 工具，路径/参数/示例/风险全部出自该文），编译期用 `include_str!` 内嵌，
//! 运行时解析——文档即目录，不需要额外维护一份可能过期的参数表。
//!
//! 核心出口：
//! - [`catalog`] / [`find`]：工具索引（分类 / 名称 / 简介 / exe 相对路径）
//! - [`usage`]：单个工具的完整用法（参数表 + 示例 markdown）
//! - [`toolbelt_command_for`]：把「工具名 + 参数」解析成可直接执行的命令
//!   （exe 绝对路径 / 实参 / 工作目录 / 风险级 / 超时）
//! - [`run`]：执行并捕获输出，超时强杀
//!
//! 工具目录旁可放 `toolbelt.toml` 覆盖默认值（schema 同 apps/desktop 的
//! 工具箱约定）：`risk = "low|medium|high"`、`args = [...]`（固定默认参数，
//! 调用方给了实参则被覆盖）、`usage = "..."`、`timeout_secs = 60`。
//!
//! # 模块布局（2026-10-12 巨型文件拆分）
//!
//! - [`risk`]：风险级枚举与默认值、超时常量
//! - [`catalog`]：类型（CliTool/ToolOverride/ToolbeltError/…）+ 文档解析与索引
//! - [`root`]：Tools 根目录定位（exe 上溯 / 图吧绿色版兜底）
//! - [`plugin`]：插件 zip 安装/导出 + 插件 id 净化
//! - [`run`]：命令解析（toolbelt_command_for）与执行（run/status）
//! - [`tree`]：全目录扫描工具墙（catalog_tree）
//! - [`manifest`]：工具清单（tool_manifests.json）解析
//! - [`signature`]：插件包 sha256/Ed25519 签名

pub mod catalog;
pub mod manifest;
pub mod plugin;
pub mod risk;
pub mod root;
pub mod run;
pub mod signature;
pub mod tree;

pub use manifest::{all_manifests, find_manifest, parser_for, ToolManifest, ToolMode};

/// 内嵌的权威 CLI 文档（源：tubatools 仓库根《CLI工具使用文档.md》）。
pub const EMBEDDED_DOC: &str = include_str!("../assets/cli_tools_doc.md");

// ── 对外 API 重导出（保持拆分前符号路径不变）────────────────────────────

pub use catalog::{
    catalog, find, usage, CliTool, ResolvedCommand, RunOutcome, ToolOverride, ToolPluginMeta,
    ToolPluginWeb, ToolbeltError,
};
pub use plugin::{
    ensure_path_under, export_plugin_zip, install_plugin_zip, plugin_id_from_name,
    validate_plugin_entry,
};
pub use risk::{Risk, MAX_TIMEOUT_SECS, MIN_TIMEOUT_SECS};
pub use root::find_tools_root;
pub use run::{
    grouped_index, parse_args, run, status, toolbelt_command_for, ToolSpec, ToolbeltStatus,
};
pub use tree::{catalog_tree, ArchVariantOut, CategoryOut, ToolbeltCatalog, ToolbeltCatalogItem};
