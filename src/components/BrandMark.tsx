import type { SVGProps } from "react";

/**
 * The app icon's lighthouse, redrawn for the navbar mark. At 18px the tower
 * stripes and the beam gradient of `design/icon.svg` turn to mud, so this keeps
 * only the silhouette and the `>` the beam forms. Fills use `currentColor`, so
 * the mark follows whatever the surrounding chip sets.
 */
export function BrandMark(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" {...props}>
      <g transform="translate(0.8,0)">
        <polyline
          points="2.6,3 13.8,8.2 2.6,13.4"
          fill="none"
          stroke="currentColor"
          strokeWidth="3"
          strokeLinecap="round"
          strokeLinejoin="round"
          opacity="0.85"
        />
        <polygon points="12.2,22 14.3,10.4 19.1,10.4 21.2,22" />
        <rect x="12.8" y="8.4" width="7.8" height="2" rx="0.8" />
        <rect x="14.5" y="4.9" width="4.4" height="3.5" rx="1.1" />
        <polygon points="13,5.1 20.4,5.1 16.7,1.6" />
      </g>
    </svg>
  );
}
