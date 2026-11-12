/**
 * The authored sprite sheet. Everything drawn here, nothing borrowed.
 * Fills use palette custom properties so sprites inherit the night farm.
 */

type Props = { className?: string };

const drop = 'M0 0c.9 1.4 3.4 4.6 3.4 6.7a3.4 3.4 0 0 1-6.8 0C-3.4 4.6-.9 1.4 0 0z';

export function CloudMark({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 40 34" width="30" height="25" aria-hidden focusable="false">
      <g fill="var(--cloud)">
        <circle cx="14" cy="14" r="7.5" />
        <circle cx="22" cy="10" r="9" />
        <circle cx="29" cy="15" r="6.5" />
        <rect x="7" y="14" width="26" height="6.5" rx="3.2" />
      </g>
      <g fill="var(--rain)">
        <path d={drop} transform="translate(14 23) scale(1.05)" />
        <path d={drop} transform="translate(21 26) scale(.9)" />
        <path d={drop} transform="translate(28 22) scale(1.05)" />
      </g>
    </svg>
  );
}

/** Weather over a plot: the cloud that rained, with its drops. */
export function RainCloud({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 64 46" width="58" height="42" aria-hidden focusable="false">
      <g fill="var(--cloud)">
        <circle cx="22" cy="18" r="10" />
        <circle cx="35" cy="13" r="12" />
        <circle cx="47" cy="19" r="8" />
        <rect x="11" y="19" width="44" height="8" rx="4" />
      </g>
      <g fill="var(--rain)">
        <path className="rain-drop rain-drop-1" d={drop} transform="translate(21 31) scale(1)" />
        <path className="rain-drop rain-drop-2" d={drop} transform="translate(32 33) scale(.82)" />
        <path className="rain-drop rain-drop-3" d={drop} transform="translate(42 31) scale(1)" />
      </g>
    </svg>
  );
}

/** Weather over a plot: dry haze. */
export function DrySun({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 64 46" width="58" height="42" aria-hidden focusable="false">
      <g stroke="var(--coin)" strokeWidth="2.4" strokeLinecap="round" opacity=".9">
        <path d="M32 6v5M32 33v5M14 22h5M45 22h5M19 9l3.5 3.5M41.5 31.5 45 35M45 9l-3.5 3.5M22.5 31.5 19 35" />
      </g>
      <circle cx="32" cy="22" r="8.5" fill="var(--coin)" />
      <path d="M10 39h44" stroke="var(--coin)" strokeWidth="2" strokeLinecap="round" opacity=".45" strokeDasharray="3 5" />
    </svg>
  );
}

/**
 * Growth stages, drawn from the soil line up.
 * 0 bare · 1 seedling · 2 leafy · 3 tall with a gold fruit.
 */
export function Crop({ stage, className }: { stage: 0 | 1 | 2 | 3; className?: string }) {
  if (stage === 0) return null;
  return (
    <svg
      className={className}
      viewBox="0 0 80 64"
      role="img"
      aria-label={
        stage === 1 ? 'seedling' : stage === 2 ? 'leafy young plant' : 'tall plant with a coin fruit'
      }
    >
      <g fill="none" stroke="var(--sprout)" strokeLinecap="round" strokeLinejoin="round">
        {stage === 1 && (
          <>
            <path d="M40 64V42" strokeWidth="3.2" />
            <path d="M40 57c-8.5 0-13-4-13-10 7 0 13 4 13 10z" fill="var(--sprout)" stroke="none" />
            <path d="M40 52c8.5 0 13-4 13-10-7 0-13 4-13 10z" fill="var(--sprout)" stroke="none" opacity=".8" />
          </>
        )}
        {stage === 2 && (
          <>
            <path d="M40 64V26" strokeWidth="3.6" />
            <path d="M40 54c-12 0-19-5-19-13.5 10 0 19 5 19 13.5z" fill="var(--sprout)" stroke="none" />
            <path d="M40 47c12 0 19-5 19-13.5-10 0-19 5-19 13.5z" fill="var(--sprout)" stroke="none" opacity=".85" />
            <path d="M40 39c-10 0-16-4.2-16-11.5 8.5 0 16 4.2 16 11.5z" fill="var(--sprout)" stroke="none" opacity=".7" />
            <path d="M40 33c10 0 16-4.2 16-11.5-8.5 0-16 4.2-16 11.5z" fill="var(--sprout)" stroke="none" opacity=".55" />
          </>
        )}
        {stage === 3 && (
          <>
            <path d="M40 64V16" strokeWidth="4" />
            <path d="M40 56c-14 0-21-5.5-21-14.5 11 0 21 5.5 21 14.5z" fill="var(--sprout)" stroke="none" />
            <path d="M40 49c14 0 21-5.5 21-14.5-11 0-21 5.5-21 14.5z" fill="var(--sprout)" stroke="none" opacity=".85" />
            <path d="M40 40c-11 0-17-4.5-17-12 9 0 17 4.5 17 12z" fill="var(--sprout)" stroke="none" opacity=".7" />
            <path d="M40 34c11 0 17-4.5 17-12-9 0-17 4.5-17 12z" fill="var(--sprout)" stroke="none" opacity=".55" />
            <g>
              <circle cx="40" cy="10" r="9.5" fill="var(--coin)" />
              <circle cx="40" cy="10" r="6.3" fill="none" stroke="var(--act-ink)" strokeWidth="1.3" opacity=".55" />
              <path d="M40 6v8" stroke="var(--act-ink)" strokeWidth="1.7" strokeLinecap="round" opacity=".7" />
            </g>
          </>
        )}
      </g>
    </svg>
  );
}

