/**
 * The Arya brand mark: a terracotta "planet" with four feature satellites
 * orbiting it — notes, dictation (waveform), a spark of intelligence, and
 * search. Recreated as vector art so it stays crisp at any size and follows the
 * warm palette. Pass `size` in px.
 */
export function Logo({ size = 96, title = "Arya" }: { size?: number; title?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 512 512"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      role="img"
      aria-label={title}
    >
      <defs>
        <radialGradient id="arya-planet" cx="38%" cy="34%" r="72%">
          <stop offset="0%" stopColor="#d17650" />
          <stop offset="55%" stopColor="#be5a38" />
          <stop offset="100%" stopColor="#a94a2c" />
        </radialGradient>
        <filter id="arya-sat" x="-40%" y="-40%" width="180%" height="180%">
          <feDropShadow dx="0" dy="4" stdDeviation="6" floodColor="#3a2a1e" floodOpacity="0.22" />
        </filter>
      </defs>

      {/* orbit */}
      <circle cx="256" cy="256" r="180" stroke="#7a5745" strokeWidth="3" opacity="0.8" />

      {/* planet */}
      <circle cx="256" cy="256" r="104" fill="url(#arya-planet)" />

      {/* top — notes / list */}
      <g filter="url(#arya-sat)">
        <circle cx="256" cy="76" r="50" fill="#f1e9dc" />
      </g>
      <g stroke="#6d4c3d" strokeWidth="6" strokeLinecap="round">
        <line x1="240" y1="63" x2="273" y2="63" />
        <line x1="240" y1="76" x2="273" y2="76" />
        <line x1="240" y1="89" x2="264" y2="89" />
      </g>

      {/* right — dictation waveform */}
      <g filter="url(#arya-sat)">
        <circle cx="436" cy="256" r="50" fill="#f1e9dc" />
      </g>
      <g stroke="#6d4c3d" strokeWidth="5.5" strokeLinecap="round">
        <line x1="418" y1="249" x2="418" y2="263" />
        <line x1="428" y1="238" x2="428" y2="274" />
        <line x1="438" y1="245" x2="438" y2="267" />
        <line x1="448" y1="234" x2="448" y2="278" />
        <line x1="458" y1="250" x2="458" y2="262" />
      </g>

      {/* bottom — spark */}
      <g filter="url(#arya-sat)">
        <circle cx="256" cy="436" r="50" fill="#f1e9dc" />
      </g>
      <path
        d="M256 408c3.4 16.2 7.4 20.2 23.6 23.6C263.4 435 259.4 439 256 455.2 252.6 439 248.6 435 232.4 431.6 248.6 428.2 252.6 424.2 256 408Z"
        fill="#6d4c3d"
      />

      {/* left — search */}
      <g filter="url(#arya-sat)">
        <circle cx="76" cy="256" r="50" fill="#f1e9dc" />
      </g>
      <g stroke="#6d4c3d" strokeWidth="6" strokeLinecap="round" fill="none">
        <circle cx="70" cy="250" r="16" />
        <line x1="82" y1="262" x2="95" y2="275" />
      </g>
    </svg>
  );
}
