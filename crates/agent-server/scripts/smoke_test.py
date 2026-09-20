"""冒烟测试：通过 stdio 管道走 MCP 协议调用全部 33 个工具 + 安全守卫。用法：python smoke_test.py"""
import json
import subprocess
import sys
import time

BIN = r"J:\xiangm_transfer\xiangm\tools\diskpilot_src\target\debug\agent-server.exe"
BS = chr(92)
CWIN = "C:" + BS + "Windows"
DRV = CWIN + BS + "System32" + BS + "drivers"

proc = subprocess.Popen(
    [BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL
)


def read_line(timeout=30):
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
    proc.stdin.write((json.dumps(obj) + "\n").encode())
    proc.stdin.flush()


def call(name, args, label):
    send({"jsonrpc": "2.0", "id": 99, "method": "tools/call",
          "params": {"name": name, "arguments": args}})
    raw = read_line()
    if raw is None or raw == b"TIMEOUT":
        print("[{}] FAIL: no response".format(label))
        return False
    try:
        obj = json.loads(raw)
        c = obj["result"]["content"][0]["text"]
        is_err = obj["result"].get("isError", False)
        print("[{}] {} {}".format(label, "ERR-RESULT" if is_err else "OK", c[:220].replace("\n", " / ")))
        return not is_err
    except Exception as e:
        print("[{}] PARSE-ERR {}: {}".format(label, e, raw[:200]))
        return False


# 握手
send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
    "protocolVersion": "2025-03-26", "capabilities": {},
    "clientInfo": {"name": "smoke", "version": "0.1"}}})
r = read_line()
print("handshake:", "OK" if r and b"serverInfo" in r else "FAIL")
send({"jsonrpc": "2.0", "method": "notifications/initialized"})

# 工具清单
send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
raw = read_line()
try:
    tools = [t["name"] for t in json.loads(raw)["result"]["tools"]]
    print("tools/list: {} tools -> {}".format(len(tools), tools))
except Exception as e:
    print("tools/list FAIL", e)

results = []
results.append(call("disk_health", {}, "disk_health"))
results.append(call("disk_partition_usage", {"path": CWIN}, "disk_partition_usage"))
results.append(call("disk_volume_meta", {}, "disk_volume_meta"))
results.append(call("file_type_stats", {"path": DRV, "top_n": 5}, "file_type_stats"))
results.append(call("file_tree", {"path": DRV, "max_depth": 2, "max_nodes": 10}, "file_tree"))
results.append(call("system_info", {}, "system_info"))
results.append(call("list_dir", {"path": DRV, "limit": 5}, "list_dir"))
results.append(call("read_file", {"path": CWIN + BS + "win.ini"}, "read_file"))
results.append(call("find_files", {"root": DRV, "keyword": "sys", "max_hits": 5}, "find_files"))
results.append(call("list_processes", {"top_n": 5}, "list_processes"))
results.append(call("process_info", {"pid": 4}, "process_info"))
results.append(call("app_list", {"limit": 3}, "app_list"))

# ── 硬件（hw.rs 7 个）─────────────────────────────────────────────
results.append(call("hw_cpu", {}, "hw_cpu"))
results.append(call("hw_gpu", {}, "hw_gpu"))
results.append(call("hw_memory", {}, "hw_memory"))
results.append(call("hw_motherboard", {}, "hw_motherboard"))
results.append(call("hw_temperature", {}, "hw_temperature"))
results.append(call("hw_battery", {}, "hw_battery"))
results.append(call("hw_disk_smart", {}, "hw_disk_smart"))

# ── 网络（net.rs 5 个）────────────────────────────────────────────
results.append(call("net_status", {}, "net_status"))
results.append(call("net_connections", {"top_n": 5}, "net_connections"))
results.append(call("net_speed", {"interval_ms": 1000}, "net_speed"))
results.append(call("net_share", {}, "net_share"))
results.append(call("net_wifi", {}, "net_wifi"))

# ── 系统服务/驱动/启动（sys.rs 3 个）──────────────────────────────
results.append(call("sys_services", {"top_n": 5}, "sys_services"))
results.append(call("sys_drivers", {"top_n": 5}, "sys_drivers"))
results.append(call("sys_boot_items", {}, "sys_boot_items"))

