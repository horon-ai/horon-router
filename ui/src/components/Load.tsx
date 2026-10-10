import { Button } from "./ui/button";
import type { ReactNode } from "react";
import { ApiError } from "../api.ts";
import { t } from "../lang.ts";
import { Rich } from "./Rich.tsx";

type Read<T> = {
  data: T | undefined;
  error: Error | null;
  isPending: boolean;
  refetch: () => unknown;
};

export function Load<T>({ res, what, children }: {
  res: Read<T>;
  what: string;
  children: (data: T) => ReactNode;
}) {
  const error = res.error;
  const missing = error instanceof ApiError && error.missing;
  if (res.data !== undefined && !missing) {
    return <>
      {error && <p className="note error" role="alert">
        <span className="lamp bad" />{t("load.stale", { message: error.message })}
        <Button variant="quiet" size="small" onClick={() => void res.refetch()}>{t("common.retry")}</Button>
      </p>}
      {children(res.data)}
    </>;
  }
  if (error instanceof ApiError && missing) {
    return <div className="state">
      <div className="row"><span className="lamp off" />{t("load.missing")}</div>
      <p><Rich text={t("load.missingBody", { call: `${error.method} ${error.path}`, status: error.status })} /></p>
    </div>;
  }
  if (error) {
    return <div className="state" role="alert">
      <div className="row"><span className="lamp bad" />{t("load.failed", { what: what.toLowerCase() })}</div>
      <p>{error.message}</p>
      <div className="actions">
        <Button size="small" onClick={() => void res.refetch()}>{t("common.tryAgain")}</Button>
      </div>
    </div>;
  }
  return <div aria-busy="true" aria-label={t("load.busy", { what: what.toLowerCase() })}>
    <div className="skel" />
  </div>;
}
