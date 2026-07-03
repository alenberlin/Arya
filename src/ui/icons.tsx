/**
 * A small, consistent icon set (1.75 stroke, rounded) drawn inline so the app
 * carries no emoji or raster icons. Each takes the current text color.
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
  <svg {...base} {...p} aria-hidden="true">
    <path d="M4 4.5A1.5 1.5 0 0 1 5.5 3h9L20 8.5V19.5A1.5 1.5 0 0 1 18.5 21h-13A1.5 1.5 0 0 1 4 19.5z" />
    <path d="M14 3v5.5H20M8 13h8M8 17h5" />
  </svg>
);
export const AgentIcon = (p: IconProps) => (
  <svg {...base} {...p} aria-hidden="true">
    <rect x="4" y="7" width="16" height="12" rx="2.5" />
    <path d="M12 3v4M8.5 12v1.5M15.5 12v1.5M2 12h2M20 12h2" />
  </svg>
);
export const SearchIcon = (p: IconProps) => (
  <svg {...base} {...p} aria-hidden="true">
    <circle cx="11" cy="11" r="6.5" />
    <path d="M20 20l-4-4" />
  </svg>
);
export const RoutinesIcon = (p: IconProps) => (
  <svg {...base} {...p} aria-hidden="true">
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 7.5V12l3 2" />
  </svg>
);
export const McpIcon = (p: IconProps) => (
  <svg {...base} {...p} aria-hidden="true">
    <path d="M6 3v6a6 6 0 0 0 12 0V3M12 15v6M8 21h8" />
  </svg>
);
export const DictationIcon = (p: IconProps) => (
  <svg {...base} {...p} aria-hidden="true">
    <rect x="9" y="3" width="6" height="11" rx="3" />
    <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
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
