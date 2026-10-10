import { api, configValue } from "./api.ts";
import { equal } from "./core.ts";
import { t } from "./lang.ts";
import { keys, queryClient } from "./queries.ts";

/** Save the reviewed value only if the server still has the opening snapshot. */
export async function saveEditor(
  path: string,
  baseline: unknown,
  body: unknown,
  yaml: boolean,
  isAlive: () => boolean,
): Promise<boolean> {
  const latest = yaml
    ? await api(path, "GET", undefined, "text")
    : await configValue(path, Array.isArray(baseline) ? [] : {});
  if (!isAlive()) return false;
  if (!equal(latest, baseline)) throw new Error(t("editor.changed"));
  await api(path, "PUT", body);
  await queryClient.invalidateQueries({ queryKey: keys.config });
  return isAlive();
}
