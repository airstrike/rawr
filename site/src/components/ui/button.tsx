// shadcn's Button in the work plan's look: square, 30 px tall, with an
// uppercase label; the primary one is ink on paper inverted, red under the pointer.
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";
import { cn } from "../../lib/utils.ts";

export const buttonVariants = cva(
  "inline-flex h-[30px] shrink-0 cursor-pointer items-center justify-center gap-2 whitespace-nowrap rounded-none border-2 px-3 font-sans text-[12px] font-bold leading-[1.2] tracking-[0.08em] uppercase outline-none select-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-red disabled:cursor-default disabled:opacity-60",
  {
    variants: {
      variant: {
        default: "border-ink bg-transparent text-ink hover:bg-ink hover:text-paper",
        primary: "border-ink bg-ink text-paper hover:border-red hover:bg-red hover:text-on-red",
        ghost: "border-transparent bg-transparent text-quiet hover:text-ink",
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
