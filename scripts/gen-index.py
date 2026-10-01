#!/usr/bin/env python3
"""生成插件市场社区索引 index.json（schema 1 信封）。

数据源 = crates/toolbelt/assets/tool_manifests.json 的真实工具元数据，
产物 = 契约兼容的信封（客户端 plugin_registry 的 RegistryEnvelope 语义：
schema=1、plugins[] 必填 id/name/version/url、url 必须 https 或留空表示待上架）。

用法（官方索引仓库建好后）：
    python tools/gen-index.py > index.json
或在本仓库产出样例：
    python tools/gen-index.py --out docs/index.example.json

发布者填直链：把插件 zip 传到 GitHub Releases 后，把 url/sha256/signer 填进条目。
"""
import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFESTS = ROOT / "crates/toolbelt/assets/tool_manifests.json"
DEFAULT_SOURCE_URL = "https://raw.githubusercontent.com/lxfebd/diskpilot-scaffolds-index/main/index.json"


def plugin_id(name: str) -> str:
    """名字净化成 kebab-case 插件 id（与 toolbelt plugin_id_from_name 同口径）。"""
    s = re.sub(r"[^a-zA-Z0-9]+", "-", name).strip("-").lower()
    return re.sub(r"-+", "-", s)


def build_envelope() -> dict:
    docs = json.loads(MANIFESTS.read_text(encoding="utf-8"))
    tools = docs["tools"]
    plugins = []
    for t in tools:
        plugins.append(
            {
                "id": plugin_id(t["name"]),
                "name": t["name"],
                "version": "1.0.0",
                "author": t.get("publisher", "社区"),
                "description": t.get("purpose", ""),
                "category": t.get("category", ""),
                "risk": t.get("risk", "low"),
                "tags": t.get("tags", []),
                # 发布者填直链；留空 = 待上架（seed 同口径）
                "url": "",
                "sha256": "",
                "signer": "",
                "downloads": 0,
                "versions": [],
                "license": "MIT",
                "depends_on": [],
                "verified": False,
                "homepage": "",
            }
        )
    return {
        "schema": 1,
        "source_url": DEFAULT_SOURCE_URL,
        "fetched_at": 0,
        "signature": "",
        "signer": "",
        "plugins": plugins,
    }


def validate(envelope: dict) -> None:
    """契约自检：与客户端 RegistryEnvelope 解析语义对齐。"""
    assert envelope["schema"] == 1, "schema 必须为 1（客户端 REGISTRY_SCHEMA）"
    assert len(envelope["plugins"]) > 0, "plugins[] 不能为空（客户端拒绝空索引）"
    for p in envelope["plugins"]:
        assert p["id"] and p["name"] and p["version"], "必填字段缺失"
        assert p["url"] == "" or p["url"].startswith("https://"), "url 必须 https 或留空待上架"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", help="输出路径；缺省写 stdout")
    args = ap.parse_args()

    env = build_envelope()
    validate(env)
    text = json.dumps(env, ensure_ascii=False, indent=2) + "\n"
    if args.out:
        Path(args.out).write_text(text, encoding="utf-8")
        print(f"已写 {args.out}（{len(env['plugins'])} 个插件，契约校验通过）")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
