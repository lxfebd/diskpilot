// R455：刷新的「进行中 + 一次补跑」闸门。
//
// R441 的布尔闸门只做「在途就丢弃」：调用方在这期间改了拉取上限（历史页
// 「查看全部」把 limit 从 500 放宽到 5000），那次刷新会被静默丢掉——列表仍是
// 旧的 500 条，而截断横幅按 entries.length < limit 判定又会整个消失，用户以为
// 已经看全，连「查看全部」按钮都不见了。这里在「被拦下」时记一个补跑标记，
// 收尾时告诉调用方「再跑一次」，补跑用的是最新闭包里的上限。

export interface RefreshGate {
  /** 开始一次刷新：空闲则占位返回 true；在途则记下补跑并返回 false。 */
  tryStart: () => boolean;
  /** 收尾：返回 true 表示在途期间有过被拦下的刷新，调用方应立刻再跑一次。 */
  finish: () => boolean;
}

export function createRefreshGate(): RefreshGate {
  let running = false;
  let queued = false;
  return {
    tryStart() {
      if (running) {
        queued = true;
        return false;
      }
      running = true;
      queued = false;
      return true;
    },
    finish() {
      running = false;
      if (!queued) return false;
      queued = false;
      return true;
    },
  };
}
