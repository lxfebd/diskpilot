import { describe, it, expect, vi } from 'vitest';

vi.mock('../env', () => ({ isTauri: true }));

import { api } from '../api';
import { useStore } from '../store';
import { agentProgressDone, stopAgentTool } from './AgentProgressMonitor';

// R328：终态判定收口成纯函数——pct>=1000 或 message 含「完成」才算结束。
// 修复前这段判断写在 hook 的监听回调里，且事件监听每次渲染都重注册（onStop 内联）不注销。
describe('agentProgressDone（终态判定，R328）', () => {
  it('pct 到千分比上限即终态，message 无关', () => {
    expect(agentProgressDone({ pct: 1000, message: '仍在前台' })).toBe(true);
  });

  it('message 含「完成」兜底也算终态（历史兼容）', () => {
    expect(agentProgressDone({ pct: 500, message: '清理完成，共释放 2.1 GB' })).toBe(true);
  });

  it('进行中不误判：既不到 1000 也不含「完成」', () => {
    expect(agentProgressDone({ pct: 999, message: '扫描目录中…' })).toBe(false);
    expect(agentProgressDone({ pct: 0, message: '' })).toBe(false);
    expect(agentProgressDone({ pct: 100, message: '已处理 1/10 项' })).toBe(false);
  });
});

describe('停止 agent 工具要说话（R341）', () => {
  it('取消失败 → 返回 false，并 toast 出失败原因（任务可能还在跑）', async () => {
    const toast = vi.fn();
    const origToast = useStore.getState().toast;
    useStore.setState({ toast });
    const spy = vi.spyOn(api, 'cancelToolCalls').mockRejectedValue(new Error('cancel timed out'));
    await expect(stopAgentTool()).resolves.toBe(false);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(toast).toHaveBeenCalledTimes(1);
    expect(String(toast.mock.calls[0][0])).toContain('cancel timed out');
    expect(toast.mock.calls[0][1]).toBe('err');
    spy.mockRestore();
    useStore.setState({ toast: origToast });
  });

  it('取消成功 → 返回 true，不弹 toast（正常时不打扰）', async () => {
    const toast = vi.fn();
    const origToast = useStore.getState().toast;
    useStore.setState({ toast });
    const spy = vi.spyOn(api, 'cancelToolCalls').mockResolvedValue(undefined as never);
    await expect(stopAgentTool()).resolves.toBe(true);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(toast).not.toHaveBeenCalled();
    spy.mockRestore();
    useStore.setState({ toast: origToast });
  });
});
