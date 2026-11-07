// shadcn's Input and Textarea in the work plan's look: square, monospace, on the wash colour.
import * as React from "react";
import { cn } from "../../lib/utils.ts";

export function Input({ className, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      data-slot="input"
      className={cn(
        "h-[30px] min-w-0 rounded-none border border-grid bg-paper px-3 font-mono text-[13px] text-ink outline-none placeholder:text-quiet hover:border-quiet focus-visible:border-ink",
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
        "w-full resize-y rounded-none border border-grid bg-wash p-3 font-mono text-[13px] leading-[1.55] text-ink outline-none placeholder:text-quiet focus-visible:border-ink",
        className,
      )}
      {...props}
    />
  );
}
