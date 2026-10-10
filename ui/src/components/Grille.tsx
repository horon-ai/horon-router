import { level, sum, type Buckets } from "../core.ts";
import { t } from "../lang.ts";

export function Grille({ data, max, d = 7, gap = 4, stack = 1 }: {
  data: Buckets;
  max: number;
  d?: number;
  gap?: number;
  stack?: number;
}) {
  const step = d + gap;
  const r = d / 2;
  const failed = sum(data.failed);
  return <svg className="grille" width={data.total.length * step - gap} height={stack * step - gap}
    role="img" aria-label={t("grille.label", { n: sum(data.total), min: data.total.length * 10 }) +
      (failed ? t("grille.failed", { n: failed }) : "")}>
    {data.total.flatMap((n, i) => {
      if (stack === 1) return <circle key={i} cx={i * step + r} cy={r} r={r}
        className={data.failed[i] ? "f" : `l${level(n, max)}`} />;
      const lit = n > 0 ? Math.max(1, Math.round((n / max) * stack)) : 0;
      return Array.from({ length: stack }, (_, j) =>
        <circle key={`${i}-${j}`} cx={i * step + r} cy={j * step + r} r={r}
          className={stack - j > lit ? "" : stack - j === lit && data.failed[i] ? "f" : "l4"} />
      );
    })}
  </svg>;
}
