import { Button as BaseButton } from "@base-ui/react/button";
import type { ComponentProps } from "react";

type ButtonProps = ComponentProps<typeof BaseButton> & {
  variant?: "default" | "primary" | "quiet" | "danger";
  size?: "default" | "small" | "round";
};

function Button({ className = "", variant = "default", size = "default", ...props }: ButtonProps) {
  return <BaseButton className={`key ${variant === "default" ? "" : variant} ${size === "default" ? "" : size} ${className}`.trim()} {...props} />;
}

export { Button };
