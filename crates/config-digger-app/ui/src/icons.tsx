import { useId, type ReactNode } from "react";

/** Solid, rounded icons in the MISTY style. Cut-outs use a mask with an id unique to each instance. */
const ICONS = {
  grid: () => (
    <>
      <rect x="3" y="3" width="8" height="8" rx="2.5" />
      <rect x="13" y="3" width="8" height="8" rx="2.5" />
      <rect x="3" y="13" width="8" height="8" rx="2.5" />
      <rect x="13" y="13" width="8" height="8" rx="2.5" />
    </>
  ),
  history: (mask: string) => (
    <>
      <mask id={mask}>
        <rect width="24" height="24" fill="#fff" />
        <path d="M12 7v5l3 2" stroke="#000" strokeWidth="2.2" fill="none" strokeLinecap="round" />
      </mask>
      <circle cx="12" cy="12" r="9.5" mask={`url(#${mask})`} />
    </>
  ),
  folder: () => (
    <path d="M3 6.5A2.5 2.5 0 0 1 5.5 4h4l2 2.5h7A2.5 2.5 0 0 1 21 9v8.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5z" />
  ),
  gear: (mask: string) => (
    <>
      <mask id={mask}>
        <rect width="24" height="24" fill="#fff" />
        <circle cx="12" cy="12" r="3" fill="#000" />
      </mask>
      <g mask={`url(#${mask})`}>
        <circle cx="12" cy="12" r="7" />
        {[0, 45].map((angle) => (
          <g key={angle} transform={`rotate(${angle} 12 12)`}>
            <rect x="10" y="1.5" width="4" height="5" rx="1.2" />
            <rect x="10" y="17.5" width="4" height="5" rx="1.2" />
            <rect x="1.5" y="10" width="5" height="4" rx="1.2" />
            <rect x="17.5" y="10" width="5" height="4" rx="1.2" />
          </g>
        ))}
      </g>
    </>
  ),
  info: (mask: string) => (
    <>
      <mask id={mask}>
        <rect width="24" height="24" fill="#fff" />
        <rect x="10.8" y="10.5" width="2.4" height="7" rx="1.2" fill="#000" />
        <circle cx="12" cy="7.6" r="1.5" fill="#000" />
      </mask>
      <circle cx="12" cy="12" r="9.5" mask={`url(#${mask})`} />
    </>
  ),
  refresh: () => (
    <>
      <path d="M20 12a8 8 0 1 1-2.6-5.9" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" />
      <path d="M21 3.5v6h-6z" />
    </>
  ),
  server: (mask: string) => (
    <>
      <mask id={mask}>
        <rect width="24" height="24" fill="#fff" />
        <circle cx="7.5" cy="7.5" r="1.4" fill="#000" />
        <circle cx="7.5" cy="16.5" r="1.4" fill="#000" />
      </mask>
      <g mask={`url(#${mask})`}>
        <rect x="3" y="3.5" width="18" height="8" rx="2.5" />
        <rect x="3" y="12.5" width="18" height="8" rx="2.5" />
      </g>
    </>
  ),
  users: () => (
    <>
      <circle cx="9" cy="8" r="4" />
      <path d="M2 20c0-3.9 3.1-6.5 7-6.5s7 2.6 7 6.5z" />
      <circle cx="17" cy="8.5" r="3" />
      <path d="M17.5 13.5c2.8.2 4.5 2.4 4.5 5.5h-4.2c0-2.2-.7-4-2-5.3z" />
    </>
  ),
  flag: () => <path d="M5 3.5h11.5l-2.5 4.5 2.5 4.5H7V21H5z" />,
} satisfies Record<string, (mask: string) => ReactNode>;

export type IconName = keyof typeof ICONS;

export function Icon({ name, small = false }: { name: IconName; small?: boolean }) {
  const mask = useId();
  return (
    <svg className={`icon${small ? " small" : ""}`} viewBox="0 0 24 24" aria-hidden="true">
      {ICONS[name](mask)}
    </svg>
  );
}
