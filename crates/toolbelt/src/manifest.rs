//! Tool Manifest：每个工具的能力说明书（purpose / invocation / output / risk /
//! examples 五段）。AI 路由、权限门、结构化解析都以它为准。
//!
//! 数据源：编译期内嵌 `assets/tool_manifests.json`（与 `cli_tools_doc.md` 同源，
//! 2026-08-12 生成）。参数表/示例/风险都出自权威 CLI 文档；`toolbelt.toml` 仍可
//! 在执行层覆盖 risk/args/timeout（优先级：toml > manifest > 文档默认）。

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// 工具调用方式：cli = 命令行可自动化（A 类）；gui = 纯图形界面（B 类，AI 不能
/// 直接操作，只能启动提示用户）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolMode {
    Cli,
    Gui,
}

/// 一个工具的能力说明书。字段名与 `tool_manifests.json` 一一对应。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolManifest {
    /// 工具名（与 CLI 目录 `find` 的名字一致；双名工具用「Autoruns / autorunsc」）。
    pub name: String,
    /// 所属分类（原版分类名）。
    pub category: String,
    /// 相对 Tools 根的 exe 路径（正斜杠）。
    pub exe_rel: String,
    /// 一句话用途（AI 判断「该不该用这个工具」）。
    pub purpose: String,
    pub when_to_use: String,
    pub when_not_to_use: String,
    pub invocation: ManifestInvocation,
    pub output: ManifestOutput,
    /// 静态风险级（"low" / "medium" / "high"，执行层再叠 toolbelt.toml 覆盖）。
    pub risk: String,
    /// 权限级别（"L0".."L3"）：L0 只读恒开；L1 每次确认；L2 高危需手动开启+确认；
    /// L3 永久禁止自动执行（如 FPT64 刷 BIOS）。
    pub permission_level: String,
    /// 副作用描述（AI 必须如实告知用户）。
    pub side_effects: String,
    pub examples: Vec<ManifestExample>,
    /// 开发商/发行者（对齐图吧工具箱 `tools.json` 的 publisher，2026-09 调研吸收）。
    #[serde(default)]
    pub publisher: String,
    /// 功能标签（对齐上游 tags，AI 检索/详情卡用）。
    #[serde(default)]
    pub tags: Vec<String>,
    /// 官方教程/文档页（优先各工具官方站，不用图吧教程站）。
    #[serde(default)]
    pub tutorial_url: String,
    /// 下载来源提示（官方下载/厂商站点；空 = 随图吧工具箱整包分发）。
    #[serde(default)]
    pub download_hint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestInvocation {
    pub mode: ToolMode,
    /// 能否直接启动/执行（GUI 工具可启动但不可脚本化）。
    pub launchable: bool,
    /// 参数模板：`{param}` 占位符会被替换为实际值（A 类全自动执行用）。
    /// 布尔参数为 `{name}` 单独占位，省略时整段剔除。
    pub args_template: String,
    pub params: Vec<ManifestParam>,
    /// 默认超时（秒）；执行层仍可被 toolbelt.toml / 调用方覆盖。
    pub timeout_secs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestParam {
    pub name: String,
    /// 参数本身的开关（如 `--max-time` / `/export`）；位置参数为空串。
    #[serde(default)]
    pub flag: String,
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
    pub desc: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestOutput {
    /// "stdout" / "file"（产物写盘，路径见 schema）。
    pub format: String,
    /// 解析器：smart_csv（带表头 CSV → JSON 行数组）/ kv_lines（key: value 行）/
    /// stdout（原样）。
    pub parser: String,
    /// 产物位置 / 表结构说明（AI 解读结果用）。
    pub schema: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestExample {
    pub args: String,
    pub desc: String,
    pub expect: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestsDoc {
    pub version: u64,
    pub tools: Vec<ToolManifest>,
}

/// 编译期内嵌的 Manifest 资产。
pub const EMBEDDED_MANIFESTS: &str = include_str!("../assets/tool_manifests.json");

/// 解析全部 manifest（进程级缓存：资产恒定）。
pub fn all_manifests() -> &'static [ToolManifest] {
    static CACHE: OnceLock<Vec<ToolManifest>> = OnceLock::new();
    CACHE.get_or_init(|| {
        serde_json::from_str::<ManifestsDoc>(EMBEDDED_MANIFESTS)
            .map(|d| d.tools)
            .unwrap_or_else(|e| {
                eprintln!("[diskpilot-toolbelt] tool_manifests.json 解析失败: {e}");
                Vec::new()
            })
    })
}

/// 按名字查找 manifest（不区分大小写，支持部分匹配，同 `find`）。
pub fn find_manifest(name: &str) -> Option<&'static ToolManifest> {
    let q = name.trim().to_ascii_lowercase();
    if q.is_empty() {
        return None;
    }
    all_manifests().iter().find(|m| {
        let n = m.name.to_ascii_lowercase();
        n == q || n.contains(&q) || q.contains(&n)
    })
}

/// 名字 → manifest 索引（一次算好，供前端/AI 注入用）。
pub fn manifest_index() -> HashMap<&'static str, &'static ToolManifest> {
    all_manifests()
        .iter()
        .map(|m| (m.name.as_str(), m))
        .collect()
}

/// 按 `output.parser` 选择的解析器名（smart_csv / kv_lines / stdout）。
pub fn parser_for(manifest: &ToolManifest) -> &str {
    &manifest.output.parser
}

// ── 输出解析器（W1）：把工具 stdout/文件产物解析成结构化 JSON ──────────
// AI 执行 CLI 工具后，把原始输出交给这些解析器转成 JSON，再解读。
// 解析失败/空输入一律返回原样文本（stdout 兜底），绝不 panic。

/// 按 parser 名解析一段文本：smart_csv → JSON 行数组；kv_lines → JSON 对象；
/// stdout → 原样字符串。
pub fn parse_output(text: &str, parser: &str) -> serde_json::Value {
    match parser {
        "smart_csv" => parse_smart_csv(text),
        "kv_lines" => parse_kv_lines(text),
        _ => serde_json::Value::String(text.to_string()),
    }
}

/// 带表头 CSV → `[{"列名": "值", ...}, ...]`。
/// 支持：逗号分隔、双引号包裹含逗号/换行的字段、空行跳过。
/// 表头行含 `\t` 时按制表符切（NirSoft /stext 系）。
pub fn parse_smart_csv(text: &str) -> serde_json::Value {
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    if lines.is_empty() {
        return serde_json::Value::String(text.to_string());
    }
    let header = split_csv_line(&lines.remove(0));
    if header.is_empty() {
        return serde_json::Value::String(text.to_string());
    }
    let rows: Vec<serde_json::Value> = lines
        .iter()
        .filter_map(|l| {
            let cols = split_csv_line(l);
            if cols.is_empty() {
                return None;
            }
            let mut obj = serde_json::Map::new();
            for (i, h) in header.iter().enumerate() {
                let key = h.trim().trim_matches('"').to_string();
                let val = cols
                    .get(i)
                    .cloned()
                    .unwrap_or_default()
                    .trim()
                    .trim_matches('"')
                    .to_string();
                if !key.is_empty() {
                    obj.insert(key, serde_json::Value::String(val));
                }
            }
            Some(serde_json::Value::Object(obj))
        })
        .collect();
    if rows.is_empty() {
        return serde_json::Value::String(text.to_string());
    }
    serde_json::Value::Array(rows)
}

/// 切分一行 CSV（处理双引号包裹字段）。分隔符：行内含 `\t` 用制表符，否则逗号。
fn split_csv_line(line: &str) -> Vec<String> {
    let sep = if line.contains('\t') { '\t' } else { ',' };
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_q && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_q = !in_q;
                }
            }
            c if c == sep && !in_q => {
                out.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// `key: value` / `key = value` 行 → JSON 对象（重复 key 后者覆盖）。
/// 供 HWiNFO 传感器日志、CrystalDiskInfo DiskInfo.txt 用。
pub fn parse_kv_lines(text: &str) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let idx = l.find(':').or_else(|| l.find('='));
        let Some(idx) = idx else { continue };
        let key = l[..idx].trim();
        let val = l[idx + 1..].trim();
        if key.is_empty() {
            continue;
        }
        // 数值优先转 number，便于 AI 解读
        let v = if let Ok(n) = val.parse::<i64>() {
            serde_json::Value::Number(n.into())
        } else if let Ok(f) = val.parse::<f64>() {
            serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::String(val.to_string()))
        } else {
            serde_json::Value::String(val.to_string())
        };
        obj.insert(key.to_string(), v);
    }
    if obj.is_empty() {
        serde_json::Value::String(text.to_string())
    } else {
        serde_json::Value::Object(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_csv_parses_header_and_rows() {
        let csv = "Path,Size,Folder\n\"C:\\a.txt\",1024,true\n\"C:\\b c.txt\",2048,false\n";
        let v = parse_smart_csv(csv);
        let rows = v.as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["Path"], "C:\\a.txt");
        assert_eq!(rows[0]["Size"], "1024");
        assert_eq!(rows[1]["Path"], "C:\\b c.txt");
    }

    #[test]
    fn smart_csv_falls_back_on_empty() {
        let v = parse_smart_csv("  \n");
        assert!(v.is_string(), "空输入应回落为字符串");
    }

    #[test]
    fn kv_lines_parses_numbers_and_strings() {
        let txt = "Temperature: 52\nVoltage: 1.35\nModel = ABC 123\n";
        let v = parse_kv_lines(txt);
        assert_eq!(v["Temperature"], 52);
        assert_eq!(v["Voltage"], 1.35);
        assert_eq!(v["Model"], "ABC 123");
    }

    #[test]
    fn parse_output_dispatches_by_parser() {
        assert!(parse_output("a,b\n1,2\n", "smart_csv").is_array());
        assert!(parse_output("k: v\n", "kv_lines").is_object());
        assert!(parse_output("raw", "stdout").is_string());
    }
}
