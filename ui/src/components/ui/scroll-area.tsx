import { ScrollArea as BaseScrollArea } from "@base-ui/react/scroll-area";
import type { ComponentProps } from "react";

function ScrollArea({ className = "", children, ...props }: ComponentProps<typeof BaseScrollArea.Root>) {
  return <BaseScrollArea.Root className={`scroll ${className}`.trim()} {...props}><BaseScrollArea.Viewport>{children}</BaseScrollArea.Viewport></BaseScrollArea.Root>;
}

export { ScrollArea };
