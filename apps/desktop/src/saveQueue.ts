// R446：串行保存队列。
// 提醒设置的分段控件每点一下就把一个 patch 发给后端；快速连点时多个请求并发，
// 到达/处理顺序与点击顺序不一定一致，后端最终落的是「最后一个处理完的」而不是
// 「用户最后点的」，界面也会被先发的旧响应覆盖回旧选择。这里把写请求排成一条链：
// 上一个完成（成功或失败）才发下一个，用户最后一次点击必然最后落盘。

export type SaveQueue = <T>(task: () => Promise<T>) => Promise<T>;

/** 创建一个串行执行器：任务按提交顺序逐个跑，前一个失败不阻断后一个。 */
export function createSaveQueue(): SaveQueue {
  let tail: Promise<unknown> = Promise.resolve();
  return <T>(task: () => Promise<T>): Promise<T> => {
    // then(task, task)：前一个失败也要继续跑下一个，不能把整条链卡死。
    const run = tail.then(task, task);
    // 链尾吞掉结果，避免未处理的 rejection；调用方拿的是 run 本身。
    tail = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  };
}
