/** A column for the fit: its smallest useful width in pixels, and when it gives way. */
export interface FitSpec<Id extends string> {
  id: Id;
  /** Pixels the column needs to read well; fixed-width columns give their width. */
  min: number;
  /**
   * Where the column leaves as the grid narrows: 1 leaves first. Without it the column stays
   * whatever the width.
   */
  drop?: number;
}

/**
 * The columns that fit in `width` pixels, in `specs` order. Columns leave strictly in `drop`
 * order: one is kept only while every column that leaves after it is kept too, so a narrow
 * window never shows a secondary column instead of a primary one. `null` (width not measured,
 * as where there is no layout) keeps them all.
 */
export function fitColumns<Id extends string>(
  specs: readonly FitSpec<Id>[],
  width: number | null,
): Id[] {
  if (width === null) return specs.map((spec) => spec.id);
  const kept = new Set<Id>(specs.filter((spec) => spec.drop === undefined).map((spec) => spec.id));
  let used = specs.reduce((sum, spec) => sum + (kept.has(spec.id) ? spec.min : 0), 0);
  const leaving = specs
    .filter((spec) => spec.drop !== undefined)
    .sort((a, b) => (b.drop ?? 0) - (a.drop ?? 0));
  for (const spec of leaving) {
    used += spec.min;
    if (used > width) break;
    kept.add(spec.id);
  }
  return specs.filter((spec) => kept.has(spec.id)).map((spec) => spec.id);
}
