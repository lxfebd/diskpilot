// R537 持久化通知中心单测
import { describe, expect, it } from 'vitest';
import { pushNotice, unreadCount, NOTICES_CAP, type Notice } from './notices';

const base = { kind: 'weekly' as const, title: '周报', body: '全盘可清理约 1 GB' };

describe('unreadCount（R542 未读角标）', () => {
  it('at 大于水位才算未读', () => {
    const list: Notice[] = [
      { id: 'a', at: 100, kind: 'weekly', title: 't', body: 'b' },
      { id: 'b', at: 300, kind: 'space-full', title: 't', body: 'b' },
      { id: 'c', at: 200, kind: 'scan-stale', title: 't', body: 'b' },
    ];
    expect(unreadCount(list, 250)).toBe(1);
    expect(unreadCount(list, 0)).toBe(3);
    expect(unreadCount(list, 999)).toBe(0);
    expect(unreadCount([], 0)).toBe(0);
  });
});

describe('pushNotice（R537 通知中心）', () => {
  it('新通知插到最前并带 id/at', () => {
    const list = pushNotice([], base, 1000);
    expect(list).toHaveLength(1);
    expect(list[0].id).toMatch(/^n1000-/);
    expect(list[0].at).toBe(1000);
    expect(list[0].kind).toBe('weekly');
  });
  it('超过上限截断到 NOTICES_CAP，旧的在后面被挤掉', () => {
    let list: Notice[] = [];
    for (let i = 0; i < NOTICES_CAP + 10; i++) list = pushNotice(list, base, i);
    expect(list).toHaveLength(NOTICES_CAP);
    expect(list[0].at).toBe(NOTICES_CAP + 9);
    expect(list[NOTICES_CAP - 1].at).toBe(10);
  });
  it('不修改原数组（不可变更新）', () => {
    const orig: Notice[] = [];
    const next = pushNotice(orig, base, 5);
    expect(orig).toHaveLength(0);
    expect(next).toHaveLength(1);
  });
});
