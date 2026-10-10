import { Input as BaseInput } from "@base-ui/react/input";
import type { ComponentProps } from "react";

function Input({ className = "", ...props }: ComponentProps<typeof BaseInput>) {
  return <BaseInput className={`field-input ${className}`.trim()} {...props} />;
}

export { Input };
