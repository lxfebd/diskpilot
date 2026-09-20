# -*- coding: utf-8 -*-
"""agent-server.exe MCP newline-JSON 冒烟脚本（rmcp stdio 帧 = 换行分隔 JSON，非 Content-Length）。
用法: python scripts/mcp_smoke_newline.py [二进制路径]
流程: initialize -> notifications/initialized -> tools/list -> bench_disk 实测（dry_run=false, 64MB 临时目录）-> 守卫拒绝路径验证
"""
import json
import subprocess
import sys
import time

BIN = sys.argv[1] if len(sys.argv) > 1 else r"J:\DiskPilot\agent-server.exe"


def main():
    print(f"== spawn: {BIN}")
    p = subprocess.Popen(
        [BIN],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        bufsize=1,
    )

    def send(msg):
        line = (json.dumps(msg, ensure_ascii=False) + "\n").encode("utf-8")
        p.stdin.write(line)
        p.stdin.flush()

    def read_msg(timeout=90):
        """逐行读 stdout（newline-JSON 一帧一行）。Windows 管道无 select，用线程兜超时。"""
        import threading

        buf = []

        def worker():
            try:
                line = p.stdout.readline()
                if line:
                    buf.append(line)
            except Exception as e:  # noqa: BLE001
                buf.append(repr(e).encode())

        t = threading.Thread(target=worker, daemon=True)
        t.start()
        t.join(timeout)
        if not buf:
            return None
        raw = buf[0]
        try:
            return json.loads(raw.decode("utf-8", "replace"))
        except Exception as e:  # noqa: BLE001
            return {"_raw": raw.decode("utf-8", "replace"), "_err": str(e)}

    t0 = time.time()
    send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
          "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                     "clientInfo": {"name": "smoke", "version": "1.0"}}})
    init = read_msg(30)
    print(f"[initialize] {time.time()-t0:.1f}s ->", "OK" if init else "FAIL")
    if init:
        print("  protocol:", init.get("result", {}).get("protocolVersion"))
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    lst = read_msg(30)
    tools = lst.get("result", {}).get("tools", []) if lst else []
    print(f"[tools/list] {len(tools)} 个工具")
    names = [t.get("name") for t in tools]
    for probe in ["bench_disk", "bench_cpu", "bench_memory", "system_report", "disk_health",
                  "cleanup_suggestions", "file_recycle", "stress_test", "fan_selfheal_diag"]:
        print(f"  {'✓' if probe in names else '✗ 缺失'} {probe}")

    # bench_disk 真跑（dry_run=false, 64MB，普通用户临时目录）
    import tempfile
    tmp = tempfile.gettempdir()
    t0 = time.time()
    send({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
          "params": {"name": "bench_disk",
                     "arguments": {"path": tmp, "size_mb": 64, "dry_run": False}}})
    r = read_msg(120)
    dt = time.time() - t0
    if r is None:
        print(f"[bench_disk 真跑] 120s 超时无响应 —— 卡死！")
    else:
        txt = ""
        for c in r.get("result", {}).get("content", []):
            txt += c.get("text", "")
        print(f"[bench_disk 真跑] {dt:.1f}s 返回")
        print("  " + txt.replace("\n", "\n  ")[:600])

    # 守卫：盘根必须被拒绝（不真跑）
    t0 = time.time()
    send({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
          "params": {"name": "bench_disk",
                     "arguments": {"path": "C:\\", "size_mb": 16, "dry_run": False}}})
    r = read_msg(30)
    dt = time.time() - t0
    txt = ""
    for c in (r or {}).get("result", {}).get("content", []):
        txt += c.get("text", "")
    is_err = (r or {}).get("result", {}).get("isError")
    print(f"[bench_disk 盘根守卫] {dt:.1f}s isError={is_err}")
    print("  " + txt.replace("\n", "\n  ")[:300])

    p.kill()
    err = p.stderr.read().decode("utf-8", "replace")
    if err.strip():
        print("[stderr]", err.strip()[:400])


if __name__ == "__main__":
    main()
