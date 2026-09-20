// 铺开进度计量：把一个源文件里「还带着中文文案的代码行」数出来。
// 只用于守卫测试的棘轮（ratchet）闸，不参与运行时逻辑。
// 口径：去掉块注释与行尾 `//` 注释后，仍含汉字（含全角标点）的行数。
// 注释里的中文不算债——那是给人读的；字符串与 JSX 文本里的中文才算。

const CJK = /[㐀-䶿一-鿿＀-￯]/;

function stripBlockComments(code: string): string {
  return code.replace(/\/\*[\s\S]*?\*\//g, '');
}

/**
 * 去掉行尾 `//` 注释：逐字走一遍，只在**引号外**的第一个 `//` 处截断。
 * 早先按「`//` 前有没有未闭合引号」的正近似会在 `'a.png'],  // 图片` 这类
 * 偶数引号行上误判成字符串内，把行尾注释连同中文一起留下，虚报欠账。
 */
function dropLineComment(line: string): string {
  const trimmed = line.trimStart();
  if (trimmed.startsWith('//')) return '';
  let quote: string | null = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === '\\') i += 1;
      else if (c === quote) quote = null;
    } else if (c === '"' || c === "'" || c === '`') {
      quote = c;
    } else if (c === '/' && line[i + 1] === '/') {
      return line.slice(0, i);
    }
  }
  return line;
}

/** 去掉注释与 `@i18n-keep` 行，只留代码。守卫测试的两种扫描共用这一口径。 */
export function codeOnly(code: string): string {
  // 先按原始行剔除带 `@i18n-keep` 的行（标记可能写在 JSX 的 {/* */} 里，
  // 必须赶在去注释之前认，否则连标记本身一起被剥掉）。
  const kept = code
    .split('\n')
    .map((line) => (line.includes('@i18n-keep') ? '' : line))
    .join('\n');
  return stripBlockComments(kept)
    .split('\n')
    .map(dropLineComment)
    .join('\n');
}

/**
 * 还剩多少行硬编码中文文案。
 * 行尾标了 `@i18n-keep` 的行不计入 —— 用于「语言选择器里的『中文』」这类
 * 本来就该保持原生书写、或发给模型的中文 prompt，它们是数据不是待译文案。
 */
export function cjkLines(code: string): number {
  let n = 0;
  for (const line of codeOnly(code).split('\n')) {
    if (CJK.test(line)) n++;
  }
  return n;
}
