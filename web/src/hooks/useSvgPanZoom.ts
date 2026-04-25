import { useCallback, useMemo, useState } from "react";
import type { PointerEvent as ReactPointerEvent, WheelEvent as ReactWheelEvent } from "react";

type ViewportState = {
  scale: number;
  x: number;
  y: number;
};

type UseSvgPanZoomOptions = {
  minScale?: number;
  maxScale?: number;
  initialScale?: number;
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function useSvgPanZoom(options: UseSvgPanZoomOptions = {}) {
  const minScale = options.minScale ?? 0.45;
  const maxScale = options.maxScale ?? 2.8;
  const initialScale = options.initialScale ?? 1;
  const [viewport, setViewport] = useState<ViewportState>({
    scale: initialScale,
    x: 0,
    y: 0,
  });
  const [dragging, setDragging] = useState<{
    pointerId: number;
    startClientX: number;
    startClientY: number;
    originX: number;
    originY: number;
  } | null>(null);

  const zoomTo = useCallback(
    (nextScale: number) => {
      setViewport((current) => ({
        ...current,
        scale: clamp(nextScale, minScale, maxScale),
      }));
    },
    [maxScale, minScale],
  );

  const zoomBy = useCallback(
    (delta: number) => {
      setViewport((current) => ({
        ...current,
        scale: clamp(current.scale + delta, minScale, maxScale),
      }));
    },
    [maxScale, minScale],
  );

  const reset = useCallback(() => {
    setViewport({
      scale: initialScale,
      x: 0,
      y: 0,
    });
    setDragging(null);
  }, [initialScale]);

  const onWheel = useCallback(
    (event: ReactWheelEvent<SVGSVGElement>) => {
      event.preventDefault();

      const rect = event.currentTarget.getBoundingClientRect();
      const pointerX = event.clientX - rect.left;
      const pointerY = event.clientY - rect.top;
      const zoomDelta = event.deltaY < 0 ? 1.12 : 0.9;

      setViewport((current) => {
        const nextScale = clamp(current.scale * zoomDelta, minScale, maxScale);
        if (nextScale === current.scale) {
          return current;
        }

        const worldX = (pointerX - current.x) / current.scale;
        const worldY = (pointerY - current.y) / current.scale;

        return {
          scale: nextScale,
          x: pointerX - worldX * nextScale,
          y: pointerY - worldY * nextScale,
        };
      });
    },
    [maxScale, minScale],
  );

  const onPointerDown = useCallback((event: ReactPointerEvent<SVGSVGElement>) => {
    const target = event.target;
    if (
      target instanceof Element &&
      target.closest(".graph-cloud__node, .dependency-map__node")
    ) {
      return;
    }

    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragging({
      pointerId: event.pointerId,
      startClientX: event.clientX,
      startClientY: event.clientY,
      originX: viewport.x,
      originY: viewport.y,
    });
  }, [viewport.x, viewport.y]);

  const onPointerMove = useCallback((event: ReactPointerEvent<SVGSVGElement>) => {
    setDragging((current) => {
      if (!current || current.pointerId !== event.pointerId) {
        return current;
      }

      setViewport((viewportState) => ({
        ...viewportState,
        x: current.originX + (event.clientX - current.startClientX),
        y: current.originY + (event.clientY - current.startClientY),
      }));

      return current;
    });
  }, []);

  const releasePointer = useCallback((event: ReactPointerEvent<SVGSVGElement>) => {
    if (dragging?.pointerId === event.pointerId) {
      try {
        event.currentTarget.releasePointerCapture(event.pointerId);
      } catch {
        // Ignore capture release errors when the browser already released it.
      }
      setDragging(null);
    }
  }, [dragging?.pointerId]);

  const transform = useMemo(
    () => `translate(${viewport.x} ${viewport.y}) scale(${viewport.scale})`,
    [viewport.scale, viewport.x, viewport.y],
  );

  return {
    dragging: dragging !== null,
    transform,
    scale: viewport.scale,
    zoomBy,
    zoomTo,
    reset,
    svgHandlers: {
      onWheel,
      onPointerDown,
      onPointerMove,
      onPointerUp: releasePointer,
      onPointerLeave: releasePointer,
      onPointerCancel: releasePointer,
    },
  };
}
