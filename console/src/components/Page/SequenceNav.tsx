import { ChevronLeft, ChevronRight } from "lucide-react";
import type { MouseEvent, ReactNode } from "react";
import { type NavigateOptions, useHref, useNavigate } from "react-router";
import buttonStyles from "@/components/Button/Button.module.css";
import styles from "./Page.module.css";

interface SequenceLink {
  href: string;
  routerOptions?: NavigateOptions;
}

/** Previous and Next records of the list a record page was opened from. */
export interface SequenceLinks {
  /** `null` at the start of the list. */
  previous: SequenceLink | null;
  /** `null` at the end of the list. */
  next: SequenceLink | null;
}

/**
 * Previous and Next as links. At either end the link stays in place, disabled, so Next does
 * not move under the pointer while someone steps through a list, and keeps focus when it
 * reaches the last record (WCAG 2.4.3).
 */
export function SequenceNav({ sequence }: { sequence: SequenceLinks }) {
  return (
    <nav aria-label="Records in list order" className={styles.sequence}>
      <SequenceStep link={sequence.previous}>
        <ChevronLeft aria-hidden="true" />
        Previous
      </SequenceStep>
      <SequenceStep link={sequence.next}>
        Next
        <ChevronRight aria-hidden="true" />
      </SequenceStep>
    </nav>
  );
}

const modified = (event: MouseEvent) =>
  event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey;

/**
 * One step, always the same `a` element, so focus survives the step becoming disabled at
 * an end (react-aria's `Link` swaps to a `span` there). Disabled, it has no `href`, stays
 * focusable with `aria-disabled`, and ignores activation.
 */
function SequenceStep({ link, children }: { link: SequenceLink | null; children: ReactNode }) {
  const navigate = useNavigate();
  const href = useHref(link?.href ?? ".");
  function follow(event: MouseEvent<HTMLAnchorElement>) {
    if (link && modified(event)) return;
    event.preventDefault();
    if (link) void navigate(link.href, link.routerOptions);
  }
  return (
    <a
      href={link ? href : undefined}
      role={link ? undefined : "link"}
      aria-disabled={link ? undefined : true}
      tabIndex={0}
      onClick={follow}
      className={`${buttonStyles.button} ${styles.sequenceLink}`}
    >
      {children}
    </a>
  );
}
