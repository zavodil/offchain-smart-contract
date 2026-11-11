import type { Verdict } from '../lib/verify/types';

const base = { width: 16, height: 16, viewBox: '0 0 24 24', fill: 'none', strokeWidth: 2.2, strokeLinecap: 'round', strokeLinejoin: 'round' } as const;

export function VerdictIcon({ verdict, size = 16 }: { verdict: Verdict; size?: number }) {
  const props = { ...base, width: size, height: size, stroke: 'currentColor' };
  switch (verdict) {
    case 'pass':
      return (
        <svg {...props}>
          <path d="M5 12.5l4.5 4.5L19 7.5" />
        </svg>
      );
    case 'warn':
      return (
        <svg {...props}>
          <path d="M12 8v5M12 16.5v.5" />
        </svg>
      );
    case 'fail':
      return (
        <svg {...props}>
          <path d="M7 7l10 10M17 7L7 17" />
        </svg>
      );
    case 'skip':
      return (
        <svg {...props}>
          <path d="M7 12h10" />
        </svg>
      );
  }
}

export function Badge({ verdict, size = 16 }: { verdict: Verdict; size?: number }) {
  return (
    <span className={`badge badge-${verdict}`} style={{ width: size + 10, height: size + 10 }}>
      <VerdictIcon verdict={verdict} size={size} />
    </span>
  );
}

export function Lock({ size = 16 }: { size?: number }) {
  return (
    <svg {...base} width={size} height={size} stroke="currentColor">
      <rect x="5" y="11" width="14" height="9" rx="2" />
      <path d="M8 11V8a4 4 0 018 0v3" />
    </svg>
  );
}

export function Chip({ size = 16 }: { size?: number }) {
  return (
    <svg {...base} width={size} height={size} stroke="currentColor">
      <rect x="7" y="7" width="10" height="10" rx="1.5" />
      <path d="M10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4" />
    </svg>
  );
}

export function Shield({ size = 16 }: { size?: number }) {
  return (
    <svg {...base} width={size} height={size} stroke="currentColor">
      <path d="M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6l7-3z" />
    </svg>
  );
}

export function Eye({ size = 16 }: { size?: number }) {
  return (
    <svg {...base} width={size} height={size} stroke="currentColor">
      <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}

export function Spinner({ size = 16 }: { size?: number }) {
  return <span className="spinner" style={{ width: size, height: size }} />;
}
