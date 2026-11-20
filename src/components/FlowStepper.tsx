import { CheckIcon } from "./Icon";
import { cx } from "../lib/cx";

export type FlowStep = number;

type FlowStepperStep = {
  value: FlowStep;
  label: string;
};

type FlowStepperProps = {
  label: string;
  steps: readonly FlowStepperStep[];
  current: FlowStep;
  furthest: FlowStep;
  onSelect: (step: FlowStep) => void;
};

function gridColumns(count: number): string {
  if (count <= 3) {
    return "grid-cols-3";
  }

  return "grid-cols-4";
}

export function FlowStepper({ label, steps, current, furthest, onSelect }: FlowStepperProps) {
  return (
    <nav aria-label={label} className="border-b border-line pb-2.5">
      <ol className={cx("grid gap-1", gridColumns(steps.length))}>
        {steps.map((step) => {
          const done = step.value < current;
          const active = step.value === current;
          const locked = step.value > furthest;

          return (
            <li key={step.value} className="min-w-0">
              <button
                type="button"
                onClick={() => onSelect(step.value)}
                disabled={locked}
                aria-current={active ? "step" : undefined}
                title={step.label}
                className={cx(
                  "relative flex w-full min-w-0 flex-col items-start gap-1 rounded-[6px] px-1.5 py-1 text-left transition-colors sm:flex-row sm:items-center sm:gap-2",
                  !locked && "hover:bg-hover",
                  locked && "cursor-not-allowed",
                )}
              >
                <span
                  className={cx(
                    "inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full border text-[10.5px] font-semibold tabular-nums",
                    active
                      ? "border-accent bg-accent text-accent-contrast"
                      : done
                        ? "border-accent bg-accent/10 text-accent-text"
                        : "border-line bg-canvas text-tertiary",
                  )}
                >
                  {done ? <CheckIcon size={11} /> : step.value}
                </span>
                <span
                  className={cx(
                    "min-w-0 max-w-full truncate text-[11.5px] font-medium",
                    active ? "text-primary" : locked ? "text-tertiary" : "text-secondary",
                  )}
                >
                  {step.label}
                </span>
                {active ? (
                  <span
                    aria-hidden="true"
                    className="absolute inset-x-1.5 -bottom-2.5 h-[2px] rounded-full bg-accent"
                  />
                ) : null}
              </button>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}
