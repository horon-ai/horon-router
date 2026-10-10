import type { ComponentProps } from "react";

function Badge({ className = "", ...props }: ComponentProps<"span">) {
  return <span className={`chip ${className}`.trim()} {...props} />;
}

export { Badge };
