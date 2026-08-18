import type { ComponentProps, JSX } from "solid-js";
import { Badge, type BadgeProps } from "../components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../components/ui/table";

// Solid's SSR transform represents reactive component children as accessors.
// ScriptC does not lower accessor-bearing object literals yet. These direct-call
// adapters preserve the real registry primitives while passing their immutable
// server-rendered children as ordinary values.
export function card(children: JSX.Element, className?: string): JSX.Element {
  return Card({ class: className, children } as ComponentProps<"div">);
}

export function cardHeader(children: JSX.Element, className?: string): JSX.Element {
  return CardHeader({ class: className, children } as ComponentProps<"div">);
}

export function cardContent(children: JSX.Element, className?: string): JSX.Element {
  return CardContent({ class: className, children } as ComponentProps<"div">);
}

export function cardTitle(children: JSX.Element, className?: string): JSX.Element {
  return CardTitle({ class: className, children } as ComponentProps<"h3">);
}

export function cardDescription(children: JSX.Element, className?: string): JSX.Element {
  return CardDescription({ class: className, children } as ComponentProps<"p">);
}

export function badge(children: JSX.Element, variant: BadgeProps["variant"] = "default"): JSX.Element {
  return Badge({ variant, children } as BadgeProps);
}

export function table(children: JSX.Element): JSX.Element {
  return Table({ children } as ComponentProps<"table">);
}

export function tableHeader(children: JSX.Element): JSX.Element {
  return TableHeader({ children } as ComponentProps<"thead">);
}

export function tableBody(children: JSX.Element): JSX.Element {
  return TableBody({ children } as ComponentProps<"tbody">);
}

export function tableRow(children: JSX.Element): JSX.Element {
  return TableRow({ children } as ComponentProps<"tr">);
}

export function tableHead(children: JSX.Element): JSX.Element {
  return TableHead({ children } as ComponentProps<"th">);
}

export function tableCell(children: JSX.Element): JSX.Element {
  return TableCell({ children } as ComponentProps<"td">);
}
