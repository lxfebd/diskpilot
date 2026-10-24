// R360：外链一律交给系统浏览器。`<a target="_blank">` 在 Tauri/WebView2 里
// 会被静默取消（wry 没有 on_new_window 处理器时 `args.SetHandled(true)`），
// 免责声明的 GitHub、设置里的更新下载、工具箱的教程/下载、插件市场主页点了
// 全都没反应。这里统一走 Rust 命令 open_external_url（协议白名单在那边）。
import { api } from './api';
import { t } from './i18n';
import { useStore } from './store';

/**
 * 用系统默认浏览器打开链接。成功不打扰，失败 toast 原因（不静默）。
 * 返回是否成功。
 */
export async function openExternal(url: string): Promise<boolean> {
  const target = url.trim();
  if (!target) return false;
  try {
    await api.openExternalUrl(target);
    return true;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    useStore.getState().toast(t('common.openLinkFailed', { msg }), 'err');
    return false;
  }
}
