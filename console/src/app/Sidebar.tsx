import { Ellipsis, PanelLeftClose, PanelLeftOpen } from "lucide-react";
import { Button, MenuTrigger, Tooltip, TooltipTrigger } from "react-aria-components";
import { ActionMenu } from "@/components/RowActions/ActionMenu";
import styles from "./Sidebar.module.css";
import { SideNav } from "./SideNav";
import { NAV_DEFAULT, NAV_MAX, NAV_RAIL, NAV_STEP, useSidebarLayout } from "./useSidebarLayout";

/** The navigation frame keeps its controls fixed while the work-area list scrolls. */
export function Sidebar() {
  const layout = useSidebarLayout();
  return (
    <aside
      aria-label="Navigation sidebar"
      className={styles.sidebar}
      data-collapsed={layout.collapsed || undefined}
      data-resizing={layout.dragging || undefined}
      // Runtime geometry from the resize control; all visual styling stays in the CSS module.
      style={{ width: layout.width }}
    >
      <SidebarControls layout={layout} />
      <SideNav collapsed={layout.collapsed} />
      {/* biome-ignore lint/a11y/useSemanticElements: A focusable ARIA window splitter has drag and keyboard behavior; it is not a static hr. */}
      <div
        role="separator"
        tabIndex={0}
        aria-label="Navigation width"
        aria-orientation="vertical"
        aria-controls="main-navigation"
        aria-valuemin={NAV_RAIL}
        aria-valuemax={NAV_MAX}
        aria-valuenow={layout.width}
        aria-valuetext={layout.collapsed ? "Collapsed" : `${layout.width} pixels`}
        className={styles.resize}
        {...layout.separatorProps}
      />
    </aside>
  );
}

function SidebarControls({ layout }: { layout: ReturnType<typeof useSidebarLayout> }) {
  const ToggleIcon = layout.collapsed ? PanelLeftOpen : PanelLeftClose;
  const toggleLabel = layout.collapsed ? "Expand navigation" : "Collapse navigation";
  return (
    <div className={styles.controls}>
      {!layout.collapsed && <span className={styles.title}>Navigation</span>}
      {!layout.collapsed && (
        <MenuTrigger>
          <Button aria-label="Navigation width options" className={styles.control}>
            <Ellipsis aria-hidden="true" />
          </Button>
          <ActionMenu
            items={[
              {
                id: "narrower",
                label: "Narrower navigation",
                onAction: () => layout.resize(layout.width - NAV_STEP),
              },
              {
                id: "wider",
                label: "Wider navigation",
                onAction: () => layout.resize(layout.width + NAV_STEP),
              },
              {
                id: "reset",
                label: "Reset navigation width",
                onAction: () => layout.resize(NAV_DEFAULT),
              },
            ]}
          />
        </MenuTrigger>
      )}
      <TooltipTrigger>
        <Button
          aria-label={toggleLabel}
          aria-expanded={!layout.collapsed}
          aria-controls="main-navigation"
          onPress={layout.toggle}
          className={styles.control}
        >
          <ToggleIcon aria-hidden="true" />
        </Button>
        <Tooltip placement="right" className={styles.tooltip}>
          {toggleLabel}
        </Tooltip>
      </TooltipTrigger>
    </div>
  );
}
