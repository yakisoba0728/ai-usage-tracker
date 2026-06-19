import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

/*
 * Focus is handled globally (see index.css `:focus-visible`) so every
 * interactive surface gets one uniform white ring.
 */
const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md font-medium transition-[background-color,border-color,color,box-shadow,transform] duration-150 disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 [&_svg]:shrink-0 select-none cursor-default",
  {
    variants: {
      variant: {
        // The one solid action — white, dark ink.
        default:
          "bg-primary text-primary-foreground hover:bg-primary/90 active:bg-primary/80",
        outline:
          "border border-border-strong bg-surface text-text hover:bg-surface-2 hover:border-text-faint",
        secondary:
          "bg-surface-2 text-text hover:bg-white/[0.06]",
        // Quiet icon / text actions.
        ghost:
          "text-text-dim hover:bg-surface-2 hover:text-text",
        // Destructive-but-neutral — destructive intent reads from label/icon.
        destructive:
          "border border-border-strong bg-surface-2 text-text-dim hover:bg-surface-2 hover:text-text hover:border-text-faint",
        link: "text-text underline-offset-4 hover:underline",
      },
      size: {
        default: "h-9 px-3.5 text-sm has-[>svg]:px-3",
        sm: "h-8 rounded-md gap-1.5 px-3 text-xs has-[>svg]:px-2.5",
        lg: "h-10 rounded-md px-5 text-sm has-[>svg]:px-4",
        icon: "size-8",
        "icon-sm": "size-7 rounded-md",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot : "button";

  return (
    <Comp
      data-slot="button"
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };
