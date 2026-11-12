import { Menu, MenuItem, Popover } from "react-aria-components";
import styles from "./RowActions.module.css";

/** One action on a record, taken from the record's `allowedActions`. */
export interface ActionItem {
  /** Stable key, such as the engine's action name. */
  id: string;
  /** What the action does, in operator terms, such as "Pause channel". */
  label: string;
  /** Runs when the item is chosen; usually opens the action's dialog. */
  onAction: () => void;
  /** `danger` for destructive actions: red text, never a red fill. */
  variant?: "danger";
}

/**
 * The popover menu shared by `RowActions` and `MoreActions`, placed inside a `MenuTrigger`.
 * The first item takes focus when it opens, so the keyboard can act at once.
 */
export function ActionMenu({ items }: { items: readonly ActionItem[] }) {
  return (
    <Popover placement="bottom end" className={styles.popover}>
      <Menu items={items} autoFocus="first" className={styles.menu}>
        {(item) => (
          <MenuItem
            id={item.id}
            textValue={item.label}
            onAction={item.onAction}
            className={item.variant === "danger" ? `${styles.item} ${styles.danger}` : styles.item}
          >
            {item.label}
          </MenuItem>
        )}
      </Menu>
    </Popover>
  );
}
