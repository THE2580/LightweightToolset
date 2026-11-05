type ShortcutTipIconProps = {
  className?: string;
  size?: number | string;
  strokeWidth?: number | string;
};

/**
 * 「快捷键提示」工具图标：键盘 + 提示气泡。
 * 按 lucide 的 24 网格 / 2px 描边 / 圆角端点绘制，用法与 lucide 图标一致。
 */
export function ShortcutTipIcon({ className, size = 16, strokeWidth = 2 }: ShortcutTipIconProps) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={strokeWidth}
      viewBox="0 0 24 24"
      width={size}
      xmlns="http://www.w3.org/2000/svg"
    >
      <rect height="8.5" rx="2" width="19" x="2.5" y="12" />
      <path d="M6 16h.01M9.5 16h.01M13 16h.01M18 16h.2" />
      <path d="M15.5 2.6h5a1.6 1.6 0 0 1 1.6 1.6v2.6a1.6 1.6 0 0 1-1.6 1.6H19l-1.4 1.9-1.4-1.9h-.7a1.6 1.6 0 0 1-1.6-1.6V4.2a1.6 1.6 0 0 1 1.6-1.6z" />
    </svg>
  );
}
