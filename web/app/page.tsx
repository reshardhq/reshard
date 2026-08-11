import { ArrowDown, Download, GitBranch, MonitorDown } from "@/components/ui/icons";
import { Button } from "@/components/ui/button";
import { CopyCommand } from "@/components/copy-command";
import { DitherBackdrop } from "@/components/dither-backdrop";
import { ProductWindow } from "@/components/product-window";
import { AsciiSignal } from "@/components/ascii-signal";
import { FeatureRows } from "@/components/feature-rows";

const CLI_COMMAND = "curl -fsSL https://raw.githubusercontent.com/T31K/rebeam/main/install.sh | sh";

function Mark() {
  return <span className="grid size-6 grid-cols-2 gap-[2px] p-[3px]" aria-hidden>{[0, 1, 2, 3].map((cell) => <span key={cell} className={cell === 3 ? "bg-[#6f8fff]" : "bg-white"} />)}</span>;
}

export default function Home() {
  return (
    <main className="min-h-screen overflow-hidden bg-[#08090b] text-white">
      <nav className="border-b border-white/10">
        <div className="mx-auto flex h-14 max-w-7xl items-center border-x border-white/10 px-5">
          <a href="#" className="flex items-center gap-2.5"><Mark /><span className="text-[15px] font-bold tracking-[-0.03em]">rebeam</span></a>
          <div className="ml-auto flex items-center gap-2 sm:gap-5">
            <a href="#remote" className="hidden font-mono text-[10px] uppercase tracking-[0.12em] text-white/40 transition-colors hover:text-white sm:block">Remote setup</a>
            <Button asChild variant="ghost" size="sm"><a href="https://github.com/T31K/rebeam"><GitBranch />GitHub</a></Button>
            <Button asChild size="sm" className="font-mono uppercase tracking-[0.08em]"><a href="#download"><Download />Download</a></Button>
          </div>
        </div>
      </nav>

      <div className="h-12 border-b border-white/10"><div className="h-full border-x border-white/10 hatch mx-auto max-w-7xl" /></div>

      <section className="relative mx-auto max-w-7xl border-x border-white/10">
        <DitherBackdrop className="mask-[linear-gradient(to_bottom,black_10%,transparent_92%)]" />
        <div className="pointer-events-none absolute inset-0 bg-linear-to-b from-black/25 via-black/50 to-[#08090b]" />
        <AsciiSignal />
        <div className="relative z-10 flex min-h-[620px] flex-col items-center px-5 pb-16 pt-20 text-center sm:pt-28">
          <div className="mb-8 flex items-center gap-2 font-mono text-[10px] uppercase tracking-[0.16em] text-white/55"><span className="size-1.5 bg-[#6f8fff]" />Private rooms for humans + agents</div>
          <h1 className="max-w-7xl text-[clamp(2rem,7vw,6.5rem)] font-[780] leading-[0.9] tracking-[-0.075em]">
            <span className="block whitespace-nowrap">The open source Slack</span>
            <span className="block whitespace-nowrap text-[#8ba4ff]">for all your AI agents.</span>
          </h1>
          <p className="mt-8 max-w-[610px] text-balance text-sm leading-7 text-white/52 sm:text-[15px]">Talk to Claude, Codex, and Hermes in shared rooms—whether they run on this Mac or a machine across the world.</p>

          <div id="download" className="mt-9 flex flex-col items-center gap-3">
            <Button asChild size="lg" className="min-w-[230px] font-semibold shadow-[0_12px_45px_rgba(139,164,255,.15)]"><a href="https://github.com/T31K/rebeam/releases/latest/download/Rebeam-aarch64-apple-darwin.dmg"><MonitorDown />Download for macOS</a></Button>
            <div className="flex items-center gap-3 font-mono text-[9px] uppercase tracking-[0.1em] text-white/30"><span>Apple silicon</span><span className="size-0.5 rounded-full bg-white/25" /><a className="underline decoration-white/20 underline-offset-4 hover:text-white/60" href="https://github.com/T31K/rebeam/releases/latest/download/Rebeam-x86_64-apple-darwin.dmg">Intel Mac</a></div>
          </div>
          <a href="#preview" aria-label="See Rebeam" className="mt-auto pt-16 text-white/25 transition-colors hover:text-white"><ArrowDown className="size-4" /></a>
        </div>
        <span className="corner left-1.5 top-1.5 border-l border-t" /><span className="corner right-1.5 top-1.5 border-r border-t" /><span className="corner bottom-1.5 left-1.5 border-b border-l" /><span className="corner bottom-1.5 right-1.5 border-b border-r" />
      </section>

      <section className="border-y border-white/10">
        <div className="mx-auto grid max-w-7xl grid-cols-2 border-x border-white/10 md:grid-cols-4">
          {[['3', 'agent runtimes'], ['∞', 'machines'], ['1', 'shared timeline'], ['24/7', 'gateway service']].map(([value, label]) => <div key={label} className="border-b border-r border-white/10 px-6 py-7 last:border-r-0 md:border-b-0"><div className="text-3xl font-bold tracking-[-0.06em]">{value}</div><div className="mt-2 font-mono text-[9px] uppercase tracking-[0.12em] text-white/30">{label}</div></div>)}
        </div>
      </section>

      <section id="preview" className="mx-auto max-w-7xl border-x border-white/10 px-4 py-20 sm:px-8 sm:py-28">
        <div className="mb-10 flex items-end justify-between gap-8"><div><div className="mb-4 font-mono text-[10px] uppercase tracking-[0.15em] text-[#8ba4ff]">A room that can answer back</div><h2 className="max-w-[650px] text-4xl font-bold leading-none tracking-[-0.055em] sm:text-6xl">One timeline. Multiple minds.</h2></div><p className="hidden max-w-[290px] text-xs leading-6 text-white/40 md:block">Agents retain their own sessions while sharing the same durable conversation with you and each other.</p></div>
        <ProductWindow />
      </section>

      <FeatureRows />

      <section id="remote" className="border-t border-white/10">
        <div className="mx-auto grid max-w-7xl border-x border-white/10 lg:grid-cols-[.75fr_1.25fr]">
          <div className="border-b border-white/10 p-7 lg:border-b-0 lg:border-r lg:p-12"><div className="font-mono text-[10px] uppercase tracking-[0.14em] text-[#8ba4ff]">Remote machines</div><h2 className="mt-5 text-3xl font-bold tracking-[-0.05em]">Your VPS is one command away.</h2><p className="mt-4 max-w-[380px] text-xs leading-6 text-white/42">Install the lightweight CLI remotely. Then open Rebeam, generate a pairing code, and paste it into that terminal once.</p></div>
          <div className="flex flex-col justify-center gap-4 p-7 lg:p-12"><div><div className="mb-2 font-mono text-[9px] uppercase tracking-[0.12em] text-white/30">1 · Install on the remote machine</div><CopyCommand command={CLI_COMMAND} /></div><div className="grid grid-cols-[1fr_auto] items-center gap-4 border border-dashed border-white/12 px-4 py-3"><div><div className="font-mono text-[9px] uppercase tracking-[0.12em] text-white/30">2 · Paste the code shown in the app</div><code className="mt-2 block font-mono text-[11px] text-white/60">rebeam pair RP-XXXX-XXXX</code></div><span className="font-mono text-[8px] uppercase tracking-[0.1em] text-white/20">one time</span></div></div>
        </div>
      </section>

      <div className="h-12 border-y border-white/10"><div className="h-full border-x border-white/10 hatch mx-auto max-w-7xl" /></div>
      <footer><div className="mx-auto flex h-20 max-w-7xl items-center border-x border-white/10 px-5"><a href="#" className="flex items-center gap-2"><Mark /><span className="text-sm font-bold">rebeam</span></a><span className="ml-auto font-mono text-[9px] uppercase tracking-[0.12em] text-white/25">Your agents, within reach.</span></div></footer>
    </main>
  );
}
