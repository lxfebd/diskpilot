import { useEffect, useRef } from 'react';
import { ExternalLink } from 'lucide-react';
import { useEscapeKey } from '../escapeKey';
import { useReturnFocus } from '../dialogFocus';

export interface ContextMenuItem {
  label: string;
  icon?: React.ReactNode;
  onClick: () => void;
  danger?: boolean;
}

export interface ContextMenuState {
  x: number;
  y: number;
  items: ContextMenuItem[];
}

type Props = {
  state: ContextMenuState | null;
  onClose: () => void;
};

/** R353：菜单项之间的移动（带环绕）。current 传 -1 表示还没落在任何一项上。 */
export function nextMenuIndex(current: number, len: number, delta: number): number {
  if (len <= 0) return -1;
  if (current < 0 || current >= len) return delta >= 0 ? 0 : len - 1;
  return (current + delta + len) % len;
}

export function ContextMenu({ state, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  // R353：菜单也是「关掉要把焦点还回去」的一层（键盘唤出菜单时尤其重要）。
  useReturnFocus(state !== null);
  // R353：Esc 交给 R349 的栈——菜单叠在别的弹窗上时只关菜单这一层。
  useEscapeKey(onClose, state !== null);

  // R353：打开后把焦点放到第一项，方向键才有起点（菜单此前只能用鼠标点）。
  useEffect(() => {
    if (!state) return;
    ref.current?.querySelector<HTMLButtonElement>('button')?.focus();
  }, [state]);

  useEffect(() => {
    if (!state) return;
    const handleDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const handleScroll = () => onClose();
    window.addEventListener('mousedown', handleDown, true);
    window.addEventListener('scroll', handleScroll, true);
    window.addEventListener('blur', onClose);
    return () => {
      window.removeEventListener('mousedown', handleDown, true);
      window.removeEventListener('scroll', handleScroll, true);
      window.removeEventListener('blur', onClose);
    };
  }, [state, onClose]);

  if (!state) return null;

  const W = 220;
  const H = state.items.length * 30 + 8;
  const x = Math.min(state.x, window.innerWidth - W - 4);
  const y = Math.min(state.y, window.innerHeight - H - 4);

  return (
    <div
      ref={ref}
      className="ctxmenu"
      role="menu"
      style={{ left: x, top: y, width: W }}
      onContextMenu={(e) => e.preventDefault()}
      onKeyDown={(e) => {
        // R353：上下方向键 / Home / End 在菜单项之间走；Enter 与空格沿用按钮原生激活。
        const items = Array.from(ref.current?.querySelectorAll<HTMLButtonElement>('button') ?? []);
        const cur = items.findIndex((b) => b === document.activeElement);
        let idx = -1;
        if (e.key === 'ArrowDown') idx = nextMenuIndex(cur, items.length, 1);
        else if (e.key === 'ArrowUp') idx = nextMenuIndex(cur, items.length, -1);
        else if (e.key === 'Home') idx = 0;
        else if (e.key === 'End') idx = items.length - 1;
        if (idx >= 0 && idx < items.length) {
          e.preventDefault();
          items[idx].focus();
        }
      }}
    >
      {state.items.map((it, i) => (
        <button
          key={i}
          role="menuitem"
          tabIndex={-1}
          className={'ctxmenu-item' + (it.danger ? ' danger' : '')}
          onClick={() => { it.onClick(); onClose(); }}
        >
          <span className="ctxmenu-icon">{it.icon ?? <ExternalLink size={12} />}</span>
          <span>{it.label}</span>
        </button>
      ))}
    </div>
  );
}
