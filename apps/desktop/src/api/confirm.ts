import { invoke } from '@tauri-apps/api/core';
import { isTauri } from '../env';

/**
 * R508 写操作确认 nonce：后端 `issue_confirm_nonce` 签发一次性 nonce（60s 有效、
 * 绑定 op、消费即失效），随写命令一起提交，防 invoke 参数被重放。
 * 所有真实写操作（execute_scope 非 dryRun / recycle_paths / undo /
 * install_scaffold / uninstall_scaffold / toolbelt_recycle / toolbelt_run 的
 * medium+ 风险）都先走这里拿 nonce。
 */
export async function withConfirmNonce<T>(
  op: string,
  run: (confirmNonce: string | null) => Promise<T>,
): Promise<T> {
  if (!isTauri) return run(null);
  const confirmNonce = await invoke<string>('issue_confirm_nonce', { op });
  return run(confirmNonce);
}
