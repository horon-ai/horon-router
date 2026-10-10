import { ToggleGroup as BaseToggleGroup } from "@base-ui/react/toggle-group";
import { Toggle as BaseToggle } from "@base-ui/react/toggle";
import type { ComponentProps } from "react";

function ToggleGroup({ className = "", ...props }: ComponentProps<typeof BaseToggleGroup>) {
  return <BaseToggleGroup className={`seg ${className}`.trim()} {...props} />;
}
function ToggleGroupItem({ className = "", ...props }: ComponentProps<typeof BaseToggle>) {
  return <BaseToggle className={className} {...props} />;
}

export { ToggleGroup, ToggleGroupItem };