# ── 安全/审计（sec.rs 3 个）───────────────────────────────────────
results.append(call("security_event_logs", {"hours": 24, "max_events": 3}, "security_event_logs"))
results.append(call("security_firewall_rules", {"top_n": 3}, "security_firewall_rules"))
results.append(call("security_login_events", {"max_events": 3}, "security_login_events"))

# ── 磁盘深入（diskx.rs 4 个）──────────────────────────────────────
results.append(call("disk_find_biggest_files", {"path": DRV, "top_n": 5}, "disk_find_biggest_files"))
results.append(call("disk_find_duplicate_files", {"path": DRV, "top_n": 3}, "disk_find_duplicate_files"))
results.append(call("disk_io_usage", {"sample_ms": 1000}, "disk_io_usage"))
results.append(call("disk_top_directories", {"path": CWIN + BS + "Temp", "top_n": 5}, "disk_top_directories"))

# ── 新增（steam/cleanup/process_cpu/usb/smart_raw，2026-09-09）────────
results.append(call("steam_games", {"top_n": 5}, "steam_games"))
results.append(call("cleanup_suggestions", {"root": CWIN + BS + "Temp", "top_n": 3}, "cleanup_suggestions"))
results.append(call("process_cpu_usage", {}, "process_cpu_usage"))
results.append(call("usb_devices", {}, "usb_devices"))
results.append(call("disk_smart_raw_attributes", {}, "disk_smart_raw_attributes"))
results.append(call("recycle_bin_stats", {}, "recycle_bin_stats"))
results.append(call("env_vars", {"name": "PATH"}, "env_vars"))
results.append(call("env_vars", {}, "env_vars_common"))

# 安全守卫：盘根必须被拒绝（isError=true）
send({"jsonrpc": "2.0", "id": 98, "method": "tools/call",
      "params": {"name": "list_dir", "arguments": {"path": "C:" + BS}}})
raw = read_line()
guard_ok = raw is not None and raw != b"TIMEOUT" and b'"isError":true' in raw
print("[guard C:\\] {} : {}".format(
    "OK 拒绝盘根" if guard_ok else "FAIL 未拦截", (raw or b"")[:160]))
results.append(guard_ok)

# ── 写操作工具（process_kill / service_control）守护断言（2026-09-10）─────
# 这两个工具会改变系统状态，但**守卫必须先拦**——绝不能被 AI 误用于系统关键目标。
# - process_kill PID 4（System）必须被拒绝（isError=true）
# - service_control 不存在的服务必须被拒绝（isError=true）
# 真实可逆往返（stop→start 用户态服务）由主项目集成测试覆盖，不在冒烟里真停服务。
guard_write_ok = True

# 1) process_kill 系统关键 PID
send({"jsonrpc": "2.0", "id": 97, "method": "tools/call",
      "params": {"name": "process_kill", "arguments": {"pid": 4}}})
raw = read_line()
decoded = raw.decode("utf-8", errors="replace") if raw and raw != b"TIMEOUT" else ""
ok = b'"isError":true' in (raw or b"") and "系统关键" in decoded
print("[guard process_kill 4] {} : {}".format(
    "OK 拒绝系统进程" if ok else "FAIL 未拦截系统进程", decoded[:160]))
guard_write_ok = guard_write_ok and ok

# 2) service_control 不存在的服务
send({"jsonrpc": "2.0", "id": 96, "method": "tools/call",
      "params": {"name": "service_control", "arguments": {"name": "nonexistent_svc_xyz", "action": "stop"}}})
raw = read_line()
decoded = raw.decode("utf-8", errors="replace") if raw and raw != b"TIMEOUT" else ""
ok = b'"isError":true' in (raw or b"") and "不存在" in decoded
print("[guard service_control] {} : {}".format(
    "OK 拒绝不存在服务" if ok else "FAIL 未拦截不存在服务", decoded[:160]))
guard_write_ok = guard_write_ok and ok

results.append(guard_write_ok)

# ── 卸载助手 uninstall_app 守卫断言（2026-09-10，R5）─────────────
# 卸载器会真改系统，冒烟只验证守卫**先拦**——绝不能让 AI 乱启动卸载器。
# - 未知程序 → 拒绝（isError=true）
# - 非白名单目录卸载器 / 非白名单静默参数 → 拒绝（isError=true）
# 真实卸载不在冒烟里执行（会拉起卸载器 UI）。
guard_uninstall_ok = True

