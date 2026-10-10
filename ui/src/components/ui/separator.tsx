import { Separator as BaseSeparator } from "@base-ui/react/separator";
import type { ComponentProps } from "react";

function Separator({ className = "", ...props }: ComponentProps<typeof BaseSeparator>) {
  return <BaseSeparator className={className} {...props} />;
}

export { Separator };
