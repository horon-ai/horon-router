import type { ComponentProps } from "react";

function Skeleton({ className = "", ...props }: ComponentProps<"div">) {
  return <div className={`skel ${className}`.trim()} {...props} />;
}

export { Skeleton };
