import type { ComponentType, ReactNode } from "react";

export type IconProps = {
  size?: number;
  className?: string;
};

export type IconComponent = ComponentType<IconProps>;

function Svg({ size = 20, className, children }: IconProps & { children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      className={className}
    >
      {children}
    </svg>
  );
}

export function ChatIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3.5 6.8A2.8 2.8 0 0 1 6.3 4h7.4a2.8 2.8 0 0 1 2.8 2.8v4.4a2.8 2.8 0 0 1-2.8 2.8H9.2L5 16.6v-2.9a2.8 2.8 0 0 1-1.5-2.5z" />
    </Svg>
  );
}

export function ModelsIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M10 2.9 16.9 6.5 10 10.1 3.1 6.5z" />
      <path d="M3.1 10.2 10 13.8l6.9-3.6" />
      <path d="M3.1 13.9 10 17.5l6.9-3.6" />
    </Svg>
  );
}

export function SpeechIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3.2 8.2v3.6M6.4 5.6v8.8M9.6 3.3v13.4M12.8 6.4v7.2M16 8.6v2.8" />
    </Svg>
  );
}

export function EmbeddingsIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="4.5" cy="4.5" r="1.3" />
      <circle cx="10" cy="4.5" r="1.3" />
      <circle cx="15.5" cy="4.5" r="1.3" />
      <circle cx="4.5" cy="10" r="1.3" />
      <circle cx="10" cy="10" r="1.9" fill="currentColor" />
      <circle cx="15.5" cy="10" r="1.3" />
      <circle cx="4.5" cy="15.5" r="1.3" />
      <circle cx="15.5" cy="15.5" r="1.3" />
    </Svg>
  );
}

export function SettingsIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3.2 6.5h5.6M12.3 6.5h4.5M3.2 13.5h3.4M10.1 13.5h6.7" />
      <circle cx="10.5" cy="6.5" r="1.9" />
      <circle cx="8.2" cy="13.5" r="1.9" />
    </Svg>
  );
}

export function KeyboardIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="2.4" y="5.2" width="15.2" height="9.6" rx="2.2" />
      <path d="M5.6 8.6h.01M8.4 8.6h.01M11.2 8.6h.01M14 8.6h.01M6.6 11.6h6.8" />
    </Svg>
  );
}

export function SearchIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="8.8" cy="8.8" r="4.9" />
      <path d="m12.5 12.5 3.2 3.2" />
    </Svg>
  );
}

export function InfoIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="10" cy="10" r="6.6" />
      <path d="M10 9.2v4M10 6.8h.01" />
    </Svg>
  );
}

export function RefreshIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M15.4 7.2A6 6 0 1 0 16 11.2" />
      <path d="M15.7 3.9v3.6h-3.6" />
    </Svg>
  );
}

export function DownloadIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M10 3.4v8.4M6.7 8.9 10 12.2l3.3-3.3" />
      <path d="M3.8 13.6v1.6a1.6 1.6 0 0 0 1.6 1.6h9.2a1.6 1.6 0 0 0 1.6-1.6v-1.6" />
    </Svg>
  );
}

export function LoadIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M10 12.2V3.4M6.7 6.7 10 3.4l3.3 3.3" />
      <path d="M3.8 13.6v1.6a1.6 1.6 0 0 0 1.6 1.6h9.2a1.6 1.6 0 0 0 1.6-1.6v-1.6" />
    </Svg>
  );
}

export function PlayIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M7.2 5.4 14.6 10l-7.4 4.6z" />
    </Svg>
  );
}

export function MicIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="7.4" y="3.6" width="5.2" height="8.3" rx="2.6" />
      <path d="M5.7 9.7a4.3 4.3 0 0 0 8.6 0" />
      <path d="M10 14.1v2.3M8.1 16.4h3.8" />
    </Svg>
  );
}

export function StopIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="6.2" y="6.2" width="7.6" height="7.6" rx="1.2" />
    </Svg>
  );
}

export function TrashIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4.4 5.7h11.2M8.2 5.7V4.2h3.6v1.5M5.7 5.7l.7 10a1.2 1.2 0 0 0 1.2 1.1h4.8a1.2 1.2 0 0 0 1.2-1.1l.7-10" />
      <path d="M8.6 8.7v4.8M11.4 8.7v4.8" />
    </Svg>
  );
}

export function StarIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="m10 3.2 2.1 4.2 4.6.7-3.3 3.2.8 4.6-4.2-2.2-4.2 2.2.8-4.6-3.3-3.2 4.6-.7z" />
    </Svg>
  );
}

export function ArchiveIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="3.3" y="4.1" width="13.4" height="3.4" rx="1" />
      <path d="M4.6 7.5v7.2a1.2 1.2 0 0 0 1.2 1.2h8.4a1.2 1.2 0 0 0 1.2-1.2V7.5" />
      <path d="M8.2 10.4h3.6" />
    </Svg>
  );
}

export function FolderIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3.2 6.4A1.6 1.6 0 0 1 4.8 4.8h2.7l1.4 1.6h6.3a1.6 1.6 0 0 1 1.6 1.6v6a1.6 1.6 0 0 1-1.6 1.6H4.8a1.6 1.6 0 0 1-1.6-1.6z" />
    </Svg>
  );
}

export function LayersIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M10 3.4 16.4 6.7 10 10 3.6 6.7z" />
      <path d="m3.6 10.1 6.4 3.3 6.4-3.3" />
      <path d="m3.6 13.4 6.4 3.3 6.4-3.3" />
    </Svg>
  );
}

export function PlusIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M10 4.6v10.8M4.6 10h10.8" />
    </Svg>
  );
}