export function CoinIcon({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 16 16" width="14" height="14" aria-hidden focusable="false">
      <circle cx="8" cy="8" r="7" fill="var(--coin)" />
      <circle cx="8" cy="8" r="4.4" fill="none" stroke="var(--act-ink)" strokeWidth="1.1" opacity=".5" />
      <path d="M8 5v6" stroke="var(--act-ink)" strokeWidth="1.4" strokeLinecap="round" opacity=".65" />
    </svg>
  );
}

export function SeedIcon({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 20 20" width="18" height="18" aria-hidden focusable="false">
      <path
        d="M10 17c-3.6 0-6.5-2.9-6.5-6.5C3.5 6 10 3 10 3s6.5 3 6.5 7.5C16.5 14.1 13.6 17 10 17z"
        fill="var(--seed)"
      />
      <path d="M10 8.5v6" stroke="var(--act-ink)" strokeWidth="1.4" strokeLinecap="round" opacity=".55" />
    </svg>
  );
}

export function FlagIcon({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 16 18" width="13" height="15" aria-hidden focusable="false">
      <path d="M3 17V2" stroke="var(--ink)" strokeWidth="1.8" strokeLinecap="round" />
      <path d="M3.8 2.6h8.6L10 6l2.4 3.4H3.8z" fill="var(--coin)" stroke="var(--ink)" strokeWidth=".9" strokeLinejoin="round" />
    </svg>
  );
}

export function SunIcon({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 20 20" width="18" height="18" aria-hidden focusable="false">
      <g stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
        <path d="M10 1.8v2.2M10 16v2.2M1.8 10H4M16 10h2.2M4.2 4.2l1.6 1.6M14.2 14.2l1.6 1.6M15.8 4.2l-1.6 1.6M5.8 14.2l-1.6 1.6" />
      </g>
      <circle cx="10" cy="10" r="3.4" fill="currentColor" />
    </svg>
  );
}

export function CloudIcon({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 20 20" width="18" height="18" aria-hidden focusable="false">
      <g fill="currentColor">
        <circle cx="7" cy="7.5" r="3.4" />
        <circle cx="12" cy="6" r="4" />
        <circle cx="15" cy="8.4" r="2.8" />
        <rect x="4" y="7.4" width="12" height="3" rx="1.5" />
      </g>
      <path d="M8 13.4l1 2.6M13 13.4l1 2.6" stroke="var(--rain)" strokeWidth="1.6" strokeLinecap="round" />
    </svg>
  );
}

export function CrowdIcon({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 20 20" width="18" height="18" aria-hidden focusable="false">
      <g fill="currentColor">
        <circle cx="10" cy="6.2" r="3" />
        <path d="M4.4 16.5c0-3.1 2.5-5.6 5.6-5.6s5.6 2.5 5.6 5.6z" />
        <circle cx="4" cy="8.4" r="2.2" opacity=".55" />
        <circle cx="16" cy="8.4" r="2.2" opacity=".55" />
      </g>
    </svg>
  );
}

export function SparkIcon({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 20 20" width="18" height="18" aria-hidden focusable="false">
      <path
        d="M10 1.5c.7 4 1.9 5.8 6 6.5-4.1.7-5.3 2.5-6 6.5-.7-4-1.9-5.8-6-6.5 4.1-.7 5.3-2.5 6-6.5z"
        fill="currentColor"
      />
      <path d="M16 14c.3 1.6.8 2.4 2.4 2.7-1.6.3-2.1 1.1-2.4 2.7-.3-1.6-.8-2.4-2.4-2.7 1.6-.3 2.1-1.1 2.4-2.7z" fill="currentColor" opacity=".6" />
    </svg>
  );
}

export function CloseIcon({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 16 16" width="15" height="15" aria-hidden focusable="false">
      <path d="M3.5 3.5l9 9M12.5 3.5l-9 9" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
    </svg>
  );
}

export function ChevronIcon({ className }: Props) {
  return (
    <svg className={className} viewBox="0 0 16 16" width="14" height="14" aria-hidden focusable="false">
      <path d="M4 10.5 8 6.5l4 4" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