# 1) 未知程序
send({"jsonrpc": "2.0", "id": 95, "method": "tools/call",
      "params": {"name": "uninstall_app", "arguments": {"name": "不存在的程序XYZ"}}})
raw = read_line()
decoded = raw.decode("utf-8", errors="replace") if raw and raw != b"TIMEOUT" else ""
ok = b'"isError":true' in (raw or b"") and "未找到已安装程序" in decoded
print("[guard uninstall_app 未知程序] {} : {}".format(
    "OK 拒绝未知程序" if ok else "FAIL 未拦截未知程序", decoded[:160]))
guard_uninstall_ok = guard_uninstall_ok and ok

# 2) 白名单外的卸载器目录（如 powershell 这类不在已安装目录白名单的）
send({"jsonrpc": "2.0", "id": 94, "method": "tools/call",
      "params": {"name": "uninstall_app", "arguments": {"name": "Bun"}}})
raw = read_line()
decoded = raw.decode("utf-8", errors="replace") if raw and raw != b"TIMEOUT" else ""
ok = b'"isError":true' in (raw or b"") and "不在已安装程序目录白名单" in decoded
print("[guard uninstall_app 非白名单目录] {} : {}".format(
    "OK 拒绝非白名单卸载器" if ok else "FAIL 未拦截非白名单卸载器", decoded[:160]))
guard_uninstall_ok = guard_uninstall_ok and ok

# 3) 非白名单静默参数（如 Photoshop 的 --uninstall=1）
# 注意：具体拒绝理由随机器已装程序而异（未找到该程序 / 卸载器目录不在白名单 /
# 参数不在静默白名单 / 卸载器文件不存在）。本用例目标是验证写工具守卫**必然拦截**，
# 不断言具体理由，只要求 isError=true 且文本包含「拒绝」。
send({"jsonrpc": "2.0", "id": 93, "method": "tools/call",
      "params": {"name": "uninstall_app", "arguments": {"name": "Adobe Photoshop 2024"}}})
raw = read_line()
decoded = raw.decode("utf-8", errors="replace") if raw and raw != b"TIMEOUT" else ""
ok = b'"isError":true' in (raw or b"") and "拒绝" in decoded
print("[guard uninstall_app 非白名单参数] {} : {}".format(
    "OK 拒绝非白名单参数" if ok else "FAIL 未拦截非白名单参数", decoded[:160]))
guard_uninstall_ok = guard_uninstall_ok and ok

results.append(guard_uninstall_ok)

# ── 风扇调速 fan_control 守卫断言（2026-09-11）────────────────────
# 写工具：dry_run=true 只出计划不执行（安全验证）；dry_run=false 会真调速，
# 冒烟不执行（会接管风扇）。核心断言：dry_run 返回计划且不含错误。
fan_control_ok = True

send({"jsonrpc": "2.0", "id": 92, "method": "tools/call",
      "params": {"name": "fan_control", "arguments": {"idx": "0", "pct": 60, "dry_run": True}}})
raw = read_line()
decoded = raw.decode("utf-8", errors="replace") if raw and raw != b"TIMEOUT" else ""
ok = ("dry_run" in decoded) and ("fancmd write" in decoded)
print("[guard fan_control dry_run] {} : {}".format(
    "OK 只出计划不执行" if ok else "FAIL 未返回计划", decoded[:160]))
fan_control_ok = fan_control_ok and ok

# 非法索引（非数字）→ 拒绝
send({"jsonrpc": "2.0", "id": 91, "method": "tools/call",
      "params": {"name": "fan_control", "arguments": {"idx": "abc", "pct": 60, "dry_run": True}}})
raw = read_line()
decoded = raw.decode("utf-8", errors="replace") if raw and raw != b"TIMEOUT" else ""
ok = b'"isError":true' in (raw or b"") and "非法" in decoded
print("[guard fan_control 非法索引] {} : {}".format(
    "OK 拒绝非法索引" if ok else "FAIL 未拦截非法索引", decoded[:160]))
fan_control_ok = fan_control_ok and ok

results.append(fan_control_ok)

print("== SUMMARY: {}/{} PASS ==".format(sum(results), len(results)))
proc.kill()
sys.exit(0 if all(results) else 1)
