// R316：复制路径要回音。此前三处右键菜单（扫描树 / 工具树 / Studio 卡片）复制路径写的是
// `navigator.clipboard?.writeText(p).catch(() => {})`——成功没有任何提示；浏览器没有
// clipboard API 时可选链直接短路，连 catch 都不进，用户以为复制了其实什么都没发生。
// Steam 安装路径与跑分报告早就有 toast 回音，这里把「复制 + 回音」也收敛成一处。
import { t } from './i18n';
import { useStore } from './store';

/** 复制一段文本并回音：成功 toast「已复制路径」，失败 toast 错误原因。返回是否成功。 */
export async function copyPathWithToast(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    useStore.getState().toast(t('common.copiedPath'));
    return true;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    useStore.getState().toast(t('common.copyFailed', { msg }), 'err');
    return false;
  }
}
