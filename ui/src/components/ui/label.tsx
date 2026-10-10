import { Field } from "@base-ui/react/field";
import type { ComponentProps } from "react";

function FieldRoot({ className = "", ...props }: ComponentProps<typeof Field.Root>) {
  return <Field.Root className={`field ${className}`.trim()} {...props} />;
}
function Label({ className = "", ...props }: ComponentProps<typeof Field.Label>) {
  return <Field.Label className={className} {...props} />;
}

export { FieldRoot, Label };
