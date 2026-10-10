import { useEffect, useSyncExternalStore } from "react";
import { t } from "../lang.ts";
import { session } from "../session.ts";

export function Missing({ actions }: { actions: [method: string, path: string, name: string][] }) {
  useSyncExternalStore(session.subscribe, () => session.getSnapshot().caps);
  useEffect(() => { session.probe(actions); }, [actions]);
  const off = actions.filter(([method, path]) => !session.can(method, path))
    .map((action) => action[2]);
  return off.length ? <p className="note">
    <span className="lamp off" />{t("missing.off", { list: off.join(", ") })}
  </p> : null;
}
