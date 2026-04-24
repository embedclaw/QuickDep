import type { PropsWithChildren, ReactNode } from "react";

type PanelProps = PropsWithChildren<{
  title: string;
  eyebrow?: string;
  tone?: "light" | "dark";
  actions?: ReactNode;
  className?: string;
}>;

export function Panel({
  title,
  eyebrow,
  tone = "light",
  actions,
  className,
  children,
}: PanelProps) {
  return (
    <section className={`panel panel--${tone}${className ? ` ${className}` : ""}`}>
      <header className="panel__header">
        <div>
          {eyebrow ? <p className="panel__eyebrow">{eyebrow}</p> : null}
          <h2 className="panel__title">{title}</h2>
        </div>
        {actions ? <div className="panel__actions">{actions}</div> : null}
      </header>
      <div className="panel__body">{children}</div>
    </section>
  );
}
