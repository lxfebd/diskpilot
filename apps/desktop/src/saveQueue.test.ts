import { describe, expect, it } from 'vitest';
import { createSaveQueue } from './saveQueue';

describe('串行保存队列（R446）', () => {
  it('按提交顺序逐个执行——后发的不会插到先发的前面', async () => {
    const queue = createSaveQueue();
    const order: string[] = [];
    const slow = queue(async () => {
      await new Promise((r) => setTimeout(r, 30));
      order.push('first');
      return 1;
    });
    const fast = queue(async () => {
      order.push('second');
      return 2;
    });
    const [a, b] = await Promise.all([slow, fast]);
    expect(order).toEqual(['first', 'second']);
    expect(a).toBe(1);
    expect(b).toBe(2);
  });

  it('前一个失败不卡住队列，后续任务照常执行', async () => {
    const queue = createSaveQueue();
    const bad = queue(async () => { throw new Error('boom'); });
    const good = queue(async () => 'ok');
    await expect(bad).rejects.toThrow('boom');
    await expect(good).resolves.toBe('ok');
  });

  it('任务尚未开始时排队，不会并行', async () => {
    const queue = createSaveQueue();
    let running = 0;
    let peak = 0;
    const job = () => async () => {
      running += 1;
      peak = Math.max(peak, running);
      await new Promise((r) => setTimeout(r, 5));
      running -= 1;
    };
    await Promise.all([queue(job()), queue(job()), queue(job())]);
    expect(peak).toBe(1);
  });

  it('每个队列实例互不影响', async () => {
    const q1 = createSaveQueue();
    const q2 = createSaveQueue();
    const order: string[] = [];
    const a = q1(async () => {
      await new Promise((r) => setTimeout(r, 20));
      order.push('q1');
    });
    const b = q2(async () => { order.push('q2'); });
    await Promise.all([a, b]);
    expect(order).toEqual(['q2', 'q1']);
  });
});

const RAW = import.meta.glob('./components/ReminderSettings.tsx', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

describe('提醒设置的写请求接线（R446）', () => {
  it('save 走串行队列，saving 指示按在途任务计数', () => {
    const src = String(RAW['./components/ReminderSettings.tsx'] ?? '');
    expect(src).toContain('const saveQueue = useRef(createSaveQueue());');
    expect(src).toContain('await saveQueue.current(() => api.setReminderConfig(patch))');
    expect(src).toContain('pendingSaves.current += 1;');
    expect(src).toContain('if (pendingSaves.current === 0) setSaving(false);');
    // 旧的并发写法（直接 await api.setReminderConfig）必须消失
    expect(src).not.toContain('const next = await api.setReminderConfig(patch);');
  });
});
