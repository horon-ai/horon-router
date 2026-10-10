import { span, type Window } from "../core.ts";
import { t } from "../lang.ts";

export function Meter({ w }: { w: Window }) {
  const resets = w.reset && Date.parse(w.reset) > Date.now()
    ? t("meter.resets", { span: span(Date.parse(w.reset) - Date.now(), t) }) : "";
  return <div className="quota">
    <span className="legend grow">{w.label}</span>
    {w.used === null ? <span className="legend detail">{w.detail}</span> : <>
      <span role="meter" aria-valuenow={w.used} aria-valuemin={0} aria-valuemax={100}
        aria-label={t("meter.used", { label: w.label })}>
        <svg className="grille" width="216" height="7" aria-hidden="true">
          {Array.from({ length: 20 }, (_, i) => <circle key={i} cx={i * 11 + 3.5} cy="3.5" r="3.5"
            className={i < Math.round(w.used! / 5) ? (w.used! >= 90 ? "f" : "l4") : ""} />)}
        </svg>
      </span>
      <span className="num count">{w.used}%</span>
      <span className="legend reset">{w.detail || resets}</span>
    </>}
  </div>;
}
