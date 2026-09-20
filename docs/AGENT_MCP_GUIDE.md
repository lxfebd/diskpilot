# 把 DiskPilot 的工具集接给其他 AI（MCP 接入指南）

> 面向：想把「这台电脑的 AI 可操作能力」接给任意 AI 客户端的人。
> 前提：已构建 `agent-server`（见文末「构建」一节）。

## 一、一句话原理

DiskPilot 的 75 个 AI 工具（64 只读 + 11 写）全部封装在 **`agent-server`** 一个可执行文件里。
它是标准的 **MCP（Model Context Protocol）stdio server**——任何支持 MCP 的 AI 客户端
（Claude Desktop / Claude Code / Cursor / 其他 agent / 自研程序）只要把它配成一个
`mcpServers` 条目，就能用自然语言让 AI 操作这台电脑：查磁盘、看进程、读温度、列服务、
盘点软件、出清理建议……全部走标准 `tools/list` + `tools/call` 通道。

## 二、给 AI 用的配置（核心）

### 通用 JSON 配置（MCP 标准格式，Claude / Cursor / 各类 agent 通用）

```json
{
  "mcpServers": {
    "diskpilot": {
      "command": "D:\\Tools\\DiskPilot\\agent-server.exe",
      "args": []
    }
  }
}
```

- `command`：`agent-server` 的**绝对路径**（Windows 下是 `.exe`）。路径只要不写错，
  无需任何环境变量——守卫判定全部从 Windows 环境变量自动推导。
- `args`：不需要参数。
- 安装版 DiskPilot 会把 `agent-server.exe` 装到安装目录（与主程序同目录），直接用那个路径即可。

### Claude Desktop

编辑 `claude_desktop_config.json`（`%APPDATA%\Claude\claude_desktop_config.json`）：

```json
{
  "mcpServers": {
    "diskpilot": {
      "command": "D:\\Tools\\DiskPilot\\agent-server.exe"
    }
  }
}
```

### Claude Code（CLI）

```bash
claude mcp add diskpilot -- "D:/Tools/DiskPilot/agent-server.exe"
```

### Cursor

Settings → MCP → Add server，`type: stdio`，`command: D:/Tools/DiskPilot/agent-server.exe`。

### 自研程序 / 脚本

直接 spawn 进程，走 MCP stdio 协议（换行分隔 JSON，非 Content-Length 帧）：

```python
import json, subprocess

p = subprocess.Popen([r"D:\Tools\DiskPilot\agent-server.exe"],
                     stdin=subprocess.PIPE, stdout=subprocess.PIPE)

def call(req_id, method, params):
    p.stdin.write((json.dumps({"jsonrpc": "2.0", "id": req_id, "method": method,
                               "params": params}) + "\n").encode())
    p.stdin.flush()
    while True:  # 响应按任意顺序到达，必须按 id 匹配
        obj = json.loads(p.stdout.readline())
        if obj.get("id") == req_id:
            return obj

call(0, "initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                       "clientInfo": {"name": "my-agent", "version": "1.0"}})
print(call(1, "tools/list", {}))
print(call(2, "tools/call", {"name": "disk_health", "arguments": {}}))
```

## 三、AI 能做什么（75 个工具一览）

**64 只读（L0，恒开无需确认）**：磁盘健康/分区/大文件/重复文件、进程/CPU 占用、
服务/驱动/启动项、CPU/GPU/内存/主板/温度/全量传感器/电池/SMART、网卡明细/WiFi
（绝不读密码）、防火墙/事件日志/登录审计/Defender/用户账户、回收站/环境变量、
软件与 Office 激活状态、Steam 游戏库、按 scaffold 分类的清理建议（只出建议不删文件）、
AIDA64 复刻基准（CPU/内存/磁盘/GPU）、传感器趋势/告警、DRAM 时序/CPUID 指令集/显示器、
蓝屏分析/内存检测/体检报告。

**11 写操作（需用户确认）**：`process_kill` / `process_start` / `file_recycle` /
`service_control` / `scheduled_task_manage` / `fan_selfheal_fix` / `fan_control` /
`uninstall_app` / `toolbelt_run` / `stress_test` / `stress_test_gpu`。写操作默认返回 `dry-run` 计划；真正执行前 AI 必须把要做的展示给用户、
用户同意后传 `confirmed=true` / `dry_run=false`。

> ⚠️ **写操作安全模型**：agent-server 侧自带三层守卫（路径守卫 `PathGuard`——拒绝盘根/
> 系统目录/主目录根；关键服务/系统任务/关键进程黑名单；卸载器白名单）。但**真正的
> 确认门在 DiskPilot 主程序**（`agent.rs` WRITE_TOOLS 层）。如果你把它接给**其他 AI 客户端**，
> 请确认该客户端也会在 AI 请求写操作时二次确认，或只在可信环境接入。

## 四、工具名速查（常用）

| 场景 | 工具 |
|---|---|
| 这盘多大/满了没 | `disk_health` / `disk_partition_usage` |
| 什么占了我的空间 | `disk_find_biggest_files` / `disk_top_directories` / `file_type_stats` |
| 电脑热不热 | `hw_temperature` / `hw_sensors` / `stress_test` |
| 谁在联网 | `net_connections` / `net_speed` |
| 开机启动了什么 | `sys_boot_items` |
| 装了哪些软件 | `app_list` / `app_licenses` |
| 这个文件夹能删吗 | `file_type_stats` / `cleanup_suggestions` |
| 有没有重复文件 | `disk_find_duplicate_files` |
| 硬件体检报告 | `system_report` / `hw_disk_smart` / `bench_cpu` / `bench_memory` / `bench_disk` |
| 蓝屏分析 | `bsod_analyze` |

完整清单与参数见 [TOOL_CATALOG.md](docs/tools-agent/TOOL_CATALOG.md)（参数以
`crates/agent-server/src/tools/mod.rs` 的代码为准）。

## 五、构建 / 获取 agent-server

**绝大多数情况不用自己构建**：安装包（NSIS 或 MSI）已经内置 `agent-server.exe`，
装完在安装目录里直接用（与 DiskPilot 主程序同目录）。首次打开 DiskPilot 后，
AI 工具通道（工具墙「AI 可操作」分区 / 磁盘工具等）就能直接连通。

需要从源码构建时才用：

```bash
# 仓库内构建（release，7.4MB）
cargo build -p agent-server --release
# 产物：target/release/agent-server.exe
```

DiskPilot 定位 `agent-server` 的顺序（`agent_server_binary()`）：

1. 环境变量 `DISKPILOT_AGENT_SERVER` 显式指定；
2. 主程序 exe 同目录（安装版形态）；
3. 从 exe 位置向上枚举 `<root>/target/{debug,release}/`（开发形态，release 优先）。

所以无论 dev 还是安装版，都不需要手工复制或设置环境变量。

## 六、验证接入成功

AI 客户端连上后，随便问一句「这台电脑的磁盘状况怎么样」——AI 会调用 `disk_health`、
`hw_disk_smart` 等工具并给出真实数据。若工具列表为空，检查 `command` 路径是否存在
（`agent-server.exe` 是否真的在那个位置）。
