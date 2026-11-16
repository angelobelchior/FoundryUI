import { cx } from "../lib/cx";

type SpinnerProps = {
  size?: number;
  className?: string;
};

export function Spinner({ size = 14, className }: SpinnerProps) {
  return (
    <span
      aria-hidden="true"
      className={cx(
        "inline-block shrink-0 animate-spin rounded-full border-2 border-current border-t-transparent",
        className,
      )}
      style={{ width: size, height: size }}
    />
  );
}
