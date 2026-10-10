import { Tabs as BaseTabs } from "@base-ui/react/tabs";
import type { ComponentProps } from "react";

function Tabs({ className = "", ...props }: ComponentProps<typeof BaseTabs.Root>) {
  return <BaseTabs.Root className={className} {...props} />;
}
function TabsList({ className = "", ...props }: ComponentProps<typeof BaseTabs.List>) {
  return <BaseTabs.List className={`seg ${className}`.trim()} {...props} />;
}
function TabsTrigger({ className = "", ...props }: ComponentProps<typeof BaseTabs.Tab>) {
  return <BaseTabs.Tab className={`data-[active]:bg-[var(--raised)] data-[active]:text-[var(--ink)] data-[active]:shadow-[var(--lift)] ${className}`.trim()} {...props} />;
}
function TabsContent({ className = "", ...props }: ComponentProps<typeof BaseTabs.Panel>) {
  return <BaseTabs.Panel className={className} {...props} />;
}

export { Tabs, TabsList, TabsTrigger, TabsContent };
