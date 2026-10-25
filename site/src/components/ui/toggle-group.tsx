// shadcn's ToggleGroup as rawr's segmented control: joined square cells,
// the chosen one pink.
import * as ToggleGroupPrimitive from "@radix-ui/react-toggle-group";
import * as React from "react";
import { cn } from "../../lib/utils.ts";

export function ToggleGroup({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Root>) {
  return <ToggleGroupPrimitive.Root data-slot="toggle-group" className={cn("inline-flex", className)} {...props} />;
}

export const toggleItemClass =
  "inline-flex h-[30px] min-w-0 cursor-pointer items-center justify-center whitespace-nowrap rounded-none border border-line bg-raised px-3 font-mono text-[11px] leading-[1.4] tracking-[0.06em] text-foreground uppercase outline-none select-none [&+&]:border-l-0 hover:text-foreground focus-visible:z-10 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-pink data-[state=on]:border-pink data-[state=on]:bg-pink data-[state=on]:text-primary-foreground";

export function ToggleGroupItem({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Item>) {
  return <ToggleGroupPrimitive.Item data-slot="toggle-group-item" className={cn(toggleItemClass, className)} {...props} />;
}
