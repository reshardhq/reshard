import { useState } from "react";
import { toast } from "sonner";
import { LoadingState } from "@/components/ai";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

export function LoginForm({ className, ...props }: React.ComponentProps<"div">) {
  const signIn = useChat((state) => state.signIn);
  const register = useChat((state) => state.register);
  const [mode, setMode] = useState<"login" | "register">("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [submitting, setSubmitting] = useState(false);

  async function submit() {
    if (submitting) return;
    setSubmitting(true);
    try {
      if (mode === "register") {
        // No name field — keep Create the same height as Sign in. Derive a
        // display name from the email's local part (server requires 1–80 chars).
        const derived = (email.split("@")[0] ?? "").trim().slice(0, 80) || "there";
        await register(derived, email, password);
      } else await signIn(email, password);
    } catch (error) {
      const message = error instanceof Error ? error.message : "Unable to continue.";
      toast.error(message.charAt(0).toUpperCase() + message.slice(1));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className={cn("flex flex-col gap-6", className)} {...props}>
      <Card className="overflow-hidden p-0 ring-0 shadow-none">
        <CardContent className="p-0">
          <form
            className="p-6 tracking-tight md:p-8"
            onSubmit={(event) => {
              event.preventDefault();
              void submit();
            }}
          >
            <FieldGroup>
              <div className="flex flex-col items-center gap-2 text-center">
                <h1 className="text-2xl font-bold">
                  {mode === "login" ? "Welcome back" : "Create your account"}
                </h1>
                <p className="text-balance text-muted-foreground">
                  {mode === "login"
                    ? "Sign in to continue to your agent chats"
                    : "Start a private home for your agents"}
                </p>
              </div>
              <Field>
                <FieldLabel htmlFor="email">Email</FieldLabel>
                <Input id="email" type="email" placeholder="you@example.com" value={email} onChange={(event) => setEmail(event.target.value)} autoComplete="email" required />
              </Field>
              <Field>
                <FieldLabel htmlFor="password">Password</FieldLabel>
                <Input id="password" type="password" placeholder="At least 8 characters" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete={mode === "login" ? "current-password" : "new-password"} minLength={8} required />
              </Field>
              <Field>
                <Button type="submit" disabled={submitting}>
                  {submitting ? <LoadingState label={mode === "login" ? "Signing in" : "Creating account"} variant="Dots" /> : mode === "login" ? "Sign in" : "Create account"}
                </Button>
              </Field>
              <Field>
                <Button variant="ghost" type="button" disabled={submitting} onClick={() => setMode(mode === "login" ? "register" : "login")}>
                  {mode === "login" ? "Need an account? Create one" : "Already have an account? Sign in"}
                </Button>
              </Field>
            </FieldGroup>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
