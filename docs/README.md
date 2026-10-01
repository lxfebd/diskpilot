# DiskPilot 文档

作者：[@lxfebd](https://github.com/lxfebd) · 仓库：[github.com/lxfebd/diskpilot](https://github.com/lxfebd/diskpilot)

| 文档 | 读者 | 内容 |
|---|---|---|
| [guide.md](guide.md) | 普通用户 | 使用说明书：安装、扫描、问 AI、清理、撤销、工具墙、MCP 接入、FAQ |
| [architecture.md](architecture.md) | 开发者 | 架构总设计：分层、扫描管线与内存预算、清理脚本体系、AI 层、发布管线、测试策略 |
| [scaffold-authoring.md](scaffold-authoring.md) | 贡献者 | 如何为新应用编写清理脚本（勘测 → TOML → 安全测试 → PR） |
| [health-butler.md](health-butler.md) | 产品/开发 | 定位升级方案：「预检 → 清单 → 一键确认」管家流程与 P0-P2 路线 |
| [plugin-market-index.md](plugin-market-index.md) | 插件作者/维护者 | 社区索引契约：schema 1 信封、签名模型、发布流程、建仓模板 |
| [publish-plugin-guide.md](publish-plugin-guide.md) | 插件作者 | 三步发布自己的清理插件（导出 zip → 填 index.json → 提 PR） |
| [PROJECT_FUNCTION_MAP.md](PROJECT_FUNCTION_MAP.md) | 开发者 | 功能地图：模块 ↔ 代码文件 ↔ 状态 |

其他文档入口：

- [README](../README.md) — 项目主页（下载、功能、安全模型）
- [crates/agent-server/README.md](../crates/agent-server/README.md) — MCP server 说明
- [.github/CONTRIBUTING.md](../.github/CONTRIBUTING.md) — 贡献总则
- 宣传网站：[website/index.html](../website/index.html)（可直接发布到 GitHub Pages）
