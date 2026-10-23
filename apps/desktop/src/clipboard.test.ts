import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest';
import { setLang } from './i18n';
import { useStore } from './store';
import { copyPathWithToast } from './clipboard';

describe('复制路径回音（R316）', () => {
  let origToast: ReturnType<typeof useStore.getState>['toast'];

  beforeEach(() => {
    setLang('zh');
    origToast = useStore.getState().toast;
  });

  afterEach(() => {
    useStore.setState({ toast: origToast });
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('复制成功 → toast「已复制路径」并返回 true', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const toast = vi.fn();
    useStore.setState({ toast });

    await expect(copyPathWithToast('D:\\out\\clip.mp4')).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith('D:\\out\\clip.mp4');
    expect(toast).toHaveBeenCalledTimes(1);
    expect(String(toast.mock.calls[0][0])).toContain('已复制路径');
  });

  it('剪贴板拒绝（权限/占用）→ 错误 toast 带原因，返回 false 且不再静默', async () => {
    vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn().mockRejectedValue(new Error('denied')) } });
    const toast = vi.fn();
    useStore.setState({ toast });

    await expect(copyPathWithToast('C:\\a.txt')).resolves.toBe(false);
    expect(toast).toHaveBeenCalledTimes(1);
    expect(String(toast.mock.calls[0][0])).toContain('denied');
    expect(toast.mock.calls[0][1]).toBe('err');
  });

  it('环境没有 clipboard API 也要说出来，而不是可选链短路后毫无反应', async () => {
    vi.stubGlobal('navigator', {});
    const toast = vi.fn();
    useStore.setState({ toast });

    await expect(copyPathWithToast('C:\\b.txt')).resolves.toBe(false);
    expect(toast).toHaveBeenCalledTimes(1);
    expect(String(toast.mock.calls[0][0])).toContain('复制失败');
    expect(toast.mock.calls[0][1]).toBe('err');
  });

  it('传自己的成功文案键（错误面板等非路径场景）→ toast 用该键（R321）', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const toast = vi.fn();
    useStore.setState({ toast });

    await expect(copyPathWithToast('boom\nstack', 'shell.boundary.copied')).resolves.toBe(true);
    expect(toast).toHaveBeenCalledTimes(1);
    expect(String(toast.mock.calls[0][0])).toContain('错误信息已复制');
  });
});
