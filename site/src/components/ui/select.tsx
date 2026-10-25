// shadcn's Select in rawr's look: a square trigger like the other controls,
// and a list whose group names keep their case, as the docs' sidebar shows them.
import * as SelectPrimitive from "@radix-ui/react-select";
import * as React from "react";
import { cn } from "../../lib/utils.ts";

export const Select = SelectPrimitive.Root;
export const SelectGroup = SelectPrimitive.Group;
export const SelectValue = SelectPrimitive.Value;

const Chevron = () => (
  <svg viewBox="0 0 12 12" aria-hidden="true" className="size-3 shrink-0 text-dim">
    <path d="M2.5 4.5L6 8L9.5 4.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
  </svg>
);

export function SelectTrigger({ className, children, ...props }: React.ComponentProps<typeof SelectPrimitive.Trigger>) {
  return (
    <SelectPrimitive.Trigger
      data-slot="select-trigger"
      className={cn(
        "inline-flex h-[30px] min-w-0 cursor-pointer items-center justify-between gap-2 rounded-none border border-line bg-raised px-3 text-left font-mono text-[11px] leading-[1.4] tracking-[0.06em] text-foreground outline-none select-none hover:border-dim focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-pink data-[state=open]:border-dim [&>span]:truncate",
        className,
      )}
      {...props}
    >
      {children}
      <SelectPrimitive.Icon asChild>
        <Chevron />
      </SelectPrimitive.Icon>
    </SelectPrimitive.Trigger>
  );
}

export function SelectContent({ className, children, position = "popper", ...props }: React.ComponentProps<typeof SelectPrimitive.Content>) {
  return (
    <SelectPrimitive.Portal>
      <SelectPrimitive.Content
        data-slot="select-content"
        position={position}
        sideOffset={4}
        className={cn(
          "relative z-50 max-h-[min(var(--radix-select-content-available-height),70vh)] min-w-[var(--radix-select-trigger-width)] overflow-x-hidden overflow-y-auto rounded-none border border-line bg-raised text-foreground shadow-[0_12px_32px_rgb(0_0_0/0.35)]",
          className,
        )}
        {...props}
      >
        <SelectPrimitive.Viewport className="p-1">{children}</SelectPrimitive.Viewport>
      </SelectPrimitive.Content>
    </SelectPrimitive.Portal>
  );
}

export function SelectLabel({ className, ...props }: React.ComponentProps<typeof SelectPrimitive.Label>) {
  return <SelectPrimitive.Label data-slot="select-label" className={cn("px-2 pt-2.5 pb-1 font-mono text-[12px] leading-[1.4] tracking-[0.04em] text-dim", className)} {...props} />;
}

export function SelectItem({ className, children, ...props }: React.ComponentProps<typeof SelectPrimitive.Item>) {
  return (
    <SelectPrimitive.Item
      data-slot="select-item"
      className={cn(
        "relative flex w-full cursor-pointer items-center gap-2 rounded-none border-l border-line py-1.5 pr-8 pl-3 font-sans text-[14px] leading-[1.4] text-foreground outline-none select-none data-[highlighted]:border-pink data-[highlighted]:bg-inset data-[state=checked]:text-ink",
        className,
      )}
      {...props}
    >
      <SelectPrimitive.ItemText>{children}</SelectPrimitive.ItemText>
      <span className="absolute right-2 flex size-3 items-center justify-center">
        <SelectPrimitive.ItemIndicator>
          <span className="block size-2 bg-pink" />
        </SelectPrimitive.ItemIndicator>
      </span>
    </SelectPrimitive.Item>
  );
}

export function SelectSeparator({ className, ...props }: React.ComponentProps<typeof SelectPrimitive.Separator>) {
  return <SelectPrimitive.Separator data-slot="select-separator" className={cn("my-1 h-px bg-line", className)} {...props} />;
}
