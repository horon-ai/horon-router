import { Button } from "./components/ui/button";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { text } from "./api.ts";
import { lineDiff } from "./core.ts";
import { getSnapshot as language, subscribe as subscribeLang, t } from "./lang.ts";
import { session } from "./session.ts";
import { saveEditor } from "./editor-save.ts";

type Props = {
  path: string;
  value: unknown;
  title: string;
  yaml?: boolean;
  onclose: (saved: boolean) => void;
};

export default function Editor({ path, value, title, yaml = false, onclose }: Props) {
  useSyncExternalStore(subscribeLang, language);
  const [baseline] = useState(() => structuredClone(value));
  const [before] = useState(() => yaml ? String(baseline) : JSON.stringify(baseline, null, 2));
  const [draft, setDraft] = useState(before);
  const [review, setReview] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const alive = useRef(false);
  const dirty = draft !== before;

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => { session.dirty = dirty; }, [dirty]);

  function check() {
    try {
      if (!yaml) JSON.parse(draft);
      setReview(true);
      setError("");
    } catch {
      setError(t("editor.badJson"));
    }
  }

  async function save() {
    const body = yaml ? draft : JSON.parse(draft);
    setSaving(true);
    setError("");
    try {
      if (!await saveEditor(path, baseline, body, yaml, () => alive.current)) return;
      session.dirty = false;
      session.notify(t("common.saved"));
      onclose(true);
    } catch (e) {
      if (alive.current) setError(text(e));
    } finally {
      if (alive.current) setSaving(false);
    }
  }

  function close() {
    if (dirty && !confirm(t("app.discard"))) return;
    session.dirty = false;
    onclose(false);
  }

  return <section className="section editor" aria-label={title}>
    <div className="section-head">
      <h2>{title}</h2>
      <Button variant="quiet" disabled={saving} onClick={close}>{t("common.close")}</Button>
    </div>
    <p className="legend mono">{path}</p>
    {error && <p className="note error" role="alert"><span className="lamp bad" />{error}</p>}
    {review ? <>
      <div className="code-window diff" aria-label={t("editor.changes")}>
        {lineDiff(before, draft).map((line, i) =>
          <div className={line.kind} key={i}>
            <span>{line.kind === "added" ? "+" : line.kind === "removed" ? "−" : ""}</span>{line.text}
          </div>)}
      </div>
      <div className="row">
        <Button variant="primary" disabled={saving || !dirty} onClick={save}>{saving ? t("editor.saving") : t("editor.apply")}</Button>
        <Button disabled={saving} onClick={() => setReview(false)}>{t("editor.keep")}</Button>
        <span className="legend">{yaml ? t("editor.yamlNote") : ""}</span>
      </div>
    </> : <>
      <label><span className="sr">{yaml ? "YAML" : "JSON"}</span>
        <textarea className="code" rows={22} spellCheck={false} autoFocus value={draft}
          onChange={(event) => setDraft(event.target.value)} />
      </label>
      <div className="row">
        <Button variant="primary" disabled={!dirty} onClick={check}>{t("editor.review")}</Button>
      </div>
    </>}
  </section>;
}
