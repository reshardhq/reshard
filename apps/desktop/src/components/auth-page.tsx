import { LoginForm } from "@/components/login-form";
import { DitherBackdrop } from "@/components/dither-backdrop";

export function AuthPage() {
  return (
    <div className="relative flex h-svh w-full items-center justify-center overflow-hidden bg-background p-6">
      <div
        className="absolute inset-x-0 top-0 h-1/2"
        style={{
          maskImage: "linear-gradient(to bottom, black 60%, transparent)",
        }}
      >
        <DitherBackdrop variant="glow" pixel={3} opacity={0.12} />
      </div>
      {/* frameless window still needs a drag strip */}
      <div data-tauri-drag-region className="fixed inset-x-0 top-0 z-10 h-10" />
      <div className="relative w-full max-w-sm">
        <LoginForm />
      </div>
    </div>
  );
}
