import { useEffect, useRef } from 'react';
import { useT } from '../i18n';
import { splitterDelta } from '../splitterKeys';

type Props = {
  onDrag: (deltaPx: number) => void;
  onDoubleClick?: () => void;
  /** 当前宽度（px），进 aria-valuenow，让读屏也能报出面板宽度。 */
  value?: number;
  /** 最小宽度（px），对应 aria-valuemin。 */
  min?: number;
};

export function Splitter({ onDrag, onDoubleClick, value, min }: Props) {
  const t = useT();
  const startX = useRef(0);
  const dragging = useRef(false);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      const dx = e.clientX - startX.current;
      startX.current = e.clientX;
      onDrag(dx);
    };
    const onUp = () => {
      if (!dragging.current) return;
      dragging.current = false;
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, [onDrag]);

  return (
    <div
      className="splitter"
      role="separator"
      aria-orientation="vertical"
      aria-label={t('shell.splitter.aria')}
      aria-valuenow={value}
      aria-valuemin={min}
      tabIndex={0}
      onMouseDown={(e) => {
        dragging.current = true;
        startX.current = e.clientX;
        document.body.style.cursor = 'col-resize';
        document.body.style.userSelect = 'none';
      }}
      onDoubleClick={onDoubleClick}
      onKeyDown={(e) => {
        // Enter = 双击重置的键盘等价物（鼠标能双击复位，键盘也得有对应动作）
        if (e.key === 'Enter' && onDoubleClick) {
          e.preventDefault();
          onDoubleClick();
          return;
        }
        const dx = splitterDelta(e);
        if (dx === null) return;
        e.preventDefault();
        onDrag(dx);
      }}
      title={t('shell.splitter.hint')}
    >
      <span />
    </div>
  );
}
