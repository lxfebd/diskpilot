import { describe, it, expect } from 'vitest';
import { agentProgressDone } from './AgentProgressMonitor';

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
