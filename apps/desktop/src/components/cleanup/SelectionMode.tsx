// 勾选模式选择器：AI 提案工具条「全选安全项 / 全选 / 清空」与 scope 分组头
// 「全选 / 全不选」共用的按钮组。真正的勾选计算（union / per-group subtract /
// safe-only 过滤）在调用方，这里只渲染按钮 + 把模式回传。
//
// 调用方决定：拿到模式后是「并入现选」（提案工具条对 safe）还是「全组替换」
// （scope 分组头），因为语义不同——详见 model.applySelectionMode 的契约注释。
import type { LucideIcon } from 'lucide-react';
import { ShieldCheck, ListChecks, X } from 'lucide-react';
import { useT } from '../../i18n';
import { applySelectionMode, type SelectionMode } from '../../cleanup/model';

export type { SelectionMode, applySelectionMode };

interface ModeOption {
  mode: SelectionMode;
  label: string; // 已由调用方求值好的文案（避免本组件自己 import t）
  icon?: LucideIcon;
}

interface Props {
  options: ModeOption[];
  onMode: (mode: SelectionMode) => void;
  disabled?: boolean;
  /** 给每个按钮额外加的 class（scope 分组头用 `cleanup-toggle-all` 复用旧尺寸）。 */
  btnClass?: string;
}

export function SelectionModePicker({ options, onMode, disabled, btnClass }: Props) {
  return (
    <span className="sel-mode">
      {options.map(({ mode, label, icon: Icon }) => {
        const cls = `ghost${btnClass ? ' ' + btnClass : ''}`;
        return (
          <button
            key={mode}
            type="button"
            className={cls}
            onClick={() => onMode(mode)}
            disabled={disabled}
          >
            {Icon && <Icon size={11} />}
            {label}
          </button>
        );
      })}
    </span>
  );
}

// 便利预设：提案工具条 / scope 分组头各自用得最多的那组按钮。
export function useSelectionOptions() {
  const t = useT();
  return {
    proposalToolbar: [
      { mode: 'safe' as const, label: t('cleanup.selectAllSafe'), icon: ShieldCheck },
      { mode: 'all' as const, label: t('cleanup.selectAll'), icon: ListChecks },
      { mode: 'none' as const, label: t('cleanup.clearChecks'), icon: X },
    ],
    scopeGroup: [
      { mode: 'all' as const, label: t('cleanup.selectAll') },
      { mode: 'none' as const, label: t('cleanup.selectNone') },
    ],
  };
}
