import { pages } from "../session";
import { getSnapshot, subscribe, t } from "../lang";
import { useSyncExternalStore } from "react";

export function Placeholder({ page }: { page: string }) {
  useSyncExternalStore(subscribe, getSnapshot);
  const label = pages.find(([id]) => id === page)?.[1] || "page.overview";
  return <section className="section"><h1>{t(label)}</h1></section>;
}
