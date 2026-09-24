import { useEffect, useRef, useState } from "react";
import type { ElementDescriptor, Viewport } from "../api";
import { Button, Dialog } from "./ui";

interface Props {
  src: string;
  viewport: Viewport;
  caption: string;
  selected?: ElementDescriptor | null;
  mode?: "review" | "inspect" | "interact";
  busy?: boolean;
  onPoint?: (x: number, y: number) => void;
}

/** Shared viewer for live frames and saved captures. Pixels never submit themselves to an agent. */
export function ScreenshotViewer(props: Props) {
  const [expanded, setExpanded] = useState(false);
  return <>
    <ImageSurface {...props} expand={() => setExpanded(true)} />
    {expanded && <Dialog title="Screenshot viewer" variant="viewer" onClose={() => setExpanded(false)}>
      <ImageSurface {...props} mode="review" selected={null} fullscreen />
    </Dialog>}
  </>;
}

function ImageSurface({ src, viewport, caption, selected, mode = "review", busy, onPoint, expand, fullscreen }: Props & { expand?: () => void; fullscreen?: boolean }) {
  const stage = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; y: number; left: number; top: number } | null>(null);
  const [space, setSpace] = useState({ width: 600, height: 420 });
  const [natural, setNatural] = useState(viewport);
  const [zoom, setZoom] = useState<number | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    const element = stage.current!;
    const observer = new ResizeObserver(() => setSpace({ width: element.clientWidth, height: element.clientHeight }));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  useEffect(() => { setFailed(false); }, [src]);
  const fit = Math.min((space.width - 24) / natural.width, (space.height - 24) / natural.height, 1);
  const scale = zoom ?? Math.max(0.05, fit);
  const changeZoom = (by: number) => setZoom(Math.max(0.1, Math.min(3, scale + by)));
  return <section aria-label="Screenshot review" className="min-w-0 rounded-lg border border-line bg-panel">
    <div className="flex flex-wrap items-center gap-2 border-b border-line p-2">
      <Button aria-pressed={zoom === null} onClick={() => setZoom(null)}>Fit</Button>
      <Button aria-pressed={zoom === 1} onClick={() => setZoom(1)}>100%</Button>
      <Button aria-label="Zoom out" onClick={() => changeZoom(-0.25)}>−</Button>
      <span className="w-12 text-center text-[12px] tabular-nums">{Math.round(scale * 100)}%</span>
      <Button aria-label="Zoom in" onClick={() => changeZoom(0.25)}>+</Button>
      {expand && <Button onClick={expand}>Fullscreen</Button>}
      <span className="ml-auto text-[12px] text-dim">{viewport.width} × {viewport.height}</span>
    </div>
    <div ref={stage} tabIndex={0} aria-label="Image canvas; use arrow keys or scrollbars to pan" className={`overflow-auto bg-bg p-3 ${fullscreen ? "h-[72dvh]" : "h-[min(55dvh,34rem)] min-h-48"}`}
      onPointerDown={e => {
        if (mode !== "review" || e.button !== 0) return;
        drag.current = { x: e.clientX, y: e.clientY, left: e.currentTarget.scrollLeft, top: e.currentTarget.scrollTop };
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove={e => { const d = drag.current; if (d) { e.currentTarget.scrollLeft = d.left - e.clientX + d.x; e.currentTarget.scrollTop = d.top - e.clientY + d.y; } }}
      onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }}>
      {failed ? <p role="alert" className="text-bad">The image could not load. Refresh the preview or reopen the capture.</p> : <div className="relative mx-auto shrink-0" style={{ width: natural.width * scale, height: natural.height * scale }}>
        <img src={src} alt={caption} draggable={false} onError={() => setFailed(true)} onLoad={e => setNatural({ width: e.currentTarget.naturalWidth, height: e.currentTarget.naturalHeight })} className="block h-full w-full max-w-none select-none" />
        {mode !== "review" && <button className={`absolute inset-0 ${mode === "inspect" ? "cursor-crosshair" : "cursor-pointer"}`} disabled={busy} aria-label={mode === "inspect" ? "Inspect screenshot element; use the selector controls for keyboard inspection" : "Click website; use selector controls for keyboard interaction"}
          onClick={e => { if (e.detail === 0) { document.getElementById("preview-selector")?.focus(); return; } const r = e.currentTarget.getBoundingClientRect(); onPoint?.((e.clientX-r.left)*viewport.width/r.width, (e.clientY-r.top)*viewport.height/r.height); }} />}
        {selected && mode === "inspect" && <div className="pointer-events-none absolute border-2 border-accent bg-accent/10" style={{ left:`${selected.box.x/viewport.width*100}%`, top:`${selected.box.y/viewport.height*100}%`, width:`${selected.box.width/viewport.width*100}%`, height:`${selected.box.height/viewport.height*100}%` }} />}
      </div>}
    </div>
    <p className="break-words border-t border-line px-3 py-2 text-[12px] text-dim">{caption}{mode === "review" ? " · Drag or use scrollbars to pan." : ""}</p>
  </section>;
}
