// Pure i18n core: no DOM, no runes, so core.ts and its tests can use it under plain Node.
// The reactive current language lives in lang.ts.
import { en } from "./locales/en.ts";
import { ko } from "./locales/ko.ts";

export type Key = keyof typeof en;
/** A message, or [one, other] when the English plural differs. Placeholders are {name}. */
export type Msg = string | readonly [one: string, other: string];
export type Params = Record<string, string | number>;
export type Lang = "en" | "ko";
export type Tr = ((key: Key, params?: Params) => string) & { readonly lang: Lang };

/** Each language named in itself, for the switch. */
export const langs: Record<Lang, string> = { en: "English", ko: "한국어" };
const books: Record<Lang, Record<Key, Msg>> = { en, ko };

export function translator(lang: Lang): Tr {
  const plural = new Intl.PluralRules(lang);
  const tr = (key: Key, params?: Params) => {
    let msg = books[lang][key];
    // {n} picks the form: English has one and other, Korean only other.
    if (typeof msg !== "string") msg = msg[plural.select(Number(params?.n)) === "one" ? 0 : 1];
    return params ? msg.replace(/\{(\w+)\}/g, (_, name) => String(params[name] ?? `{${name}}`)) : msg;
  };
  return Object.assign(tr, { lang });
}

/** A saved choice wins; otherwise the first browser language we have a dictionary for; otherwise English. */
export function detect(languages: readonly string[], saved?: string | null): Lang {
  if (saved === "en" || saved === "ko") return saved;
  for (const tag of languages) {
    const primary = tag.toLowerCase().split("-")[0];
    if (primary === "ko" || primary === "en") return primary;
  }
  return "en";
}
