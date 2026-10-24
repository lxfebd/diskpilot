import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest';
import { setLang } from './i18n';
import { useStore } from './store';
import { api } from './api';
import { openExternal } from './external';

describe('外链打开的回音（R360）', () => {
  let origToast: ReturnType<typeof useStore.getState>['toast'];

  beforeEach(() => {
    setLang('zh');
    origToast = useStore.getState().toast;
  });

  afterEach(() => {
    useStore.setState({ toast: origToast });
    vi.restoreAllMocks();
  });

  it('成功交给系统浏览器，不弹 toast，返回 true', async () => {
    vi.spyOn(api, 'openExternalUrl').mockResolvedValue(undefined as never);
    const toast = vi.fn();
    useStore.setState({ toast });

    await expect(openExternal('https://github.com/lxfebd')).resolves.toBe(true);
    expect(api.openExternalUrl).toHaveBeenCalledWith('https://github.com/lxfebd');
    expect(toast).not.toHaveBeenCalled();
  });

  it('两端空白先裁掉再交给系统', async () => {
    vi.spyOn(api, 'openExternalUrl').mockResolvedValue(undefined as never);

    await expect(openExternal('  https://example.com/x  ')).resolves.toBe(true);
    expect(api.openExternalUrl).toHaveBeenCalledWith('https://example.com/x');
  });

  it('空链接直接返回 false，不劳烦系统打开器', async () => {
    vi.spyOn(api, 'openExternalUrl').mockResolvedValue(undefined as never);

    await expect(openExternal('   ')).resolves.toBe(false);
    expect(api.openExternalUrl).not.toHaveBeenCalled();
  });

  it('打不开 → 错误 toast 带原因，返回 false（不再静默）', async () => {
    vi.spyOn(api, 'openExternalUrl').mockRejectedValue(new Error('unsupported url: file:///x'));
    const toast = vi.fn();
    useStore.setState({ toast });

    await expect(openExternal('file:///x')).resolves.toBe(false);
    expect(toast).toHaveBeenCalledTimes(1);
    expect(String(toast.mock.calls[0][0])).toContain('unsupported url');
    expect(String(toast.mock.calls[0][0])).toContain('打不开链接');
    expect(toast.mock.calls[0][1]).toBe('err');
  });
});
