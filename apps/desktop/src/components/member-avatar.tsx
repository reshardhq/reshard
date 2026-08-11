import type { Member } from "@agentchat/shared";
import { DitherAvatar } from "@/components/dither-kit/avatar";
import { fnv1a, hueFill, xorshift32 } from "@/components/dither-kit/pixel";
import { type Rgb } from "@/components/dither-kit/palette";
import { cn } from "@/lib/utils";

/**
 * Mirror DitherAvatar's deterministic hue draw (32 pattern bits +
 * mirror axis are consumed first) so the frame matches the pixels.
 */
export function avatarFill(name: string): Rgb {
  const rand = xorshift32(fnv1a(name));
  for (let i = 0; i < 33; i++) rand();
  return hueFill(Math.floor(rand() * 180) * 2);
}

export function AvatarFrame({
  name,
  className,
}: {
  name: string;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-block size-7 shrink-0 overflow-hidden rounded-none border-[0.5px] border-foreground/25 bg-foreground/[0.06] p-px",
        className,
      )}
      style={{ filter: "grayscale(1) brightness(1.15)" }}
    >
      <DitherAvatar name={name} className="size-full" />
    </span>
  );
}

export function MemberAvatar({
  member,
  className,
}: {
  member: Member;
  className?: string;
}) {
  return <AvatarFrame name={member.name} className={className} />;
}

export function PresenceDot({ member }: { member: Member }) {
  return (
    <span
      className={cn(
        "size-1.5 shrink-0 rounded-full",
        member.presence === "online"
          ? "bg-emerald-400"
          : "border border-muted-foreground/40 bg-transparent",
      )}
    />
  );
}
