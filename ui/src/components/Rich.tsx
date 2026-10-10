import { Fragment } from "react";

export function Rich({ text }: { text: string }) {
  return <>{text.split("`").map((part, i) =>
    <Fragment key={i}>{i % 2 ? <code>{part}</code> : part}</Fragment>
  )}</>;
}
