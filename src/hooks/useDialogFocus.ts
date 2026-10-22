import { useEffect, useRef } from "react";

const FOCUSABLE =
  'a[href],button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])';

// Attach the returned ref to the dialog root: focuses [data-autofocus] (or the
// first focusable) on mount, traps Tab inside, calls onEscape (if given) on
// Escape, and restores focus to the previously focused element on unmount.
export function useDialogFocus<T extends HTMLElement>(onEscape?: () => void) {
  const ref = useRef<T | null>(null);
  const escapeRef = useRef(onEscape);
  escapeRef.current = onEscape;
  // Captured during the first render, before anything inside the dialog can take focus.
  const openerRef = useRef<HTMLElement | null>(null);
  if (openerRef.current === null && typeof document !== "undefined") {
    openerRef.current = document.activeElement as HTMLElement | null;
  }

  useEffect(() => {
    const root = ref.current;
    if (!root) {
      return;
    }
    const previous = openerRef.current;
    const focusables = () => Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE));
    (root.querySelector<HTMLElement>("[data-autofocus]") ?? focusables()[0] ?? root).focus();

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape" && escapeRef.current) {
        event.stopPropagation();
        escapeRef.current();
      } else if (event.key === "Tab") {
        const items = focusables();
        if (items.length === 0) {
          event.preventDefault();
          return;
        }
        const first = items[0];
        const last = items[items.length - 1];
        const active = document.activeElement;
        if (event.shiftKey && (active === first || !root!.contains(active))) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && (active === last || !root!.contains(active))) {
          event.preventDefault();
          first.focus();
        }
      }
    }
    root.addEventListener("keydown", onKeyDown);
    return () => {
      root.removeEventListener("keydown", onKeyDown);
      // After unmount; skip if the opener is gone (e.g. virtualized tile) or the dialog is still mounted.
      requestAnimationFrame(() => {
        if (root.isConnected) return; // StrictMode re-run, dialog still open
        if (previous && previous.isConnected && previous !== document.body) previous.focus();
      });
    };
  }, []);

  return ref;
}
