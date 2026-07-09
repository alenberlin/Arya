/**
 * The Arya brand mark: the Newsreader serif "A" (the same face as the
 * wordmark) circled by a thin orbit ring with a single satellite dot that
 * emits two small sound arcs — voice orbiting your work, reduced to one
 * glyph. The letterform is baked in as a path so no font needs to be loaded.
 * Pass `size` in px.
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
        <linearGradient id="arya-a-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#C4714B" />
          <stop offset="100%" stopColor="#A34527" />
        </linearGradient>
      </defs>

      {/* orbit */}
      <ellipse
        cx="256"
        cy="262"
        rx="220"
        ry="82"
        transform="rotate(-20 256 262)"
        stroke="#B5623F"
        strokeOpacity="0.5"
        strokeWidth="9"
      />

      {/* Newsreader semibold "A", outlined */}
      <path
        d="M522-229L180.500-229L180.500-291.500L522-291.500L522-229M421.500-682L687.500-56L764.500-27.500L764.500 0L456.500 0L456.500-27.500L541-54.500L323.500-580L348-580L144-54.500L224-27.500L224 0L-7 0L-7-27.500L71-54.500L326-682"
        transform="translate(104.94,398.00) scale(0.39883)"
        fill="url(#arya-a-fill)"
      />

      {/* satellite */}
      <circle cx="436.2" cy="250.1" r="16" fill="#9A4E30" />

      {/* sound arcs radiating from the satellite */}
      <path
        d="M469.96 253.99 A34 34 0 0 1 440.61 283.86"
        stroke="#9A4E30"
        strokeOpacity="0.8"
        strokeWidth="9"
        strokeLinecap="round"
      />
      <path
        d="M487.84 256.03 A52 52 0 0 1 442.97 301.70"
        stroke="#9A4E30"
        strokeOpacity="0.55"
        strokeWidth="9"
        strokeLinecap="round"
      />
    </svg>
  );
}
