// R317：打开所在文件夹也要回音。三处右键菜单此前写的是
// `api.revealInExplorer(p).catch(() => { /* 路径可能已不存在 */ })`——失败被吞掉，
// 用户点了没反应也不知道为什么；Steam 详情页的重叠文件按钮更是 `void api.revealInExplorer(...)`，
// 失败会变成未处理的 Promise 拒绝。路径不存在恰恰是最常见的失败，说一句比不说强。
import { api } from './api';
import { t } from './i18n';
import { useStore } from './store';

/**
 * 在资源管理器中定位路径。成功不打扰（窗口自己会跳出来），失败 toast 原因。
 * 返回是否成功，调用方需要时可据此做后续处理。
 */
export async function revealWithToast(path: string): Promise<boolean> {
  try {
    await api.revealInExplorer(path);
    return true;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    useStore.getState().toast(t('common.revealFailed', { msg }), 'err');
    return false;
  }
}
/**
 * 打开应用数据目录（R386）：撤销台账、隔离区、提醒、硬件与传感器历史、
 * 插件缓存全在这里。设置页「打开数据目录」与市场页「存哪看哪」共用——
 * 取不到路径或打不开都 toast 原因，返回是否成功。
 */
export async function openDataDirWithToast(): Promise<boolean> {
  try {
    const dir = await api.appDataDir();
    if (!dir) {
      useStore.getState().toast(t('settings.general.dataDirFail', { msg: t('settings.general.dataDirMissing') }), 'err');
      return false;
    }
    return await revealWithToast(dir);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    useStore.getState().toast(t('settings.general.dataDirFail', { msg }), 'err');
    return false;
  }
}
