// shadcn's Button in rawr's look: square, 30 px tall, with an uppercase
// monospace label; the primary one is pink.
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";
import { cn } from "../../lib/utils.ts";

export const buttonVariants = cva(
  "inline-flex h-[30px] shrink-0 cursor-pointer items-center justify-center gap-2 whitespace-nowrap rounded-none border px-3 font-mono text-[11px] leading-[1.4] tracking-[0.06em] uppercase outline-none select-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-pink disabled:cursor-default disabled:opacity-60",
  {
    variants: {
      variant: {
        default: "border-line bg-raised text-foreground hover:border-dim",
        primary: "border-pink bg-pink text-primary-foreground hover:border-[var(--pink-hover)] hover:bg-[var(--pink-hover)]",
        ghost: "border-transparent bg-transparent text-dim hover:text-foreground",
      },
    },
    defaultVariants: { variant: "default" },
  },
);

export interface ButtonProps extends React.ComponentProps<"button">, VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

export function Button({ className, variant, asChild = false, ...props }: ButtonProps) {
  const Component = asChild ? Slot : "button";
  return <Component data-slot="button" className={cn(buttonVariants({ variant }), className)} {...props} />;
}
