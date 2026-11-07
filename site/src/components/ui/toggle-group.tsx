// shadcn's ToggleGroup as rawr's segmented control: joined square cells,
// the chosen one ink on paper inverted.
import * as ToggleGroupPrimitive from "@radix-ui/react-toggle-group";
import * as React from "react";
import { cn } from "../../lib/utils.ts";

export function ToggleGroup({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Root>) {
  return <ToggleGroupPrimitive.Root data-slot="toggle-group" className={cn("inline-flex", className)} {...props} />;
}

export const toggleItemClass =
  "inline-flex h-[30px] min-w-0 cursor-pointer items-center justify-center whitespace-nowrap rounded-none border-2 border-ink bg-transparent px-3 font-sans text-[12px] font-bold leading-[1.2] tracking-[0.08em] uppercase text-quiet outline-none select-none [&+&]:border-l-0 hover:text-ink focus-visible:z-10 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-red data-[state=on]:bg-ink data-[state=on]:text-paper";

export function ToggleGroupItem({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Item>) {
  return <ToggleGroupPrimitive.Item data-slot="toggle-group-item" className={cn(toggleItemClass, className)} {...props} />;
}
