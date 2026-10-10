import type { ComponentProps } from "react";

function Textarea({ className = "", ...props }: ComponentProps<"textarea">) {
  return <textarea className={`field-input ${className}`.trim()} {...props} />;
}

export { Textarea };
