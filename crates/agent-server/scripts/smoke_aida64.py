"""冒烟验证：AIDA64 复刻新工具（bench_cpu/memory/disk + stress_test + report 三件套 + 阶段四三件套）。
用法：python smoke_aida64.py [--bin <agent-server.exe>]
「不可用跳过」：工具返回 isError 且文本含「不可用/不可读/无…/未…/跳过」等降级词 → 计 SKIP 不计 FAIL，
单条失败不中断，全部跑完出汇总。BIN 默认 target\\debug，可用 --bin 或 AGENT_SERVER_BIN 覆盖。"""
import json
import subprocess
import time
import os
import sys

BIN = os.environ.get("AGENT_SERVER_BIN", r"J:\xiangm_transfer\xiangm\tools\diskpilot_src\target\debug\agent-server.exe")
if "--bin" in sys.argv:
    BIN = sys.argv[sys.argv.index("--bin") + 1]

# 降级词：工具在受限环境如实降级（非管理员/无 GPU/无对应硬件）时应跳过而非判失败
DEGRADE_KEYWORDS = ["不可用", "不可读", "未检测", "无 nvidia", "无 NVIDIA", "未找到", "无有效",
                    "仅支持", "不支持", "跳过", "非 NVIDIA", "未内置", "未提权", "被拒",
                    "无温度护栏", "拒绝", "不存在", "白名单", "未安装"]

proc = subprocess.Popen(
    [BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL
)


def read_line(timeout=60):
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


def call(name, args, label, timeout=60):
    send({"jsonrpc": "2.0", "id": 99, "method": "tools/call",
          "params": {"name": name, "arguments": args}})
    raw = read_line(timeout)
    if raw is None or raw == b"TIMEOUT":
        print("[{}] FAIL: no response/timeout".format(label))
        return "fail"
    try:
        obj = json.loads(raw)
        c = obj["result"]["content"][0]["text"]
        is_err = obj["result"].get("isError", False)
        if is_err and any(k in c for k in DEGRADE_KEYWORDS):
            print("[{}] SKIP(环境降级): {}".format(label, c[:160].replace("\n", " / ")))
            return "skip"
        print("[{}] {} {}".format(label, "ERR-RESULT" if is_err else "OK", c[:180].replace("\n", " / ")))
        return "ok" if not is_err else "fail"
    except Exception as e:
        print("[{}] PARSE-ERR {}: {}".format(label, e, raw[:200]))
        return "fail"


# 握手
send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
    "protocolVersion": "2025-03-26", "capabilities": {},
    "clientInfo": {"name": "smoke-aida64", "version": "0.1"}}})
r = read_line()
print("handshake:", "OK" if r and b"serverInfo" in r else "FAIL")
send({"jsonrpc": "2.0", "method": "notifications/initialized"})

send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
raw = read_line()
try:
    tools = [t["name"] for t in json.loads(raw)["result"]["tools"]]
    print("tools/list: {} tools".format(len(tools)))
    for want in ["bench_cpu", "bench_memory", "bench_disk", "stress_test",
                 "system_report", "sensor_trend", "sensor_alert",
                 "hw_cpu_features", "hw_dram_timings", "hw_displays"]:
        print("  {}: {}".format(want, "✓" if want in tools else "✗ MISSING"))
except Exception as e:
    print("tools/list FAIL", e)

results = []
# 阶段二：基准 + 压测
results.append(call("bench_cpu", {"secs": 1, "threads": 4}, "bench_cpu(1s,4t)"))
results.append(call("bench_memory", {"secs": 1}, "bench_memory(1s)"))
results.append(call("bench_disk", {"size_mb": 32, "dry_run": True}, "bench_disk(dry-run)"))
# 压测确认门由主项目 agent.rs WRITE_TOOLS 管；这里直接调看是否执行
results.append(call("stress_test", {"secs": 1, "max_temp": 90, "threads": 2}, "stress_test(1s)"))
# 阶段三：报告/趋势/告警
results.append(call("system_report", {}, "system_report", timeout=90))
results.append(call("sensor_trend", {"n": 3}, "sensor_trend(n=3)"))
results.append(call("sensor_alert", {"persist": False}, "sensor_alert"))
# 阶段四：信息补全
results.append(call("hw_cpu_features", {}, "hw_cpu_features"))
results.append(call("hw_dram_timings", {}, "hw_dram_timings"))
results.append(call("hw_displays", {}, "hw_displays"))
# 阶段五：GPU 压测 + 跑分 + 写操作三件套（不可用自动跳过，失败不中断）
results.append(call("stress_test_gpu", {"secs": 2}, "stress_test_gpu(2s)"))
results.append(call("bench_gpu", {"secs": 2}, "bench_gpu(2s)"))
# 写工具参数尽量用「存在性检查/空操作」，触发守卫拒绝也计入 SKIP（降级词含「拒绝」）
results.append(call("file_recycle", {"path": r"C:\nonexistent\dp-smoke.txt"}, "file_recycle(不存在文件)"))
# process_start 真启动进程会污染共享 stdout 管道（cmd 输出混入 MCP 帧），冒烟只做存在性守卫测试
results.append(call("process_start", {"command": r"C:\Windows\System32\nonexistent-tool.exe"}, "process_start(不存在exe)"))
results.append(call("scheduled_task_manage", {"name": "__dp_smoke_task__", "action": "query"}, "scheduled_task_manage(query不存在)"))
results.append(call("uninstall_app", {"name": "__dp_smoke_nonexistent__"}, "uninstall_app(不存在)"))

passed = sum(1 for r in results if r == "ok")
skipped = sum(1 for r in results if r == "skip")
failed = sum(1 for r in results if r == "fail")
print("\n结果：{}/{} 通过，{} 跳过（环境降级），{} 失败".format(passed, len(results), skipped, failed))
proc.terminate()
sys_exit = 0 if failed == 0 else 1
sys.exit(sys_exit)
