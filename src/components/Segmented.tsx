import type { LucideIcon } from "lucide-react";

export type SegmentedOption<T extends string> = {
  value: T;
  label: string;
  Icon?: LucideIcon;
};

/**
 * Compact mutually-exclusive picker. Uses radio semantics rather than a row
 * of toggle buttons so screen readers announce it as one choice of N.
 */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: SegmentedOption<T>[];
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          className={`segmented__option${
            option.value === value ? " is-active" : ""
          }`}
          onClick={() => onChange(option.value)}
        >
          {option.Icon && <option.Icon className="icon-sm" aria-hidden="true" />}
          {option.label}
        </button>
      ))}
    </div>
  );
}
