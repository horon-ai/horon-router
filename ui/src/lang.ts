import { detect, translator, type Key, type Lang, type Params, type Tr } from "./i18n.ts";

const STORE = "cliproxy-lang";
const books = { en: translator("en"), ko: translator("ko") };
const saved = () => {
  try {
    return localStorage.getItem(STORE);
  } catch {
    return null;
  }
};
let current: Lang = detect(navigator.languages, saved());
const listeners = new Set<() => void>();
document.documentElement.lang = current;

export const t: Tr = Object.defineProperty((key: Key, params?: Params) => books[current](key, params), "lang", {
  get: () => current,
}) as Tr;
export const lang = () => current;
export const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
};
export const getSnapshot = lang;
export function setLang(next: Lang) {
  if (next === current) return;
  current = next;
  document.documentElement.lang = next;
  try {
    localStorage.setItem(STORE, next);
  } catch {}
  listeners.forEach((listener) => listener());
}
