import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from "react";
import { type ToastMessage, ToastRegion } from "./ToastRegion";

interface ToastApi {
  show: (toast: Omit<ToastMessage, "id">) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

/** Holds the transient notices and renders their live region. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const nextId = useRef(0);

  const dismiss = useCallback((id: number) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const show = useCallback((toast: Omit<ToastMessage, "id">) => {
    nextId.current += 1;
    const id = nextId.current;
    setToasts((current) => [...current, { ...toast, id }]);
  }, []);

  const api = useMemo(() => ({ show }), [show]);
  return (
    <ToastContext value={api}>
      {children}
      <ToastRegion toasts={toasts} onDismiss={dismiss} />
    </ToastContext>
  );
}

/** Shows a transient notice; the outcome of an async action is announced this way. */
export function useToast(): ToastApi {
  const api = useContext(ToastContext);
  if (api === null) {
    throw new Error("useToast needs a ToastProvider");
  }
  return api;
}
