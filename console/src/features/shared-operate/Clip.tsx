/**
 * Text in a grid cell that may not fit: the cell ends it in an ellipsis, and the whole text is
 * this element's title, so nothing is lost to a narrow column.
 */
export function Clip({ text }: { text: string }) {
  return <span title={text}>{text}</span>;
}
