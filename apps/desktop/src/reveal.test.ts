import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest';
import { setLang } from './i18n';
import { useStore } from './store';
import { api } from './api';
import { revealWithToast } from './reveal';

describe('打开所在文件夹的回音（R317）', () => {
  let origToast: ReturnType<typeof useStore.getState>['toast'];

  beforeEach(() => {
    setLang('zh');
    origToast = useStore.getState().toast;
  });

  afterEach(() => {
    useStore.setState({ toast: origToast });
    vi.restoreAllMocks();
  });

  it('成功打开不打扰（不弹 toast），返回 true', async () => {
    vi.spyOn(api, 'revealInExplorer').mockResolvedValue(undefined as never);
    const toast = vi.fn();
    useStore.setState({ toast });

    await expect(revealWithToast('D:\\a\\b.mp4')).resolves.toBe(true);
    expect(api.revealInExplorer).toHaveBeenCalledWith('D:\\a\\b.mp4');
    expect(toast).not.toHaveBeenCalled();
  });

  it('路径已不存在 → 错误 toast 带原因，返回 false（不再静默吞掉）', async () => {
    vi.spyOn(api, 'revealInExplorer').mockRejectedValue(new Error('path not found'));
    const toast = vi.fn();
    useStore.setState({ toast });

    await expect(revealWithToast('C:\\gone')).resolves.toBe(false);
    expect(toast).toHaveBeenCalledTimes(1);
    expect(String(toast.mock.calls[0][0])).toContain('path not found');
    expect(String(toast.mock.calls[0][0])).toContain('打不开');
    expect(toast.mock.calls[0][1]).toBe('err');
  });
});
