import type { ReactNode } from 'react';

// 页面头：标题 + 副标题在左，动作区在右。作为 .app-content 网格的第一行，
// 不参与滚动——列表/树图滚动时标题栏纹丝不动。
//
// 动作区承载「页面级」按钮（如工作台的选目录/扫描）；全局状态（已释放空间、
// 扫描规模、设置入口）由 App 一并塞进 actions，保持顶栏只有一行。
interface Props {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
}

export function PageHeader({ title, subtitle, actions }: Props) {
  return (
    <header className="page-head">
      <div className="page-heading">
        {/* 标题本身就在页面上，不挂原生 title（悬浮弹出一模一样的文字是噪音）；
            副标题长且会省略号截断，保留 tooltip 兜底看全文。 */}
        <h1 className="page-title">{title}</h1>
        {subtitle && <p className="page-subtitle" title={subtitle}>{subtitle}</p>}
      </div>
      {actions && <div className="page-head-actions">{actions}</div>}
    </header>
  );
}
