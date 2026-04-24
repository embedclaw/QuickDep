import type { Tone } from "../lib/presenters";

type StatusBadgeProps = {
  label: string;
  tone?: Tone;
};

export function StatusBadge({
  label,
  tone = "neutral",
}: StatusBadgeProps) {
  return (
    <span className={`status-badge status-badge--${tone}`}>
      <span className="status-badge__dot" />
      {label}
    </span>
  );
}
