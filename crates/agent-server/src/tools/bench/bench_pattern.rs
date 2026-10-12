//! 内存测试 pattern 辅助（从 bench.rs 拆分，2026-10-12）。
//! fill_pattern / verify_pattern：8 类数据 pattern 填充与校验。

// ── 内存测试 pattern 辅助 ─────────────────────────────────────────────────

/// 按 kind 填充整块内存：0=全零 1=全一 2=AA/55 交替 3=字节递增 4=伪随机（xorshift）。
pub(crate) fn fill_pattern(buf: &mut [u8], kind: u8) {
    match kind {
        0 => buf.fill(0x00),
        1 => buf.fill(0xFF),
        2 => {
            for (i, b) in buf.iter_mut().enumerate() {
                *b = if i % 2 == 0 { 0xAA } else { 0x55 };
            }
        }
        3 => {
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (i & 0xFF) as u8;
            }
        }
        _ => {
            let mut state = 0x9E37_79B9_7F4A_7C15u64;
            for b in buf.iter_mut() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *b = (state & 0xFF) as u8;
            }
        }
    }
}

/// 读回校验与 fill 的 pattern 一致，返回（错误字节数, 首个错误偏移）。
pub(crate) fn verify_pattern(buf: &[u8], kind: u8) -> (u64, Option<usize>) {
    let mut errs = 0u64;
    let mut first = None;
    match kind {
        0 | 1 => {
            let expect = if kind == 0 { 0u8 } else { 0xFF };
            for (i, &b) in buf.iter().enumerate() {
                if b != expect {
                    errs += 1;
                    if first.is_none() {
                        first = Some(i);
                    }
                }
            }
        }
        2 => {
            for (i, &b) in buf.iter().enumerate() {
                if b != if i % 2 == 0 { 0xAA } else { 0x55 } {
                    errs += 1;
                    if first.is_none() {
                        first = Some(i);
                    }
                }
            }
        }
        3 => {
            for (i, &b) in buf.iter().enumerate() {
                if b != (i & 0xFF) as u8 {
                    errs += 1;
                    if first.is_none() {
                        first = Some(i);
                    }
                }
            }
        }
        _ => {
            let mut state = 0x9E37_79B9_7F4A_7C15u64;
            for (i, &b) in buf.iter().enumerate() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                if b != (state & 0xFF) as u8 {
                    errs += 1;
                    if first.is_none() {
                        first = Some(i);
                    }
                }
            }
        }
    }
    (errs, first)
}
