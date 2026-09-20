"""全量冒烟测试：75 个 MCP 工具逐一调用（2026-09-17）。
覆盖：无参工具 / 需路径工具 / bench 基准（短时） / 写操作（dry-run 或安全跳过）。

用法：python smoke_full.py
注意：写操作工具默认跳过（会改系统状态），bench 系列跑短时（3-5s）。
"""
import json
import subprocess
import sys
import time

BIN = r"J:\xiangm_transfer\xiangm\tools\diskpilot_src\target\release\agent-server.exe"

proc = subprocess.Popen(
    [BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL
)

def read_line(timeout=35):
    buf = b""
    end = time.time() + timeout
    while time.time() < end:
        chunk = proc.stdout.read(1)
        if not chunk:
            return None
        buf += chunk
        if buf.endswith(b"\n"):
            return buf
    return b"TIMEOUT"

def send(obj):
    proc.stdin.write((json.dumps(obj) + "\n").encode("utf-8"))
    proc.stdin.flush()

def call(name, args, label, timeout=35):
    send({"jsonrpc": "2.0", "id": 99, "method": "tools/call",
          "params": {"name": name, "arguments": args}})
    raw = read_line(timeout)
    if raw is None or raw == b"TIMEOUT":
        # 超时：清空残留响应，避免污染下一次读取（该工具响应可能晚到）
        drain()
        print("[{}] TIMEOUT(>{}s)".format(label, timeout))
        return "TIMEOUT"
    try:
        obj = json.loads(raw)
        c = obj["result"]["content"][0]["text"]
        is_err = obj["result"].get("isError", False)
        # 单行化：去掉换行，截断
        one = c.replace("\n", " / ")[:180]
        print("[{}] {} {}".format(label, "ERR" if is_err else "OK", one))
        return "ERR" if is_err else "OK"
    except Exception as e:
        print("[{}] PARSE-ERR {}: {}".format(label, e, raw[:200]))
        return "PARSE_ERR"

def drain():
    """超时后把管道里残留的行全部读掉，防止错位污染下一次读取。"""
    proc.stdout.flush()
    # 非阻塞读干净：最多读 2s
    end = time.time() + 2
    while time.time() < end:
        chunk = proc.stdout.read(1)
        if not chunk:
            return
        if chunk == b"\n":
            return

# 握手
send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
    "protocolVersion": "2025-03-26", "capabilities": {},
    "clientInfo": {"name": "smoke", "version": "0.1"}}})
r = read_line()
print("handshake:", "OK" if r and b"serverInfo" in r else "FAIL")
send({"jsonrpc": "2.0", "method": "notifications/initialized"})

# 无参工具（只读，直接调）
no_args = [
    "disk_health", "disk_volume_meta", "system_info", "list_processes",
    "app_list", "app_licenses", "hw_cpu", "hw_gpu", "hw_memory", "hw_motherboard",
    "hw_temperature", "hw_superio", "hw_sensors", "hw_battery", "hw_disk_smart",
    "net_status", "net_share", "net_wifi", "net_adapter_detail", "sys_services",
    "sys_drivers", "sys_boot_items", "steam_games", "process_cpu_usage", "usb_devices",
    "disk_smart_raw_attributes", "security_event_logs", "security_firewall_rules",
    "security_login_events", "sys_defender_status", "sys_user_accounts",
    "recycle_bin_stats", "env_vars", "disk_io_usage", "bsod_analyze", "system_report",
    "sensor_trend", "sensor_alert", "hw_cpu_features", "hw_dram_timings", "hw_displays",
    "bench_gpu", "hw_ipmi", "hw_acpi", "toolbelt_list", "fan_selfheal_diag",
    "stress_cancel", "net_connections", "net_speed",
]

