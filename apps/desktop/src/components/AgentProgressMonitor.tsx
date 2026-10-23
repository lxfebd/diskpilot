/**
 * Agent 工具实时进度监控（B2/B3）
 * 
 * 监听 MCP `notifications/progress` → Tauri `agent-tool-progress` 事件，
 * 为当前运行的长任务（stress/bench/disk_find_biggest_files/disk_find_duplicate_files）
 * 提供可视化的千分比进度条。
 * 
 * 与 HwTestMonitor 独立但复用 listen API；UI 上并排显示、互不干扰。
 */
import { useEffect, useRef, useState } from 'react';
import { isTauri } from '../env';
import { api } from '../api';
import { useT } from '../i18n';

export interface AgentToolProgressPayload {
  pct: number; // 千分比 0..1000
  progress: number; // 百分比 0..100（后端已换算）
  total?: number; // 总进度目标（通常 100）
  message: string;
}

/**
 * 停止当前正在运行的 agent 工具调用（B2/B3）。
 * 后端 agent_tool_cancel 向所有在飞的工具调用发送取消信号，立即退出。
 */
export const stopAgentTool = async () => {
  if (isTauri) {
    try {
      await api.cancelToolCalls();
    } catch (e) {
      console.error('停止 agent 工具失败:', e);
    }
  }
};


/** 终态判定（纯函数，可单测）：后端 done 由 pct>=1000 表达；
 *  message 含「完成」是历史兼容兜底（旧调用路径不报 1000）。
 *  两种都认，但绝不把「进行中」误判成终态。 */
export function agentProgressDone(payload: Pick<AgentToolProgressPayload, 'pct' | 'message'>): boolean {
  return payload.pct >= 1000 || payload.message.includes('完成');
}

/**
 * 使用 hook：返回运行中状态 + 当前进度载荷。
 * onStop 回调在终态时触发（用户手动停止或后端 done=true）。
 *
 * 实现注意（R328）：事件监听只在挂载时注册一次（deps []），onStop 经 ref 读取，
 * 避免调用方传内联箭头（ChatPanel 就传 `() => {}`）时每次渲染都重注册一个监听；
 * 卸载时注销监听，import 完成后才挂载的监听也会立刻注销，不再泄漏。
 */
export function useAgentToolProgress(onStop?: () => void) {
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<AgentToolProgressPayload | null>(null);
  const onStopRef = useRef(onStop);
  useEffect(() => {
    onStopRef.current = onStop;
  }, [onStop]);

  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    import('@tauri-apps/api/event').then(async (mod) => {
      const fn = await mod.listen<AgentToolProgressPayload>('agent-tool-progress', (e) => {
        if (cancelled) return;
        const payload = e.payload;
        setProgress(payload);
        if (agentProgressDone(payload)) {
          setProgress(null);
          setRunning(false);
          onStopRef.current?.();
          return;
        }
        setRunning(true);
      });
      if (cancelled) {
        fn();
        return;
      }
      unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const stop = async () => {
    await stopAgentTool();
    setRunning(false);
    onStopRef.current?.();
  };

  const clear = () => {
    setProgress(null);
    setRunning(false);
  };

  return {
    running,
    progress,
    stop,
    clear,
  };
}

/**
 * 实时进度条组件：接收 currentProgress（AgentToolProgressPayload），
 * 用 Material UI 线性进度条渲染千分比百分比。
 * 
 * props:
 * - label: 标题（如 "稳定性压测" / "大文件扫描"）
 * - progress: 当前载荷（包含 pct/message）
 * - onCancel: 点击停止按钮的回调
 */
interface ProgressCardProps {
  label: string;
  progress: AgentToolProgressPayload | null;
  onCancel?: () => void;
  showCancelButton?: boolean;
}

export function AgentProgressCard({
  label,
  progress,
  onCancel,
  showCancelButton = true,
}: ProgressCardProps) {
  const t = useT();
  if (!progress) return null;

  const pctPercent = Math.round(progress.progress); // progress 是 0..100

  // R107：内联样式/硬编码中文 → CSS 类 + i18n（token 走主题，「停止」多语言）。
  return (
    <div className="agent-progress-card">
      <div className="agent-progress-top">
        <span>{label}</span>
        {showCancelButton && onCancel && (
          <button className="agent-progress-stop" onClick={onCancel}>
            {t('chat.input.stop')}
          </button>
        )}
      </div>
      <div className="agent-progress-row">
        <div className="agent-progress-track">
          <div
            className="agent-progress-fill"
            style={{ width: `${pctPercent}%` }}
          />
        </div>
        <div className="agent-progress-pct">{pctPercent}%</div>
      </div>
      {progress.message && (
        <div className="agent-progress-msg">{progress.message}</div>
      )}
    </div>
  );
}
