import type { ComponentProps } from "solid-js"

type BadgeVariant = "default" | "secondary" | "outline" | "success" | "warning" | "error"

function badgeVariants(options?: { variant?: BadgeVariant | null }): string {
  let variantClass = "border-transparent bg-primary text-primary-foreground"
  const variant = options?.variant
  if (variant === "secondary") variantClass = "border-transparent bg-secondary text-secondary-foreground"
  else if (variant === "outline") variantClass = "text-foreground"
  else if (variant === "success") variantClass = "border-success-foreground bg-success text-success-foreground"
  else if (variant === "warning") variantClass = "border-warning-foreground bg-warning text-warning-foreground"
  else if (variant === "error") variantClass = "border-error-foreground bg-error text-error-foreground"
  return `inline-flex items-center rounded-md border px-2.5 py-0.5 text-xs font-semibold transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 ${variantClass}`
}

type BadgeProps = ComponentProps<"div"> &
  { variant?: BadgeVariant | null } & {
    round?: boolean
  }

function Badge(props: BadgeProps) {
  return (
    <div
      class={`${badgeVariants({ variant: props.variant })}${props.round ? " rounded-full" : ""}${props.class ? ` ${props.class}` : ""}`}
    >{props.children}</div>
  )
}

export type { BadgeProps }
export { Badge, badgeVariants }
