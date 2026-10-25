// R470：类名对偶守卫——组件里写的类名必须在样式表里有定义，否则就是拼错或漏写 CSS。
// 触发它的实证：.empty-sub 与 .xsmall 两处类名在 styles/*.css 里根本不存在，
// 空态副标题与风险提示小字因此继承正文字号（提示字比旁边 11px 的数字还大）。
// 文件名以 .test.ts 结尾，不进打包；读源码走 import.meta.glob ?raw（与 commands.test.ts 同口径）。
import { describe, expect, it } from "vitest";

const RAW_CSS = import.meta.glob("./styles/*.css", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const RAW_TSX = import.meta.glob("./**/*.tsx", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

/** 有意不写样式的结构钩子：视觉由同元素上的基础类或子元素承担，这枚类名只是排版/测试的锚点。 */
const ALLOW_UNSTYLED: string[] = [
  "ao-thermal-card", // .ao-card 承担卡片视觉，这枚只标记「散热」卡
  "cleanup-preflight-recheck", // 与 .ghost.small 同行，钩子留给「重新检测」按钮
  "mcp-form", // .mcp-row / .mcp-head-* 承担排版
  "reminder-result", // 组件内联 style 兜底
  "risk-low", // 只有 .risk-high 需要警示边框，低风险有意中性
  "rule-row", // 行内 flex 写在 style 上
  "sort-label", // 与 .muted.small 同行
  "sys-tools", // 页面容器，排版由子元素各自负责
  "tdm-args-list", // 详情卡分区包装，分区标题自己有样式
  "tdm-cli",
  "tdm-manifest",
  "tdm-similar",
  "toolwall-ai-perm", // 与 .chip 同行
  "toolwall-risk-chips", // 与 .sys-filter-chips 同行
  "toolwall-sort", // 与 .sys-filter-chips 同行
  "wh-delta", // 只有 .wh-delta-title / .wh-delta-list 需要样式
];

function stripCssComments(css: string): string {
  const parts = css.split("/*");
  const out: string[] = [parts[0]];
  for (const part of parts.slice(1)) {
    const end = part.indexOf("*/");
    out.push(end >= 0 ? part.slice(end + 2) : " ");
  }
  return out.join(" ");
}

/** 样式表里出现过的全部类名（.foo / .foo.bar / .foo:hover 都算）。 */
function definedClasses(): Set<string> {
  const out = new Set<string>();
  for (const file of Object.keys(RAW_CSS)) {
    const css = stripCssComments(RAW_CSS[file]);
    for (let i = 0; i < css.length; i += 1) {
      if (css[i] !== ".") continue;
      let name = "";
      let k = i + 1;
      while (k < css.length && /[A-Za-z0-9_-]/.test(css[k])) { name += css[k]; k += 1; }
      if (name !== "" && /^[A-Za-z]/.test(name)) out.add(name);
      i = k - 1;
    }
  }
  return out;
}

/** 组件里字面写出的类名（className 用引号或模板字面量的都扫；表达式形式是动态的，不扫）。 */
function usedClasses(): Map<string, string[]> {
  const out = new Map<string, string[]>();
  const KEY = "className=";
  for (const file of Object.keys(RAW_TSX)) {
    const src = RAW_TSX[file];
    const lineOf = (at: number) => src.slice(0, at).split(String.fromCharCode(10)).length;
    let i = src.indexOf(KEY);
    while (i >= 0) {
      const j = i + KEY.length;
      let raw = "";
      const q = src[j];
      if (q === '"' || q === "'") {
        let k = j + 1;
        while (k < src.length && src[k] !== q) { raw += src[k]; k += 1; }
      } else if (q === '`') {
        let k = j + 1;
        let depth = 0;
        while (k < src.length) {
          const ch = src[k];
          if (depth === 0 && ch === '`') break;
          if (ch === "$" && src[k + 1] === "{") { depth += 1; k += 2; continue; }
          if (depth > 0 && ch === "}") { depth -= 1; k += 1; continue; }
          if (depth === 0) raw += ch;
          k += 1;
        }
      }
      const flat = raw.split(String.fromCharCode(10)).join(" ").split(String.fromCharCode(13)).join(" ").split(String.fromCharCode(9)).join(" ");
      for (const tok of flat.split(" ")) {
        if (!/^[a-z][a-z0-9-]+$/.test(tok)) continue;
        if (!out.has(tok)) out.set(tok, []);
        const sites = out.get(tok)!;
        if (sites.length < 2) sites.push(file + ":" + lineOf(i));
      }
      i = src.indexOf(KEY, i + KEY.length);
    }
  }
  return out;
}

describe("类名与样式表的对偶（R470）", () => {
  it("解析器自己没空转（否则下面几条全绿也没意义）", () => {
    expect(Object.keys(RAW_CSS).length).toBeGreaterThanOrEqual(13);
    expect(Object.keys(RAW_TSX).length).toBeGreaterThanOrEqual(40);
    expect(definedClasses().size).toBeGreaterThanOrEqual(800);
    expect(usedClasses().size).toBeGreaterThanOrEqual(500);
  });

  it("组件里写的类名都有样式，或登记为结构钩子", () => {
    const defined = definedClasses();
    const allow = new Set(ALLOW_UNSTYLED);
    const offenders: string[] = [];
    for (const [name, sites] of usedClasses()) {
      if (defined.has(name) || allow.has(name)) continue;
      offenders.push(name + " @ " + sites.join(", "));
    }
    expect(offenders).toEqual([]);
  });

  it("结构钩子表没有过期项（登记了就必须还在用，且确实没有样式）", () => {
    const defined = definedClasses();
    const used = usedClasses();
    const stale: string[] = [];
    for (const name of ALLOW_UNSTYLED) {
      if (!used.has(name)) stale.push(name + "：已无人使用，删掉这行");
      if (defined.has(name)) stale.push(name + "：样式表里已经有它了，移出登记表");
    }
    expect(stale).toEqual([]);
  });

  it("空态副标题与提示小字现在有自己的字号（R470）", () => {
    const defined = definedClasses();
    expect(defined.has("empty-sub")).toBe(true);
    expect(defined.has("xsmall")).toBe(true);
    const layoutKey = Object.keys(RAW_CSS).find((k) => k.endsWith("layout.css"))!;
    const layout = RAW_CSS[layoutKey];
    expect(layout).toContain(".empty-sub { font-size: 12.5px;");
    expect(layout).toContain(".xsmall { font-size: 10.5px; }");
  });

  it("注释里的类名不算定义（防止守卫被注释骗过）", () => {
    const css = "/* .fake-class {} */ .real {}";
    expect(stripCssComments(css)).not.toContain(".fake-class");
    expect(stripCssComments(css)).toContain(".real");
  });
});
