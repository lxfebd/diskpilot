// 吸底行动条：清理页底部那条 44px 高的「已选 N 项 · 共 X GB（预估） + 主按钮」。
// 复用既有的 .cleanup-footer/.cleanup-summary/.cleanup-actions 样式，外层
// .selbar 只是钉死高度 + 把条目钉到行动区，跟 .cleanup-modal 里那条对齐。
import type { ReactNode } from 'react';

interface Props {
  /** 左侧合计摘要（已选 N 项 / 共 X GB / 等）。 */
  summary: ReactNode;
  /** 右侧主按钮（预览 / 预备 / 真删 的 ProgressButton 或普通按钮）。 */
  children: ReactNode;
  /** 可选的「清空勾选」按钮（列表型行动条才有，模态预览窗不带）。 */
  clearLabel?: string;
  onClear?: () => void;
  clearDisabled?: boolean;
  running?: boolean;
}

export function SelectionActionBar({ summary, children, clearLabel, onClear, clearDisabled, running }: Props) {
  return (
    <div className="cleanup-footer selbar">
      <div className="cleanup-summary selbar-summary">{summary}</div>
      <div className="cleanup-actions selbar-actions">
        {clearLabel && onClear && (
          <button className="ghost" onClick={onClear} disabled={clearDisabled ?? running}>
            {clearLabel}
          </button>
        )}
        {children}
      </div>
    </div>
  );
}
