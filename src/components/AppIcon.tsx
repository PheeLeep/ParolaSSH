import { useId, type SVGProps } from "react";

/**
 * The shipped app icon - the same lighthouse the taskbar and tray show -
 * redrawn as inline SVG so the in-app brand matches the OS one.
 *
 * `variant` mirrors the two design masters: "detailed" is `design/icon.svg`
 * (beam gradient, lantern glow, tower stripes) for 40px and up; "simple" is
 * `design/icon-small.svg`, which drops the detail that turns to mud below that.
 * Both carry their own squircle and background, so containers must not paint one.
 */
export function AppIcon({
  variant = "detailed",
  ...props
}: SVGProps<SVGSVGElement> & { variant?: "detailed" | "simple" }) {
  // Two instances can share a page (navbar + About), and duplicate defs ids
  // would make one steal the other's paint.
  const uid = useId();
  const clip = `parola-squircle-${uid}`;
  const beam = `parola-beam-${uid}`;
  const glow = `parola-glow-${uid}`;

  return (
    <svg viewBox="0 0 256 256" aria-hidden="true" {...props}>
      <defs>
        <clipPath id={clip}>
          <rect width="256" height="256" rx="58" />
        </clipPath>
        {variant === "detailed" && (
          <>
            <linearGradient
              id={beam}
              x1="150"
              y1="57"
              x2="76"
              y2="57"
              gradientUnits="userSpaceOnUse"
            >
              <stop offset="0" stopColor="#D6BCFF" />
              <stop offset="1" stopColor="#7C3AED" stopOpacity="0.75" />
            </linearGradient>
            <radialGradient
              id={glow}
              cx="168"
              cy="56"
              r="84"
              gradientUnits="userSpaceOnUse"
            >
              <stop offset="0" stopColor="#8B5CF6" stopOpacity="0.4" />
              <stop offset="1" stopColor="#8B5CF6" stopOpacity="0" />
            </radialGradient>
          </>
        )}
      </defs>

      <g clipPath={`url(#${clip})`}>
        <rect width="256" height="256" fill="#0D0B16" />
        {variant === "detailed" ? (
          <g transform="translate(-2,18)">
            <rect y="216" width="256" height="40" fill="#08060F" />
            <circle cx="168" cy="56" r="84" fill={`url(#${glow})`} />
            <polyline
              points="76,14 150,57 76,100"
              fill="none"
              stroke={`url(#${beam})`}
              strokeWidth="22"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
            <polygon points="141,216 155,75 181,75 195,216" fill="#EDEBF4" />
            <polygon points="151.5,110 184.5,110 186.3,128 149.7,128" fill="#0D0B16" />
            <polygon points="147.4,152 188.6,152 190.4,170 145.6,170" fill="#0D0B16" />
            <rect x="148" y="66" width="40" height="9" rx="3.5" fill="#EDEBF4" />
            <rect x="156" y="46" width="24" height="20" rx="3.5" fill="#A78BFA" />
            <polygon points="150,46 186,46 168,26" fill="#EDEBF4" />
          </g>
        ) : (
          <g transform="translate(-4,16)">
            <polyline
              points="72,16 148,58 72,100"
              fill="none"
              stroke="#A78BFA"
              strokeWidth="28"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
            <polygon points="132,214 152,74 186,74 206,214" fill="#EDEBF4" />
            <rect x="142" y="60" width="54" height="14" rx="5" fill="#EDEBF4" />
            <rect x="153" y="34" width="32" height="26" rx="5" fill="#A78BFA" />
            <polygon points="142,35 196,35 169,10" fill="#EDEBF4" />
          </g>
        )}
      </g>
    </svg>
  );
}
