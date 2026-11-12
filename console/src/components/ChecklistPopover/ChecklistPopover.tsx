import { ChevronDown } from "lucide-react";
import { Dialog, DialogTrigger, Popover } from "react-aria-components";
import { Button } from "@/components/Button";
import { CheckboxGroup } from "@/components/CheckboxGroup";
import styles from "./ChecklistPopover.module.css";

interface ChecklistPopoverProps {
  /** Names the control and its choices, such as "Channels". */
  label: string;
  /** What the button shows beside the label, such as "All" or "2 selected". */
  summary: string;
  options: readonly { id: string; label: string }[];
  /** The selected option ids. */
  value: readonly string[];
  onChange: (value: string[]) => void;
  /** The button of the "none selected" choice, such as "Show all channels". */
  resetLabel: string;
  /**
   * For a list whose reset is not "select none" (the columns return to the default set):
   * whether the choice differs from it, and what the reset button does.
   */
  reset?: { isNeeded: boolean; onReset: () => void };
}

/**
 * A button that opens a list of checkboxes, for filters and settings with several choices at
 * once (the channels of the Messages list, its optional columns). Changes apply as they are
 * made; the reset button clears the selection.
 */
export function ChecklistPopover({
  label,
  summary,
  options,
  value,
  onChange,
  resetLabel,
  reset,
}: ChecklistPopoverProps) {
  const canReset = reset === undefined ? value.length > 0 : reset.isNeeded;
  return (
    <DialogTrigger>
      <Button>
        {label}: {summary}
        <ChevronDown aria-hidden="true" className={styles.chevron} />
      </Button>
      <Popover placement="bottom start" className={styles.popover}>
        <Dialog aria-label={label} className={styles.panel}>
          <CheckboxGroup legend={label} options={options} value={value} onChange={onChange} />
          {canReset && (
            <div>
              <Button variant="quiet" onPress={() => (reset ? reset.onReset() : onChange([]))}>
                {resetLabel}
              </Button>
            </div>
          )}
        </Dialog>
      </Popover>
    </DialogTrigger>
  );
}
