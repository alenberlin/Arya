/**
 * A small, consistent line-icon set drawn inline so the app carries no emoji or
 * raster icons. Each takes the current text color. Paths mirror the Claude
 * Design handoff so the app matches the mockups exactly.
 */
type IconProps = { className?: string };

const base = {
  width: 18,
  height: 18,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.75,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

export const NotesIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.7} {...p} aria-hidden="true">
    <rect x="5" y="3" width="14" height="18" rx="2.5" />
    <line x1="9" y1="8" x2="15" y2="8" />
    <line x1="9" y1="12" x2="15" y2="12" />
    <line x1="9" y1="16" x2="13" y2="16" />
  </svg>
);
export const AgentIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.6} {...p} aria-hidden="true">
    <path d="M12 3.5l1.6 5.4 5.4 1.6-5.4 1.6L12 17.5l-1.6-5.4L5 10.5l5.4-1.6z" />
    <circle cx="18.5" cy="5.5" r="1.3" fill="currentColor" stroke="none" />
  </svg>
);
export const SearchIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.7} {...p} aria-hidden="true">
    <circle cx="11" cy="11" r="6.5" />
    <line x1="16" y1="16" x2="20.5" y2="20.5" />
  </svg>
);
export const GalaxyIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.6} {...p} aria-hidden="true">
    <circle cx="12" cy="12" r="2.4" />
    <circle cx="5" cy="6" r="1.5" />
    <circle cx="19" cy="7" r="1.5" />
    <circle cx="18" cy="18" r="1.5" />
    <circle cx="6" cy="17.5" r="1.5" />
    <path d="M10.1 10.5 6.3 7.2M13.9 10.7 17.6 8M13.7 13.6 16.8 16.6M10.1 13.8 7.1 16.3" />
  </svg>
);
export const RoutinesIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.7} {...p} aria-hidden="true">
    <path d="M4.5 11a7.5 7.5 0 0 1 12.8-4.3L20 9" />
    <path d="M20 4.5V9h-4.5" />
    <path d="M19.5 13a7.5 7.5 0 0 1-12.8 4.3L4 15" />
    <path d="M4 19.5V15h4.5" />
  </svg>
);
export const McpIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.7} {...p} aria-hidden="true">
    <rect x="4" y="4" width="16" height="6" rx="2" />
    <rect x="4" y="14" width="16" height="6" rx="2" />
    <line x1="7.5" y1="7" x2="7.5" y2="7" />
    <line x1="7.5" y1="17" x2="7.5" y2="17" />
  </svg>
);
export const DictationIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.8} {...p} aria-hidden="true">
    <line x1="5" y1="10" x2="5" y2="14" />
    <line x1="9" y1="7" x2="9" y2="17" />
    <line x1="13" y1="4" x2="13" y2="20" />
    <line x1="17" y1="8" x2="17" y2="16" />
    <line x1="21" y1="11" x2="21" y2="13" />
  </svg>
);
export const AccountIcon = (p: IconProps) => (
  <svg {...base} {...p} aria-hidden="true">
    <circle cx="12" cy="8" r="4" />
    <path d="M4 20a8 8 0 0 1 16 0" />
  </svg>
);
export const RecordIcon = (p: IconProps) => (
  <svg {...base} {...p} aria-hidden="true">
    <circle cx="12" cy="12" r="6" fill="currentColor" stroke="none" />
  </svg>
);
export const StopIcon = (p: IconProps) => (
  <svg {...base} {...p} aria-hidden="true">
    <rect x="7" y="7" width="10" height="10" rx="2" fill="currentColor" stroke="none" />
  </svg>
);
export const MeetingIcon = (p: IconProps) => (
  <svg {...base} {...p} aria-hidden="true">
    <circle cx="8" cy="9" r="3" />
    <circle cx="16" cy="9" r="3" />
    <path d="M3 20a5 5 0 0 1 10 0M13 20a5 5 0 0 1 8-4" />
  </svg>
);
export const LockIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={2} {...p} aria-hidden="true">
    <rect x="5" y="11" width="14" height="9" rx="2" />
    <path d="M8 11V8a4 4 0 0 1 8 0v3" />
  </svg>
);
export const ThemeIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.7} {...p} aria-hidden="true">
    <circle cx="12" cy="12" r="4.2" />
    <line x1="12" y1="3" x2="12" y2="5" />
    <line x1="12" y1="19" x2="12" y2="21" />
    <line x1="4" y1="12" x2="6" y2="12" />
    <line x1="18" y1="12" x2="20" y2="12" />
    <line x1="6" y1="6" x2="7.4" y2="7.4" />
    <line x1="16.6" y1="16.6" x2="18" y2="18" />
    <line x1="6" y1="18" x2="7.4" y2="16.6" />
    <line x1="16.6" y1="7.4" x2="18" y2="6" />
  </svg>
);
export const TrashIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.7} {...p} aria-hidden="true">
    <path d="M4 7h16" />
    <path d="M9 7V5a1.5 1.5 0 0 1 1.5-1.5h3A1.5 1.5 0 0 1 15 5v2" />
    <path d="M6.5 7l.8 12a2 2 0 0 0 2 1.9h5.4a2 2 0 0 0 2-1.9l.8-12" />
    <line x1="10" y1="11" x2="10.3" y2="17" />
    <line x1="14" y1="11" x2="13.7" y2="17" />
  </svg>
);
export const PlusIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={2} {...p} aria-hidden="true">
    <line x1="12" y1="6" x2="12" y2="18" />
    <line x1="6" y1="12" x2="18" y2="12" />
  </svg>
);
export const ChevronDownIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={2} {...p} aria-hidden="true">
    <polyline points="6 9 12 15 18 9" />
  </svg>
);
export const ChevronRightIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={2} {...p} aria-hidden="true">
    <polyline points="9 6 15 12 9 18" />
  </svg>
);
export const CheckIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={2.2} {...p} aria-hidden="true">
    <polyline points="4 12 9 17 20 6" />
  </svg>
);
export const CopyIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.7} {...p} aria-hidden="true">
    <rect x="9" y="9" width="12" height="12" rx="2.5" />
    <path d="M6 15H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v1" />
  </svg>
);
export const SendIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={2} {...p} aria-hidden="true">
    <line x1="12" y1="19" x2="12" y2="5" />
    <polyline points="6 11 12 5 18 11" />
  </svg>
);
export const FileIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.7} {...p} aria-hidden="true">
    <rect x="5" y="3" width="14" height="18" rx="2.5" />
  </svg>
);
export const FileWriteIcon = (p: IconProps) => (
  <svg {...base} strokeWidth={1.8} {...p} aria-hidden="true">
    <path d="M14 3v4a1 1 0 0 0 1 1h4" />
    <path d="M5 21V5a2 2 0 0 1 2-2h8l5 5v13a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2z" />
  </svg>
);
