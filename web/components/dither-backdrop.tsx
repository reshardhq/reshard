"use client";

import { useEffect, useRef } from "react";
import { cn } from "@/lib/utils";

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

export function DitherBackdrop({ className }: { className?: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const parent = canvas?.parentElement;
    if (!canvas || !parent) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let frame = 0;
    let last = 0;

    const render = (time: number) => {
      const pixel = 3;
      const width = Math.max(1, Math.floor(parent.clientWidth / pixel));
      const height = Math.max(1, Math.floor(parent.clientHeight / pixel));
      if (canvas.width !== width) canvas.width = width;
      if (canvas.height !== height) canvas.height = height;
      const context = canvas.getContext("2d");
      if (!context) return;
      const image = context.createImageData(width, height);
      const drift = reduced ? 0 : Math.sin(time * 0.00024) * width * 0.035;
      const breathe = reduced ? 1 : 1 + Math.sin(time * 0.00031) * 0.07;

      for (let y = 0; y < height; y += 1) {
        for (let x = 0; x < width; x += 1) {
          const dx = (x - width * 0.72 - drift) / (width * 0.55 * breathe);
          const dy = (y - height * 0.12) / (height * 0.84 * breathe);
          const radial = Math.max(0, 1 - Math.hypot(dx, dy));
          const beam = Math.max(0, 1 - Math.abs((x / width) - 0.64 - (y / height) * 0.18) * 3.2);
          const value = Math.min(1, radial * 0.74 + beam * 0.24);
          const threshold = (BAYER_8[y % 8][x % 8] + 0.5) / 64;
          if (value <= threshold) continue;
          const index = (y * width + x) * 4;
          image.data[index] = 111;
          image.data[index + 1] = 143;
          image.data[index + 2] = 255;
          image.data[index + 3] = Math.round(60 + value * 155);
        }
      }
      context.putImageData(image, 0, 0);
    };

    const loop = (time: number) => {
      if (time - last > 45) {
        last = time;
        render(time);
      }
      frame = requestAnimationFrame(loop);
    };

    render(0);
    if (!reduced) frame = requestAnimationFrame(loop);
    const observer = new ResizeObserver(() => render(last));
    observer.observe(parent);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, []);

  return <canvas ref={canvasRef} aria-hidden className={cn("pointer-events-none absolute inset-0 size-full opacity-35", className)} style={{ imageRendering: "pixelated" }} />;
}