export function CheckIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="m4.9 10.4 3.3 3.2 6.9-7.2" />
    </Svg>
  );
}

export function AlertIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M10 3.4 17 16H3z" />
      <path d="M10 8.4v3.3M10 13.9h.01" />
    </Svg>
  );
}

export function ChevronDownIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M5.8 8.2 10 12.2l4.2-4" />
    </Svg>
  );
}

export function ChevronLeftIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12.2 5.8 8.2 10l4 4.2" />
    </Svg>
  );
}

export function CloseIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M5.6 5.6l8.8 8.8M14.4 5.6l-8.8 8.8" />
    </Svg>
  );
}

export function PencilIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12.9 3.9a1.8 1.8 0 0 1 2.6 0l.6.6a1.8 1.8 0 0 1 0 2.6l-7.6 7.6-3.7 1.1 1.1-3.7z" />
      <path d="m11.6 5.2 3.2 3.2" />
    </Svg>
  );
}

export function CopyIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="7.2" y="7.2" width="9.4" height="9.4" rx="2.2" />
      <path d="M12.9 5.4a2.1 2.1 0 0 0-2.1-1.8H5.9a2.2 2.2 0 0 0-2.2 2.2v4.9a2.1 2.1 0 0 0 1.8 2.1" />
    </Svg>
  );
}

export function SendIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3.5 10 16.5 4.2 13 16.2l-2.9-4.7z" />
      <path d="m10.1 11.5 6.4-7.3" />
    </Svg>
  );
}

export function PaperclipIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M13.9 6.4 7.6 12.7a2.2 2.2 0 0 0 3.1 3.1l6.2-6.2a3.9 3.9 0 0 0-5.5-5.5L5.2 10.1a5.6 5.6 0 0 0 7.9 7.9l5.2-5.2" />
    </Svg>
  );
}

export function FileIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M11.2 3.2H5.6a1.4 1.4 0 0 0-1.4 1.4v10.8a1.4 1.4 0 0 0 1.4 1.4h8.8a1.4 1.4 0 0 0 1.4-1.4V7.2z" />
      <path d="M11.2 3.2v4h4" />
    </Svg>
  );
}

export function SlidersIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4 6.5h12M4 13.5h12" />
      <circle cx="8" cy="6.5" r="1.9" />
      <circle cx="12.5" cy="13.5" r="1.9" />
    </Svg>
  );
}

export function ToolsIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <g transform="translate(0.16 1.02) scale(0.784)">
        <path
          strokeWidth={1.9}
          d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"
        />
      </g>
    </Svg>
  );
}

export function GitBranchIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M6 3.5v10.5" />
      <circle cx="6" cy="16.5" r="2" />
      <circle cx="16.5" cy="6" r="2" />
      <path d="M16.5 8a8.5 8.5 0 0 1-8.5 8.5" />
    </Svg>
  );
}

export function PackageIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3.5 6.8 10 3.4l6.5 3.4v6.4L10 16.6l-6.5-3.4z" />
      <path d="M3.5 6.8 10 10l6.5-3.2M10 10v6.6" />
    </Svg>
  );
}

export function CodeIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="m7 6-4 4 4 4" />
      <path d="m13 6 4 4-4 4" />
    </Svg>
  );
}

export function JsonIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M7.2 3.8c-1.3 0-1.9.5-1.9 1.5v1.6c0 .8-.3 1.2-1.3 1.3v1.2c1 .1 1.3.5 1.3 1.3v1.6c0 1 .6 1.5 1.9 1.5" />
      <path d="M12.8 3.8c1.3 0 1.9.5 1.9 1.5v1.6c0 .8.3 1.2 1.3 1.3v1.2c-1 .1-1.3.5-1.3 1.3v1.6c0 1-.6 1.5-1.9 1.5" />
    </Svg>
  );
}

export function BookIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3.6 5.6a1.9 1.9 0 0 1 1.9-1.9H10v12.6H5.5a1.9 1.9 0 0 0-1.9 1.9z" />
      <path d="M16.4 5.6a1.9 1.9 0 0 0-1.9-1.9H10v12.6h4.5a1.9 1.9 0 0 1 1.9 1.9z" />
    </Svg>
  );
}

export function ContainerIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="3.6" y="10.5" width="12.8" height="6" rx="1.6" />
      <path d="M3.6 12.6h12.8M6.5 10.5V8.2M10 10.5V8.2M13.5 10.5V8.2" />
      <rect x="6.5" y="4.2" width="3" height="4" rx="0.8" />
      <rect x="10" y="4.2" width="3" height="4" rx="0.8" />
    </Svg>
  );
}

export function LockIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="4.8" y="9.4" width="10.4" height="7" rx="1.9" />
      <path d="M7 9.4V6.9a3 3 0 0 1 6 0v2.5" />
    </Svg>
  );
}

export function ImageIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="3.4" y="4.4" width="13.2" height="11.2" rx="2" />
      <circle cx="7.6" cy="8.4" r="1.3" />
      <path d="m5 14 3.4-3.4 2.4 2.4 1.9-1.9 2.3 2.9" />
    </Svg>
  );
}

export function TerminalIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="m5.2 6.2 4 3.8-4 3.8" />
      <path d="M11.2 13.8h3.8" />
    </Svg>
  );
}

export function GearIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="10" cy="10" r="2.7" />
      <path d="M10 2.9v2.1M10 15v2.1M2.9 10h2.1M15 10h2.1M5 5l1.5 1.5M13.5 13.5 15 15M15 5l-1.5 1.5M6.5 13.5 5 15" />
    </Svg>
  );
}
