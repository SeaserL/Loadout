// SPDX-License-Identifier: Apache-2.0
//
// Drag-to-reorder with pointer events (ExecPlan 022/023). Not HTML5 drag and drop: the Windows
// WebView hands native drag events to the host for file drops, so `dragover` / `drop` never
// reach the page.

import { useEffect, useState } from "react";

/** `list` with `moved` put where `target` is (after it when moving down). */
export function moveBefore(list: string[], moved: string, target: string): string[] {
  const from = list.indexOf(moved);
  const to = list.indexOf(target);
  const rest = list.filter((x) => x !== moved);
  const at = rest.indexOf(target) + (from < to ? 1 : 0);
  rest.splice(at, 0, moved);
  return rest;
}

export interface GripProps {
  onPointerDown: (e: React.PointerEvent) => void;
  onKeyDown: (e: React.KeyboardEvent) => void;
}

/**
 * Items carry `data-<itemAttr>="<id>"` inside a container with `data-<scopeAttr>="<scope>"`.
 * Dragging starts on a grip (`grip(id)`); dropping on another item calls `onReorder` with the
 * new order. Arrow keys on a focused grip move the item one place; Escape cancels a drag.
 */
export function usePointerReorder(opts: {
  scope: string;
  scopeAttr: string;
  itemAttr: string;
  ids: string[];
  onReorder: (ids: string[]) => void;
}) {
  const { scope, scopeAttr, itemAttr, ids, onReorder } = opts;
  const [dragged, setDragged] = useState<string>();
  const [over, setOver] = useState<string>();

  useEffect(() => {
    if (!dragged) return;
    const selector = `[data-${scopeAttr}="${CSS.escape(scope)}"] [data-${itemAttr}]`;
    const target = (x: number, y: number) =>
      document.elementFromPoint(x, y)?.closest<HTMLElement>(selector)?.getAttribute(`data-${itemAttr}`) ?? undefined;
    const move = (e: PointerEvent) => setOver(target(e.clientX, e.clientY));
    const up = (e: PointerEvent) => {
      const to = target(e.clientX, e.clientY);
      if (to && to !== dragged) onReorder(moveBefore(ids, dragged, to));
      setDragged(undefined);
      setOver(undefined);
    };
    const cancel = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setDragged(undefined);
        setOver(undefined);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("keydown", cancel);
    document.body.classList.add("reordering");
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", cancel);
      document.body.classList.remove("reordering");
    };
    // `ids` and `onReorder` are read when the drag ends.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dragged, scope, scopeAttr, itemAttr]);

  const grip = (id: string): GripProps | undefined =>
    ids.length > 1
      ? {
          onPointerDown: (e) => {
            if (e.button !== 0) return;
            e.preventDefault();
            e.stopPropagation();
            setDragged(id);
            setOver(id);
          },
          onKeyDown: (e) => {
            const i = ids.indexOf(id);
            const back = e.key === "ArrowUp" || e.key === "ArrowLeft";
            const fwd = e.key === "ArrowDown" || e.key === "ArrowRight";
            const neighbour = back ? ids[i - 1] : fwd ? ids[i + 1] : undefined;
            if (neighbour === undefined) return;
            e.preventDefault();
            e.stopPropagation();
            onReorder(moveBefore(ids, id, neighbour));
          },
        }
      : undefined;

  /** `dragging` / `drop-target` state classes for an item. */
  const stateClass = (id: string) =>
    dragged === id ? "dragging" : over === id && dragged && dragged !== id ? "drop-target" : "";

  return { grip, stateClass };
}
