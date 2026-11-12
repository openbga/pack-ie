import { useState } from "react";

interface Option {
  id: string;
  label: string;
}

const same = (a: readonly Option[], b: readonly Option[]) =>
  a.length === b.length &&
  a.every((option, index) => option.id === b[index]?.id && option.label === b[index]?.label);

/**
 * The same array while its ids and labels stay the same. The snapshot is replaced every second;
 * the columns and the filter lists built from it should not be.
 */
export function useStableOptions(options: readonly Option[]): readonly Option[] {
  const [stable, setStable] = useState(options);
  if (same(stable, options)) return stable;
  setStable(options);
  return options;
}
