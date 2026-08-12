# Tool approvals

Reshard tool approvals are a security protocol, not chat messages. Generic
`Ask` messages remain ordinary questions and cannot release a local tool.

## Trust boundary

The provider and the exact tool input stay on the requesting machine. The
machine computes a canonical SHA-256 digest and sends the relay only that
digest plus a bounded, credential-scrubbed display projection. The relay
derives the owner from the authenticated machine and agent; callers cannot
choose an approver.

Actionable approval events are delivered only to:

- the human owner of the agent;
- the exact machine that opened the request.

Other room members receive only redacted “waiting for owner approval” status.
They cannot list, inspect, or resolve the approval.

## State machine

```text
pending -> allowed
        -> denied
        -> expired
        -> cancelled
```

Every transition is a transactional compare-and-set and appends an audit row.
Terminal requests cannot transition again. A stale card, duplicate response,
wrong owner, wrong machine, timeout, cancellation, or digest mismatch fails
closed. A digest mismatch turns the request into `denied` rather than leaving
an ambiguous pending request.

`Allow once` releases only the matching `(approval, run, tool call, digest)`
through a machine-local one-shot gate. Raw provider input is never accepted
back from the relay.

## Provider behavior

Native ACP `session/request_permission` calls use this broker in gateway mode.
Terminal mode asks locally and treats Enter as denial. Blanket approval exists
only behind the explicit unsafe `reshard acp --allow-all` flag.

Claude's old `--dangerously-skip-permissions` default has been removed. The
private `--permission-prompt-tool` MCP bridge is now installed per run with a
0600 config and inherited only by the MCP child. Unsupported Claude versions
remain disabled under the runtime capability gate; Reshard never falls back to
bypass mode.
