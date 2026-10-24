/**
 * R383：扫描（scan）是一次重 IO 的长任务，不允许并发跑两份。
 *
 * 后端 scan.rs 没有「已有扫描在进行」的拦截，两份并发会双倍读盘、scan-progress
 * 事件互相覆盖；更糟的是先结束的那份 finally 会把 scanning 置回 false，界面翻回
 * 「未扫描」，而另一份其实还在跑。
 *
 * 闸门只在真正要扫描前才取：命中扫描缓存的「秒开」路径不经过这里，所以扫描期间
 * 仍可以点盘符看已经扫好的树。盘条卡片、扫描全部、体检红盘格、救援横幅、页脚
 * 重扫共用同一个实例，后来者拿到 false 后提示「扫描正在进行中」即可。
 */
export class ScanGate {
  private running = false;

  /** 取闸：第一份拿到 true；已有扫描在跑时返回 false（调用方提示并放弃本轮）。 */
  tryEnter(): boolean {
    if (this.running) return false;
    this.running = true;
    return true;
  }

  /** 放闸：成功 / 失败 / 取消都要调用（走 finally），否则后续扫描永远进不来。 */
  leave(): void {
    this.running = false;
  }

  get busy(): boolean {
    return this.running;
  }
}
