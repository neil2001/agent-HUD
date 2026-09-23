import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

type Position = { top: number; left: number; place: "below" | "above" };

export function MetricTooltip({
  help,
  children,
}: {
  help?: string;
  children: ReactNode;
}) {
  const id = useId();
  const triggerRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<Position | null>(null);

  const close = useCallback(() => setOpen(false), []);
  const show = useCallback(() => {
    if (help) setOpen(true);
  }, [help]);

  const updatePosition = useCallback(() => {
    const trigger = triggerRef.current;
    const panel = panelRef.current;
    if (!trigger || !panel) return;

    const rect = trigger.getBoundingClientRect();
    const tooltip = panel.getBoundingClientRect();
    const gap = 8;
    const pad = 12;
    const spaceBelow = window.innerHeight - rect.bottom;
    const place: Position["place"] =
      spaceBelow < tooltip.height + gap + pad && rect.top > tooltip.height + gap
        ? "above"
        : "below";

    let top =
      place === "below" ? rect.bottom + gap : rect.top - tooltip.height - gap;
    let left = rect.left;
    left = Math.min(left, window.innerWidth - tooltip.width - pad);
    left = Math.max(pad, left);
    top = Math.max(pad, Math.min(top, window.innerHeight - tooltip.height - pad));

    setPos({ top, left, place });
  }, []);

  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    updatePosition();
  }, [open, help, updatePosition]);

  useEffect(() => {
    if (!open) return;
    const onScroll = () => updatePosition();
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onScroll);
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onScroll);
    };
  }, [open, updatePosition]);

  if (!help) {
    return <>{children}</>;
  }

  return (
    <>
      <div
        ref={triggerRef}
        className="flow-help-trigger"
        tabIndex={0}
        aria-describedby={open ? id : undefined}
        onMouseEnter={show}
        onMouseLeave={close}
        onFocus={show}
        onBlur={close}
      >
        {children}
      </div>
      {open &&
        createPortal(
          <div
            ref={panelRef}
            id={id}
            role="tooltip"
            className={`flow-tooltip flow-tooltip-${pos?.place ?? "below"}`}
            style={
              pos
                ? { top: pos.top, left: pos.left, visibility: "visible" }
                : { visibility: "hidden" }
            }
          >
            {help}
          </div>,
          document.body,
        )}
    </>
  );
}
