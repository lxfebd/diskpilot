// ── 工具墙共享契约：宿主（Toolbelt）与各面板之间的上下文类型 ──
// 新增工具 = 写一个 ToolPanel 组件 + 在 TOOLS 里注册一行。
import type { ComponentType } from 'react';
import type { LucideIcon } from 'lucide-react';
import type { SettingsTab } from '../Settings';
import type { useT } from '../../i18n';
import type { Node } from '../../types';

export interface DriveInfo {
  path: string;
  total_bytes: number;
  used_bytes: number;
  free_bytes: number;
}

export interface ToolContext {
  drives: DriveInfo[];
  scanning: boolean;
  scanCache: Record<string, Node>;
  onScanDrive: (path: string) => void;
  onScanAll: () => void;
  onGoWorkspace: () => void;
  onOpenSettings?: (tab?: SettingsTab) => void;
}

export type ToolPanel = ComponentType<{ ctx: ToolContext }>;

export interface ToolModule {
  id: string;
  icon: LucideIcon;
  name: string;
  desc: string;
  Panel: ToolPanel;
}

/** 文案函数类型（useT() 的返回值）：模块级常量表改成函数后按这个签名收 t。 */
export type TFunc = ReturnType<typeof useT>;