const art = String.raw`
                         ╭──────────────╮
                 ╭───────┤ CLAUDE   ●   │
                 │       ╰──────────────╯
  YOU  ◉━━━━━━━━━╋━━━━━━━━━━  REBEAM  ━━━━━━━━━━◉  CODEX
                 │
                 ╰───────┤ HERMES   ●   │
                         ╰──────────────╯
`;

export function AsciiSignal() {
  return (
    <div className="ascii-signal pointer-events-none absolute bottom-8 right-6 hidden select-none lg:block" aria-hidden>
      <pre className="font-mono text-xs leading-[1.35] tracking-[-0.04em] text-[#8ba4ff]/50">{art}</pre>
    </div>
  );
}
