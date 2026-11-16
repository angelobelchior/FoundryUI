import type { ReactNode } from "react";

export type SummaryField = {
  label: string;
  value: ReactNode;
  wide?: boolean;
};

type TranscriptionSummaryProps = {
  title: string;
  fields: readonly SummaryField[];
  note?: ReactNode;
};

export function TranscriptionSummary({ title, fields, note }: TranscriptionSummaryProps) {
  return (
    <section className="min-w-0">
      <h2 className="text-[11px] font-semibold uppercase tracking-[0.055em] text-tertiary">{title}</h2>
      <dl className="mt-3 grid gap-x-6 gap-y-2.5 sm:grid-cols-2">
        {fields.map((field) => (
          <div key={field.label} className={field.wide ? "min-w-0 sm:col-span-2" : "min-w-0"}>
            <dt className="text-[10.5px] text-tertiary">{field.label}</dt>
            <dd className="mt-0.5 min-w-0 break-words text-[12.5px] text-primary">{field.value}</dd>
          </div>
        ))}
      </dl>
      {note ? <p className="mt-3 text-[11px] leading-relaxed text-tertiary">{note}</p> : null}
    </section>
  );
}
