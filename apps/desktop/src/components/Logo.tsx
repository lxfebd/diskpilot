// DiskPilot logo — 磁盘阵列 + 清扫角标，与桌面图标同源（Volcengine 官方
// 设计系统 block-storage-alt / clean 矢量路径）。颜色全部走 CSS 变量：
// 圆底跟随主题表面色，磁盘跟随主题主色，白描边跟随背景，任意主题下都协调。
export function Logo({ size = 22 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 48 48" aria-hidden>
      {/* 深色圆底：用主题主色深阶，浅/深主题都有辨识度 */}
      <circle cx="24" cy="24" r="23" fill="color-mix(in srgb, var(--accent) 18%, var(--bg))" />
      <circle cx="24" cy="24" r="23" fill="none" stroke="var(--border-strong)" strokeWidth="1" opacity="0.6" />
      {/* 磁盘塔（block-storage-alt 路径，主题色） */}
      <g fill="var(--accent)" transform="translate(0, -1.5)">
        <path fillRule="evenodd" clipRule="evenodd" d="M5 45a2 2 0 01-2-2V5a2 2 0 012-2h38a2 2 0 012 2v38a2 2 0 01-2 2H5zM41 7h-4v12a2 2 0 01-2 2H13a2 2 0 01-2-2V7H7v34h4v-9a2 2 0 012-2h22a2 2 0 012 2v9h4V7zM15 34h18v7H15v-7zM33 7H15v10h18V7zm-2 5a3 3 0 10-6 0 3 3 0 006 0z" />
      </g>
      {/* 清扫角标（clean 路径简化），背景小圆用主题主色 */}
      <circle cx="37" cy="37" r="8.5" fill="var(--accent-strong)" />
      <path
        fill="var(--accent-bg)"
        transform="translate(29.5, 29.5) scale(0.31)"
        fillRule="evenodd"
        clipRule="evenodd"
        d="M29 4a1 1 0 011 1v7h13a1 1 0 011 1v11a1 1 0 01-1 1h-1.304l2.897 16.657A2 2 0 0142.623 44H5.377a2 2 0 01-1.97-2.343L6.304 25H5a1 1 0 01-1-1V13a1 1 0 011-1h13V5a1 1 0 011-1h10z"
      />
    </svg>
  );
}
