//! 风扇通道自修复（跨机器自适应诊断 + L2 写修复）。
//!
//! 目标：DiskPilot 在其他电脑上风扇控制等环境依赖功能也能正常使用；环境不匹配时
//! 内置 AI 能自动诊断并修复。本模块提供两个工具（由主项目 agent.rs 桥接层做 L2 确认门）：
//!
//! - `fan_selfheal_diag`（只读 L0）：调用 fancmd `diag json`，结构化返回
//!   驱动是否加载 / RTC 端口 / SuperIO 芯片 / env_base / 风扇列表 / 可写性，
//!   并给出明确的「可修复点」清单（缺驱动？芯片不可识别？可写但需 reset？）。
//! - `fan_selfheal_fix`（写 L2）：按诊断结果执行修复。当前支持：
//!   - `install_driver`：管理员权限下安装 inpoutx64 驱动（复制 DLL 到 System32 +
//!     `sc create` + `sc start`），装完自动 re-diag 验证。
//!   - `reset_fan <idx>`：恢复指定风扇为主板自动控制（安全可逆）。
//!   - `dry_run=true`：只出计划不执行（内置 AI 先 dry-run 再问用户确认执行）。
//!
//! 安全边界：
//! - 所有写操作都必须管理员权限（`whoami /groups` 存在 S-1-5-32-544），非管理员直接拒绝；
//! - 写操作发生在 System32（驱动 DLL 落地）与驱动服务（sc create/start）——
//!   由主项目权限中心 L2 + 确认门双重守护，AI 不能隐身执行；
//! - 绝不删除/覆盖任何已存在的驱动文件：若 System32 已存在 inpoutx64.dll，只补服务不覆盖；
//! - 只在目标是"让 fancmd 能写风扇"这一件事上动手，不做任何其他系统改动。
//!
//! fancmd 二进制由主项目打包在 `resources/fancmd/fancmd.exe`（打包）或
//! `tools/fancmd/bin/Release/net8.0/fancmd.exe`（开发）；本模块用
//! `DISKPILOT_TOOLS_DIR` 环境变量（hw.rs 查找时设置）定位，找不到则让 AI 先跑
//! `fan_selfheal_diag` 拿全局诊断。
//!
//! 2026-10-12 拆分：基础工具 → `sh_core`，诊断 → `sh_diag`，修复执行 → `sh_fix`。

mod sh_core;
mod sh_diag;
mod sh_fix;

pub use sh_diag::selfheal_diag;
pub use sh_fix::{fan_control, selfheal_fix};

// 温度域跨模块引用的基础符号。
pub(crate) use sh_core::{find_fancmd, is_admin};
