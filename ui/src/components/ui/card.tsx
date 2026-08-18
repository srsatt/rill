import type { ComponentProps } from "solid-js"

function withClass(base: string, extra?: string): string {
  return extra ? `${base} ${extra}` : base
}

function Card(props: ComponentProps<"div">) {
  return (
    <div class={withClass("rounded-lg border bg-card text-card-foreground shadow-sm", props.class)}>
      {props.children}
    </div>
  )
}

function CardHeader(props: ComponentProps<"div">) {
  return <div class={withClass("flex flex-col space-y-1.5 p-6", props.class)}>{props.children}</div>
}

function CardTitle(props: ComponentProps<"h3">) {
  return (
    <h3 class={withClass("text-lg font-semibold leading-none tracking-tight", props.class)}>{props.children}</h3>
  )
}

function CardDescription(props: ComponentProps<"p">) {
  return <p class={withClass("text-sm text-muted-foreground", props.class)}>{props.children}</p>
}

function CardContent(props: ComponentProps<"div">) {
  return <div class={withClass("p-6 pt-0", props.class)}>{props.children}</div>
}

function CardFooter(props: ComponentProps<"div">) {
  return <div class={withClass("flex items-center p-6 pt-0", props.class)}>{props.children}</div>
}

export { Card, CardHeader, CardFooter, CardTitle, CardDescription, CardContent }
