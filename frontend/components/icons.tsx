import * as React from "react";

type IconProps = React.SVGProps<SVGSVGElement>;

const make = (children: React.ReactNode) =>
  function Icon({ className = "icon", ...props }: IconProps) {
    return (
      <svg className={className} viewBox="0 0 24 24" {...props}>
        {children}
      </svg>
    );
  };

export const Ico = {
  home: make(<path d="M3 11l9-8 9 8v10a1 1 0 01-1 1h-5v-7h-6v7H4a1 1 0 01-1-1z" />),
  menu: make(<path d="M3 6h18M3 12h18M3 18h18" />),
  files: make(
    <>
      <path d="M14 3H6a2 2 0 00-2 2v14a2 2 0 002 2h12a2 2 0 002-2V9z" />
      <path d="M14 3v6h6" />
    </>
  ),
  activity: make(<path d="M22 12h-4l-3 9L9 3l-3 9H2" />),
  views: make(
    <>
      <rect x="3" y="3" width="7" height="7" rx="1" />
      <rect x="14" y="3" width="7" height="7" rx="1" />
      <rect x="3" y="14" width="7" height="7" rx="1" />
      <rect x="14" y="14" width="7" height="7" rx="1" />
    </>
  ),
  share: make(
    <>
      <circle cx="18" cy="5" r="3" />
      <circle cx="6" cy="12" r="3" />
      <circle cx="18" cy="19" r="3" />
      <path d="M8.6 13.5L15.4 17.5M15.4 6.5L8.6 10.5" />
    </>
  ),
  trash: make(
    <path d="M3 6h18M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6" />
  ),
  star: make(<polygon points="12 2 15.1 8.5 22 9.3 17 14.1 18.2 21 12 17.8 5.8 21 7 14.1 2 9.3 8.9 8.5" />),
  cog: make(
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 01-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 01-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 010-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 014 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 012.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 010 4h-.1a1.7 1.7 0 00-1.5 1z" />
    </>
  ),
  folder: make(<path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v9a2 2 0 01-2 2H5a2 2 0 01-2-2z" />),
  folderOpen: make(
    <>
      <path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2" />
      <path d="M3 9h18l-2 8a2 2 0 01-2 2H5a2 2 0 01-2-2z" />
    </>
  ),
  file: make(
    <>
      <path d="M14 3H6a2 2 0 00-2 2v14a2 2 0 002 2h12a2 2 0 002-2V9z" />
      <path d="M14 3v6h6" />
    </>
  ),
  upload: make(<path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4M7 9l5-5 5 5M12 4v12" />),
  download: make(<path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4M7 11l5 5 5-5M12 16V4" />),
  search: make(
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="M21 21l-4.3-4.3" />
    </>
  ),
  filter: make(<path d="M3 4h18M6 12h12M10 20h4" />),
  sort: make(<path d="M3 6h13M3 12h9M3 18h5M17 8l4-4 4 4M21 4v16" />),
  group: make(
    <>
      <rect x="3" y="4" width="18" height="4" rx="1" />
      <rect x="3" y="11" width="18" height="3" rx="1" />
      <rect x="3" y="17" width="18" height="3" rx="1" />
    </>
  ),
  table: make(
    <>
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <path d="M3 9h18M3 15h18M9 3v18" />
    </>
  ),
  gallery: make(
    <>
      <rect x="3" y="3" width="8" height="8" rx="1" />
      <rect x="13" y="3" width="8" height="8" rx="1" />
      <rect x="3" y="13" width="8" height="8" rx="1" />
      <rect x="13" y="13" width="8" height="8" rx="1" />
    </>
  ),
  board: make(
    <>
      <rect x="3" y="3" width="5" height="18" rx="1" />
      <rect x="10" y="3" width="5" height="13" rx="1" />
      <rect x="17" y="3" width="4" height="9" rx="1" />
    </>
  ),
  cal: make(
    <>
      <rect x="3" y="4" width="18" height="18" rx="2" />
      <path d="M3 10h18M8 2v4M16 2v4" />
    </>
  ),
  timeline: make(
    <>
      <path d="M3 6h12M3 12h18M3 18h8" />
      <circle cx="18" cy="6" r="1.5" />
      <circle cx="22" cy="12" r="1.5" />
      <circle cx="13" cy="18" r="1.5" />
    </>
  ),
  plus: make(<path d="M12 5v14M5 12h14" />),
  moon: make(<path d="M21 12.8A9 9 0 1111.2 3a7 7 0 009.8 9.8z" />),
  sun: make(
    <>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
    </>
  ),
  more: make(
    <>
      <circle cx="5" cy="12" r="1.2" />
      <circle cx="12" cy="12" r="1.2" />
      <circle cx="19" cy="12" r="1.2" />
    </>
  ),
  moreV: make(
    <>
      <circle cx="12" cy="5" r="1.2" />
      <circle cx="12" cy="12" r="1.2" />
      <circle cx="12" cy="19" r="1.2" />
    </>
  ),
  bell: make(
    <>
      <path d="M6 8a6 6 0 1112 0c0 7 3 9 3 9H3s3-2 3-9" />
      <path d="M10 21a2 2 0 004 0" />
    </>
  ),
  user: make(
    <>
      <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2" />
      <circle cx="12" cy="7" r="4" />
    </>
  ),
  users: make(
    <>
      <path d="M16 21v-2a4 4 0 00-4-4H6a4 4 0 00-4 4v2" />
      <circle cx="9" cy="7" r="4" />
      <path d="M22 21v-2a4 4 0 00-3-3.87M16 3.13a4 4 0 010 7.75" />
    </>
  ),
  chevron: make(<path d="M9 18l6-6-6-6" />),
  down: make(<path d="M6 9l6 6 6-6" />),
  up: make(<path d="M6 15l6-6 6 6" />),
  left: make(<path d="M15 6l-6 6 6 6" />),
  check: make(<path d="M5 12l5 5L20 7" />),
  x: make(<path d="M18 6L6 18M6 6l12 12" />),
  link: make(
    <>
      <path d="M10 14a4 4 0 005.66 0l3-3a4 4 0 10-5.66-5.66l-1 1" />
      <path d="M14 10a4 4 0 00-5.66 0l-3 3a4 4 0 105.66 5.66l1-1" />
    </>
  ),
  copy: make(
    <>
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
    </>
  ),
  eye: make(
    <>
      <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8S1 12 1 12z" />
      <circle cx="12" cy="12" r="3" />
    </>
  ),
  lock: make(
    <>
      <rect x="3" y="11" width="18" height="11" rx="2" />
      <path d="M7 11V7a5 5 0 0110 0v4" />
    </>
  ),
  cmd: make(<path d="M9 6a3 3 0 100 6h6a3 3 0 100-6 3 3 0 00-3 3v6a3 3 0 11-3-3 3 3 0 013 3v-6" />),
  bolt: make(<path d="M13 2L3 14h7l-1 8 10-12h-7z" />),
  pin: make(
    <>
      <path d="M12 17v5" />
      <path d="M9 10.76V6h6v4.76l3 3.24v3H6v-3z" />
    </>
  ),
  clock: make(
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </>
  ),
  history: make(
    <>
      <path d="M3 12a9 9 0 109-9 9 9 0 00-7 3" />
      <path d="M3 3v6h6" />
      <path d="M12 7v5l3 2" />
    </>
  ),
  refresh: make(
    <>
      <path d="M3 4v6h6" />
      <path d="M21 20v-6h-6" />
      <path d="M3.5 14a9 9 0 0014.85 4M20.5 10A9 9 0 005.65 6" />
    </>
  ),
  layers: make(
    <>
      <polygon points="12 2 2 7 12 12 22 7 12 2" />
      <polyline points="2 17 12 22 22 17" />
      <polyline points="2 12 12 17 22 12" />
    </>
  ),
  archive: make(
    <>
      <rect x="2" y="3" width="20" height="5" rx="1" />
      <path d="M4 8v11a2 2 0 002 2h12a2 2 0 002-2V8M10 12h4" />
    </>
  ),
  globe: make(
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18M12 3a14 14 0 010 18M12 3a14 14 0 000 18" />
    </>
  ),
  tag: make(
    <>
      <path d="M3 12V4h8l10 10-8 8z" />
      <circle cx="8" cy="8" r="1.5" />
    </>
  ),
  database: make(
    <>
      <ellipse cx="12" cy="5" rx="9" ry="3" />
      <path d="M21 5v6c0 1.7-4 3-9 3s-9-1.3-9-3V5" />
      <path d="M21 11v6c0 1.7-4 3-9 3s-9-1.3-9-3v-6" />
    </>
  ),
  bucket: make(
    <>
      <path d="M5 8h14l-1.5 11a2 2 0 01-2 2H8.5a2 2 0 01-2-2L5 8z" />
      <path d="M5 8c0-2 3-3 7-3s7 1 7 3" />
    </>
  ),
  shield: make(<path d="M12 2L4 6v6c0 5 3.5 9 8 10 4.5-1 8-5 8-10V6z" />),
  drag: make(
    <>
      <circle cx="9" cy="5" r="1" />
      <circle cx="9" cy="12" r="1" />
      <circle cx="9" cy="19" r="1" />
      <circle cx="15" cy="5" r="1" />
      <circle cx="15" cy="12" r="1" />
      <circle cx="15" cy="19" r="1" />
    </>
  ),
  warning: make(
    <>
      <path d="M10.3 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z" />
      <path d="M12 9v4M12 17h0" />
    </>
  ),
  expand: make(<path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" />),
  collapse: make(<path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" />),
  sparkle: make(
    <>
      <path d="M12 3l1.5 4.5L18 9l-4.5 1.5L12 15l-1.5-4.5L6 9l4.5-1.5z" />
      <path d="M19 14l.7 2.3L22 17l-2.3.7L19 20l-.7-2.3L16 17l2.3-.7z" />
    </>
  ),
} as const;
