import { useId } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { Button } from "@/components/Button";
import { useIdle } from "@/lib/idle";
import styles from "./IdleOverlay.module.css";
import { restoreFocusAfterIdle } from "./restoreFocusAfterIdle";

/**
 * Covers the whole console once the idle time has passed, until Continue. It is a modal dialog
 * of its own, so everything beneath it (also a dialog that was open) is inert and out of the
 * accessibility tree, focus stays on Continue, and Escape and outside clicks do nothing.
 * Message content was already dropped by the screens that held it (`useOnIdle`); the cover
 * hides what is left, such as the list.
 */
export function IdleOverlay() {
  const { idle, resume, minutes } = useIdle();
  const messageId = useId();
  return (
    <ModalOverlay
      isOpen={idle}
      isDismissable={false}
      isKeyboardDismissDisabled
      className={styles.overlay}
    >
      <Modal className={styles.modal}>
        <Dialog role="alertdialog" aria-describedby={messageId} className={styles.dialog}>
          <Heading slot="title" level={2} className={styles.title}>
            Content hidden
          </Heading>
          <p id={messageId}>
            Content hidden after {minutes} {minutes === 1 ? "minute" : "minutes"} without activity.
            Choose Continue to carry on.
          </p>
          <div className={styles.actions}>
            <Button
              variant="primary"
              autoFocus
              onPress={() => {
                resume();
                restoreFocusAfterIdle();
              }}
            >
              Continue
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
