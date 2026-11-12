import { useCallback } from "react";
import { useLocation, useSearchParams } from "react-router";

/**
 * One URL parameter of the view, such as a filter (`?state=open`), so Back, reload and a
 * breadcrumb from a record page return to the same rows. Setting it replaces the history
 * entry and keeps the other parameters and the navigation state. `null` or an empty string
 * removes it. The value is the URL's text: narrow it with `useChoiceParam` before trusting it.
 */
function useSearchParam(
  name: string,
): [value: string | null, setValue: (value: string | null) => void] {
  const [params, setParams] = useSearchParams();
  const { state } = useLocation();
  const setValue = useCallback(
    (value: string | null) =>
      setParams(
        (current) => {
          const next = new URLSearchParams(current);
          if (value === null || value === "") next.delete(name);
          else next.set(name, value);
          return next;
        },
        { replace: true, state },
      ),
    [name, setParams, state],
  );
  return [params.get(name), setValue];
}

/** A directory's find text, kept in `?q=`. */
export function useFindParam(): [query: string, setQuery: (text: string) => void] {
  const [query, setValue] = useSearchParam("q");
  const setQuery = useCallback(
    (text: string) => setValue(text.trim() === "" ? null : text),
    [setValue],
  );
  return [query ?? "", setQuery];
}

/**
 * A filter parameter restricted to `allowed` values: anything else in the URL counts as
 * unset, so a stale or hand-edited address never filters by something unknown.
 */
export function useChoiceParam<Code extends string>(
  name: string,
  allowed: readonly Code[],
): [value: Code | null, setValue: (value: Code | null) => void] {
  const [raw, setRaw] = useSearchParam(name);
  const value = allowed.find((code) => code === raw) ?? null;
  return [value, setRaw];
}

/** A yes/no filter kept as `?name=1`. */
export function useFlagParam(name: string): [value: boolean, setValue: (value: boolean) => void] {
  const [raw, setRaw] = useSearchParam(name);
  const setValue = useCallback((on: boolean) => setRaw(on ? "1" : null), [setRaw]);
  return [raw === "1", setValue];
}

/**
 * "Clear filters" for several URL parameters at once, which one setter per parameter cannot
 * do (each starts from the address as rendered).
 */
export function useClearParams(names: readonly string[]): () => void {
  const [, setParams] = useSearchParams();
  const { state } = useLocation();
  return useCallback(
    () =>
      setParams(
        (current) => {
          const next = new URLSearchParams(current);
          for (const name of names) next.delete(name);
          return next;
        },
        { replace: true, state },
      ),
    [names, setParams, state],
  );
}
