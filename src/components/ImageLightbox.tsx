import { useEffect, useRef, useState, useCallback } from "react";
import { X, ChevronLeft, ChevronRight, ZoomIn, ZoomOut, RefreshCcw } from "lucide-react";

type ImageLightboxProps = {
  images: string[];
  startIndex?: number;
  onClose: () => void;
};

const ZOOM_STEP = 0.2;
const MIN_ZOOM = 0.25;
const MAX_ZOOM = 5;

export function ImageLightbox({ images, startIndex = 0, onClose }: ImageLightboxProps) {
  const [index, setIndex] = useState(startIndex);
  const [scale, setScale] = useState(1);
  const [translate, setTranslate] = useState({ x: 0, y: 0 });
  const [isPanning, setIsPanning] = useState(false);
  const lastPosRef = useRef<{ x: number; y: number } | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);

  const clampScale = (s: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, s));

  const resetView = () => {
    setScale(1);
    setTranslate({ x: 0, y: 0 });
  };

  const onWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    const delta = e.deltaY > 0 ? -ZOOM_STEP : ZOOM_STEP;
    setScale((prev) => clampScale(prev + delta));
  };

  const onMouseDown = (e: React.MouseEvent) => {
    e.preventDefault();
    setIsPanning(true);
    lastPosRef.current = { x: e.clientX, y: e.clientY };
  };

  const onMouseMove = (e: React.MouseEvent) => {
    if (!isPanning || !lastPosRef.current) return;
    const dx = e.clientX - lastPosRef.current.x;
    const dy = e.clientY - lastPosRef.current.y;
    lastPosRef.current = { x: e.clientX, y: e.clientY };
    setTranslate((prev) => ({ x: prev.x + dx, y: prev.y + dy }));
  };

  const onMouseUp = () => {
    setIsPanning(false);
    lastPosRef.current = null;
  };

  const goPrev = useCallback(() => {
    if (index > 0) {
      setIndex((i) => i - 1);
      resetView();
    }
  }, [index]);

  const goNext = useCallback(() => {
    if (index < images.length - 1) {
      setIndex((i) => i + 1);
      resetView();
    }
  }, [index, images.length]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if (e.key === "ArrowLeft") goPrev();
      if (e.key === "ArrowRight") goNext();
    };
    document.addEventListener("keydown", onKey);
    const body = document.body;
    const prevOverflow = body.style.overflow;
    body.style.overflow = "hidden";
    return () => {
      document.removeEventListener("keydown", onKey);
      body.style.overflow = prevOverflow;
    };
  }, [goPrev, goNext, onClose]);

  const zoomIn = () => setScale((s) => clampScale(s + ZOOM_STEP));
  const zoomOut = () => setScale((s) => clampScale(s - ZOOM_STEP));

  return (
    <div
      ref={containerRef}
      className="fixed inset-0 z-50 bg-black/90 text-white flex items-center justify-center select-none"
      onWheel={onWheel}
      onMouseMove={onMouseMove}
      onMouseUp={onMouseUp}
      onMouseLeave={onMouseUp}
      role="dialog"
      aria-modal="true"
    >
      {/* Close button */}
      <button
        aria-label="Close"
        className="absolute top-4 right-4 p-2 rounded bg-white/10 hover:bg-white/20"
        onClick={onClose}
      >
        <X className="w-6 h-6" />
      </button>

      {/* Prev/Next */}
      <button
        aria-label="Previous"
        disabled={index === 0}
        className="absolute left-4 top-1/2 -translate-y-1/2 p-3 rounded-full bg-white/10 hover:bg-white/20 disabled:opacity-40"
        onClick={goPrev}
      >
        <ChevronLeft className="w-7 h-7" />
      </button>
      <button
        aria-label="Next"
        disabled={index === images.length - 1}
        className="absolute right-4 top-1/2 -translate-y-1/2 p-3 rounded-full bg-white/10 hover:bg-white/20 disabled:opacity-40"
        onClick={goNext}
      >
        <ChevronRight className="w-7 h-7" />
      </button>

      {/* Toolbar */}
      <div className="absolute bottom-6 left-1/2 -translate-x-1/2 flex items-center gap-2 bg-white/10 rounded px-3 py-2">
        <span className="text-sm opacity-80 mr-2">{index + 1} / {images.length}</span>
        <button className="p-2 rounded hover:bg-white/20" onClick={zoomOut}>
          <ZoomOut className="w-5 h-5" />
        </button>
        <button className="p-2 rounded hover:bg-white/20" onClick={zoomIn}>
          <ZoomIn className="w-5 h-5" />
        </button>
        <button className="p-2 rounded hover:bg-white/20" onClick={resetView}>
          <RefreshCcw className="w-5 h-5" />
        </button>
      </div>

      {/* Image */}
      <div className="max-w-[95vw] max-h-[85vh] overflow-hidden cursor-grab active:cursor-grabbing" onMouseDown={onMouseDown}>
        <img
          src={images[index]}
          alt={`image-${index + 1}`}
          draggable={false}
          style={{ transform: `translate(${translate.x}px, ${translate.y}px) scale(${scale})` }}
          className="origin-center select-none"
          onError={(e) => { (e.target as HTMLImageElement).src = "/placeholder.svg"; }}
        />
      </div>
    </div>
  );
}

export default ImageLightbox;


