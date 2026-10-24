// R395：启动/渲染路径（App 引导、左栏、设置、工具墙开关）上的 localStorage 读写
// 全部经过这里的 try/catch 兜底。此前这些 useState 初始化器/effect 直接调
// localStorage——在隐私模式/存储被禁用的浏览器预览里，读取抛错会让整个应用
// 渲染失败（连到根 ErrorBoundary），点「同意」写失败会让事件处理直接崩。
// 数据型路径（store/theme/i18n/permissions 等）保留各自的 try/catch，语义不变。

export function safeGetItem(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function safeSetItem(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* 存储不可用 / 配额满：静默丢弃，不影响主流程 */
  }
}

export function safeRemoveItem(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    /* 同上 */
  }
}
