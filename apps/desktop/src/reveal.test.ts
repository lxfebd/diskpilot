import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest';
import { setLang } from './i18n';
import { useStore } from './store';
import { api } from './api';
import { openDataDirWithToast, revealWithToast } from './reveal';

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

describe('打开应用数据目录的回音（R386）', () => {
  let origToast: ReturnType<typeof useStore.getState>['toast'];

  beforeEach(() => {
    setLang('zh');
    origToast = useStore.getState().toast;
  });

  afterEach(() => {
    useStore.setState({ toast: origToast });
    vi.restoreAllMocks();
  });

  it('拿到路径就在资源管理器里定位，成功不打扰', async () => {
    vi.spyOn(api, 'appDataDir').mockResolvedValue('C:\\Users\\u\\AppData\\Roaming\\com.diskpilot.app');
    vi.spyOn(api, 'revealInExplorer').mockResolvedValue(undefined as never);
    const toast = vi.fn();
    useStore.setState({ toast });

    await expect(openDataDirWithToast()).resolves.toBe(true);
    expect(api.revealInExplorer).toHaveBeenCalledWith('C:\\Users\\u\\AppData\\Roaming\\com.diskpilot.app');
    expect(toast).not.toHaveBeenCalled();
  });

  it('后端没返回路径 → 说明原因，返回 false', async () => {
    vi.spyOn(api, 'appDataDir').mockResolvedValue('');
    const toast = vi.fn();
    useStore.setState({ toast });

    await expect(openDataDirWithToast()).resolves.toBe(false);
    expect(toast).toHaveBeenCalledTimes(1);
    expect(String(toast.mock.calls[0][0])).toContain('没有返回');
    expect(toast.mock.calls[0][1]).toBe('err');
  });

  it('取路径本身失败 → 错误 toast 带原因，返回 false', async () => {
    vi.spyOn(api, 'appDataDir').mockRejectedValue(new Error('path api unavailable'));
    const toast = vi.fn();
    useStore.setState({ toast });

    await expect(openDataDirWithToast()).resolves.toBe(false);
    expect(String(toast.mock.calls[0][0])).toContain('path api unavailable');
    expect(String(toast.mock.calls[0][0])).toContain('打开数据目录失败');
  });
});
