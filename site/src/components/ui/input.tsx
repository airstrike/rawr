// shadcn's Input and Textarea in rawr's look: square, monospace, on the inset colour.
import * as React from "react";
import { cn } from "../../lib/utils.ts";

export function Input({ className, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      data-slot="input"
      className={cn(
        "h-[30px] min-w-0 rounded-none border border-line bg-raised px-3 font-mono text-[13px] text-foreground outline-none placeholder:text-dim hover:border-dim focus-visible:border-dim",
        className,
      )}
      {...props}
    />
  );
}

export function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        "w-full resize-y rounded-none border border-line bg-inset p-3 font-mono text-[13px] leading-[1.55] text-foreground outline-none placeholder:text-dim focus-visible:border-dim",
        className,
      )}
      {...props}
    />
  );
}
