import type { ComponentProps } from "solid-js"

function withClass(base: string, extra?: string): string {
  return extra ? `${base} ${extra}` : base
}

function Table(props: ComponentProps<"table">) {
  return (
    <div class="relative w-full overflow-auto">
      <table class={withClass("w-full caption-bottom text-sm", props.class)}>{props.children}</table>
    </div>
  )
}

function TableHeader(props: ComponentProps<"thead">) {
  return <thead class={withClass("[&_tr]:border-b", props.class)}>{props.children}</thead>
}

function TableBody(props: ComponentProps<"tbody">) {
  return <tbody class={withClass("[&_tr:last-child]:border-0", props.class)}>{props.children}</tbody>
}

function TableFooter(props: ComponentProps<"tfoot">) {
  return (
    <tfoot class={withClass("bg-primary font-medium text-primary-foreground", props.class)}>{props.children}</tfoot>
  )
}

function TableRow(props: ComponentProps<"tr">) {
  return (
    <tr
      class={withClass("border-b transition-colors hover:bg-muted/50 data-[state=selected]:bg-muted", props.class)}
    >{props.children}</tr>
  )
}

function TableHead(props: ComponentProps<"th">) {
  return (
    <th
      class={withClass("h-10 px-2 text-left align-middle font-medium text-muted-foreground [&:has([role=checkbox])]:pr-0", props.class)}
    >{props.children}</th>
  )
}

function TableCell(props: ComponentProps<"td">) {
  return (
    <td class={withClass("p-2 align-middle [&:has([role=checkbox])]:pr-0", props.class)}>{props.children}</td>
  )
}

function TableCaption(props: ComponentProps<"caption">) {
  return <caption class={withClass("mt-4 text-sm text-muted-foreground", props.class)}>{props.children}</caption>
}

export { Table, TableHeader, TableBody, TableFooter, TableHead, TableRow, TableCell, TableCaption }
