import { useEffect, useRef } from "react";

/* ─────────────────────────────────────────────────────────
 * DITHER BACKDROP
 * Ordered-dither (Bayer 8x8) gradient rendered to canvas at
 * chunky pixel scale, then stretched — retro print texture.
 *
 * Variants:
 *   glow    — radial glow from the top center
 *   slope   — diagonal linear fade
 *   horizon — vertical fade rising from the composer
 *
 * `animate` breathes the field and shimmers dot color through
 * a subtle violet↔cyan drift. Honors prefers-reduced-motion.
 * ───────────────────────────────────────────────────────── */

const BAYER_8 = [
  [0, 32, 8, 40, 2, 34, 10, 42],
  [48, 16, 56, 24, 50, 18, 58, 26],
  [12, 44, 4, 36, 14, 46, 6, 38],
  [60, 28, 52, 20, 62, 30, 54, 22],
  [3, 35, 11, 43, 1, 33, 9, 41],
  [51, 19, 59, 27, 49, 17, 57, 25],
  [15, 47, 7, 39, 13, 45, 5, 37],
  [63, 31, 55, 23, 61, 29, 53, 21],
];

type Variant = "glow" | "slope" | "horizon";

function intensity(
  variant: Variant,
  x: number,
  y: number,
  w: number,
  h: number,
  t: number,
) {
  switch (variant) {
    case "glow": {
      const cx = w / 2 + Math.sin(t * 0.00021) * w * 0.06;
      const breathe = 1 + Math.sin(t * 0.00037) * 0.08;
      const dx = (x - cx) / ((w / 2) * breathe);
      const dy = y / (h * 0.9 * breathe);
      return Math.max(0, 1 - Math.hypot(dx, dy));
    }
    case "slope":
      return Math.max(0, 1 - (x / w + y / h) / 1.4 + Math.sin(t * 0.0003) * 0.03);
    case "horizon":
      return Math.max(0, (y / h - 0.45) * 1.6 + Math.sin(t * 0.0003) * 0.03);
  }
}

export function DitherBackdrop({
  variant = "glow",
  pixel = 3,
  opacity = 0.5,
  animate = true,
  className,
}: {
  variant?: Variant;
  /** rendered pixel size — bigger = chunkier */
  pixel?: number;
  opacity?: number;
  /** breathe + color shimmer */
  animate?: boolean;
  className?: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const parent = canvas.parentElement;
    if (!parent) return;

    const reducedMotion = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    const live = animate && !reducedMotion;

    let raf = 0;
    let last = 0;

    const render = (t: number) => {
      const w = Math.max(1, Math.floor(parent.clientWidth / pixel));
      const h = Math.max(1, Math.floor(parent.clientHeight / pixel));
      if (canvas.width !== w) canvas.width = w;
      if (canvas.height !== h) canvas.height = h;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      const img = ctx.createImageData(w, h);
      for (let y = 0; y < h; y++) {
        for (let x = 0; x < w; x++) {
          const v = intensity(variant, x, y, w, h, live ? t : 0);
          const threshold = (BAYER_8[y % 8][x % 8] + 0.5) / 64;
          const on = v > threshold;
          const i = (y * w + x) * 4;
          img.data[i] = 255;
          img.data[i + 1] = 255;
          img.data[i + 2] = 255;
          // crisp white dots, brighter toward the core
          img.data[i + 3] = on ? Math.round(120 + v * 135) : 0;
        }
      }
      ctx.putImageData(img, 0, 0);
    };

    const loop = (t: number) => {
      // ~24fps is plenty for a shimmer
      if (t - last > 42) {
        last = t;
        render(t);
      }
      raf = requestAnimationFrame(loop);
    };

    render(0);
    if (live) raf = requestAnimationFrame(loop);

    const observer = new ResizeObserver(() => render(last));
    observer.observe(parent);
    return () => {
      cancelAnimationFrame(raf);
      observer.disconnect();
    };
  }, [variant, pixel, animate]);

  return (
    <canvas
      ref={canvasRef}
      aria-hidden
      className={`pointer-events-none absolute inset-0 h-full w-full ${className ?? ""}`}
      style={{ opacity, imageRendering: "pixelated" }}
    />
  );
}