# 需路径 / 有参工具（合理默认参数；System32/ProgramData/盘根在系统黑名单是设计如此，
# 用 C:/Users/Public 公开目录测只读枚举工具）
with_args = [
    ("disk_partition_usage", {"path": r"C:\Windows"}),
    ("file_type_stats", {"path": r"C:\Users\Public"}),
    ("file_tree", {"path": r"C:\Users\Public", "max_depth": 2, "max_nodes": 30}),
    ("list_dir", {"path": r"C:\Users\Public"}),
    ("find_files", {"root": r"C:\Users\Public", "keyword": "Microsoft"}),
    ("process_info", {"pid": 4}),
    # 清理建议：整盘 C:/ 全量 walk 实测 100s（agent-server 无缓存，单遍扫盘），
    # 这里用短路径快速验证链路；全盘口径由用户在主界面触发。
    ("cleanup_suggestions", {"root": r"C:\Windows\Temp", "top_n": 10}),
    ("disk_top_directories", {"path": r"C:\Users\Public", "top_n": 10}),
    ("disk_find_biggest_files", {"path": r"C:\Users\Public", "top_n": 5}),
    ("disk_find_duplicate_files", {"path": r"C:\Users\Public", "top_n": 3}),
    ("scheduled_task_manage", {"name": "Microsoft\\Windows\\Defrag\\ScheduledDefrag", "action": "query", "dry_run": True}),
    ("sys_drivers", {"keyword": "inpout"}),
    ("sys_services", {"keyword": "win"}),
    ("net_connections", {"top_n": 8}),
    ("net_speed", {"interval_ms": 500}),
    ("process_cpu_usage", {}),
    ("fan_control", {"idx": "0", "pct": 40, "dry_run": True}),
    # fan_selfheal_fix 只支持 install_driver / reset_fan <idx>；诊断用 fan_selfheal_diag
    ("fan_selfheal_fix", {"action": "reset_fan 0", "dry_run": True}),
    ("toolbelt_run", {"tool": "crystaldiskinfo"}),
    ("mem_test", {"size_mb": 8, "rounds": 1}),
    ("bench_cpu", {"secs": 3}),
    ("bench_memory", {"secs": 3}),
    ("bench_disk", {"path": r"C:\Windows\Temp".replace("\\", "/"), "size_mb": 16, "dry_run": True}),
    ("stress_test", {"secs": 3, "max_temp": 95}),
    ("stress_test_gpu", {"secs": 3, "max_temp": 90}),
    ("sensor_trend", {"format": "json", "n": 5}),
    ("sensor_alert", {"persist": False}),
    ("read_file", {"path": r"C:\Windows\win.ini"}),
    ("file_recycle", {"path": r"C:\Windows\win.ini"}),  # 只读守卫预期拒绝
]

# 写操作（有真实副作用，跳过或确认门拒绝验证）
write_tools = ["process_kill", "process_start", "uninstall_app", "service_control"]

results = {"OK": 0, "ERR": 0, "TIMEOUT": 0, "PARSE": 0, "SKIP": 0}
detail = []

print("\n===== 无参只读工具（%d 个）=====" % len(no_args))
for t in no_args:
    r = call(t, {}, t)
    results[r if r in results else "PARSE"] += 1
    detail.append((t, r))

print("\n===== 有参工具（%d 个）=====" % len(with_args))
for t, args in with_args:
    r = call(t, args, t)
    results[r if r in results else "PARSE"] += 1
    detail.append((t, r))

print("\n===== 写操作（%d 个，跳过——需用户确认）=====" % len(write_tools))
for t in write_tools:
    print("[{}] SKIP".format(t))
    results["SKIP"] += 1
    detail.append((t, "SKIP"))

print("\n===== 汇总 =====")
for k, v in results.items():
    print("{}: {}".format(k, v))
total = sum(v for k, v in results.items() if k != "SKIP")
print("实际调用 {} 个，OK {} / ERR {} / TIMEOUT {}".format(total - results["SKIP"], results["OK"], results["ERR"], results["TIMEOUT"]))
if results["ERR"] or results["TIMEOUT"]:
    print("\n失败列表：")
    for t, r in detail:
        if r in ("ERR", "TIMEOUT", "PARSE_ERR"):
            print("  {} -> {}".format(t, r))
proc.terminate()