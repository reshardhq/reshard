import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkBreaks from "remark-breaks";
import type { Member } from "@agentchat/shared";
import { cn } from "@/lib/utils";
import { store, useChat } from "@/lib/use-chat";

/**
 * Agents speak markdown — code, lists, tables — so messages render as
 * markdown, not plain text. `remark-breaks` keeps single newlines meaningful
 * the way every chat app does.
 *
 * @mentions are rewritten into links with a private scheme before parsing, so
 * they survive markdown and get styled like the composer's autocomplete.
 */
export function MessageText({
  text,
  members,
  className,
}: {
  text: string;
  members: Member[];
  className?: string;
}) {
  const source = linkMentions(text, members);

  return (
    <div
      className={cn(
        // No prose measure: agents answer with tables and code as often as
        // sentences, and a 72ch cap wraps those into unreadable columns. The
        // sidebars already bound the line length.
        "text-sm leading-relaxed text-foreground/90",
        "[&>*+*]:mt-2 [&_li]:my-0.5",
        className,
      )}
    >
      <Markdown
        remarkPlugins={[remarkGfm, remarkBreaks]}
        components={{
          a: ({ href, children }) =>
            href?.startsWith(MENTION_SCHEME) ? (
              <span
                className={cn(
                  "rounded px-1 py-0.5 font-medium",
                  // Being addressed is the one thing worth breaking the
                  // greyscale for — everything else stays a quiet chip.
                  href.slice(MENTION_SCHEME.length) === currentUserName()
                    ? "bg-amber-400/15 text-amber-300 ring-1 ring-inset ring-amber-400/25"
                    : "bg-foreground/10 text-foreground",
                )}
              >
                {children}
              </span>
            ) : (
              <a
                href={href}
                target="_blank"
                rel="noreferrer"
                className="text-primary underline decoration-primary/40 underline-offset-2 hover:decoration-primary"
              >
                {children}
              </a>
            ),

          code: ({ className: lang, children, ...props }) => {
            const isBlock = Boolean(lang);
            return isBlock ? (
              <code
                className="block font-mono text-[12px] leading-relaxed"
                {...props}
              >
                {children}
              </code>
            ) : (
              <code
                className="rounded border border-border bg-muted/50 px-1 py-px font-mono text-[12px]"
                {...props}
              >
                {children}
              </code>
            );
          },

          pre: ({ children }) => (
            <pre className="overflow-x-auto rounded-lg border bg-black/25 p-2.5">
              {children}
            </pre>
          ),

          ul: ({ children }) => (
            <ul className="list-disc space-y-0.5 pl-4.5">{children}</ul>
          ),
          ol: ({ children }) => (
            <ol className="list-decimal space-y-0.5 pl-4.5">{children}</ol>
          ),

          h1: ({ children }) => (
            <p className="text-sm font-semibold text-foreground">{children}</p>
          ),
          h2: ({ children }) => (
            <p className="text-sm font-semibold text-foreground">{children}</p>
          ),
          h3: ({ children }) => (
            <p className="text-sm font-semibold text-foreground">{children}</p>
          ),

          blockquote: ({ children }) => (
            <blockquote className="border-l-2 border-border pl-2.5 text-muted-foreground">
              {children}
            </blockquote>
          ),

          hr: () => <hr className="border-border" />,

          table: ({ children }) => (
            <div className="overflow-x-auto">
              <table className="w-full border-collapse text-[13px]">
                {children}
              </table>
            </div>
          ),
          th: ({ children }) => (
            <th className="border-b border-border px-2 py-1 text-left font-medium">
              {children}
            </th>
          ),
          td: ({ children }) => (
            <td className="border-b border-border/50 px-2 py-1">{children}</td>
          ),
        }}
      >
        {source}
      </Markdown>
    </div>
  );
}

const MENTION_SCHEME = "rebeam:mention/";

/** The name that means "you", for deciding which mentions light up. */
function currentUserName(): string {
  const { members } = useChat.getState();
  return members.find((m) => m.id === store.currentUserId)?.name ?? "";
}

function linkMentions(text: string, members: Member[]): string {
  if (!members.length) return text;
  const names = members
    .map((m) => m.name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join("|");
  return text.replace(
    new RegExp(`@(${names})\\b`, "g"),
    (match, name) => `[${match}](${MENTION_SCHEME}${name})`,
  );
}
