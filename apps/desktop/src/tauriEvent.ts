// R387：Tauri 事件监听都是异步挂载的（listen() 返回 Promise），而组件卸载或
// effect 依赖重跑往往发生在 Promise settle 之前。老写法 `let un; listen(..).then(fn => un = fn);
// return () => un?.()` 在「先卸载、后 attach」时留下失去所有者的监听器：卸载后还在
// 收事件、依赖重跑后叠出第二份回调。R328 在 useAgentToolProgress 里修过一份，
// 这里把它收成可复用的 Guard + 三处共用的 hook（HwPanels / SystemTools / market）。
import { useEffect, useRef } from 'react';

/**
 * 事件监听的取消竞态安全壳：attach() 开始挂监听，cancel() 之后无论 listen()
 * 是否已经 settle，都会保证解绑——迟到返回的解绑函数会被立即调用。
 */
export class ListenerGuard<T> {
  private cancelled = false;
  private unlisten: (() => void) | undefined;

  /**
   * start 负责真正挂监听并返回解绑函数；handler 在 cancel 之后不再被调用。
   */
  async attach(
    start: (handler: (payload: T) => void) => Promise<() => void>,
    handler: (payload: T) => void,
  ): Promise<void> {
    const fn = await start((payload) => {
      if (!this.cancelled) handler(payload);
    });
    if (this.cancelled) {
      fn();
      return;
    }
    this.unlisten = fn;
  }

  /** 幂等：重复调用只解绑一次。 */
  cancel(): void {
    this.cancelled = true;
    const u = this.unlisten;
    this.unlisten = undefined;
    u?.();
  }
}

/**
 * 挂一个 Tauri 事件，组件卸载 / effect 依赖（event、enabled）变化时自动解绑。
 * handler 始终用最新一次渲染的闭包（内部 ref），不必为依赖稳定而记忆化。
 */
export function useTauriEvent<T>(event: string, handler: (payload: T) => void, enabled = true): void {
  const handlerRef = useRef(handler);
  useEffect(() => {
    handlerRef.current = handler;
  }, [handler]);

  useEffect(() => {
    if (!enabled) return;
    const guard = new ListenerGuard<T>();
    void (async () => {
      const mod = await import('@tauri-apps/api/event');
      await guard.attach(
        (h) => mod.listen<T>(event, (e) => h(e.payload)),
        (payload) => handlerRef.current(payload),
      );
    })();
    return () => guard.cancel();
  }, [event, enabled]);
}
