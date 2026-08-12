# Reshard runtime discovery

Reshard uses the same `reshard-runtime` Rust crate from the CLI and the native
Tauri shell. Discovery searches the process PATH and the user's login-shell
PATH, resolves canonical executable paths, and runs independent version,
authentication, adapter, and capability probes with four-second timeouts and
bounded output capture.

```sh
reshard runtimes
reshard runtimes --json
reshard runtime doctor claude
reshard setup
reshard runtime enable codex --project /absolute/project --name codex-main
```

## Claude subscription adapter

Claude Code's subscription session is supported through a pinned ACP sidecar.
Install it into Reshard's private runtime directory with:

```sh
reshard runtime install claude-subscription
reshard runtime doctor claude
```

The installer provisions its own Python environment under
`~/.reshard/runtimes/claude-subscription/`; users do not need to install Python
or manage package versions. `reshard acp --provider claude` prefers this
sidecar automatically. Claude still must be authenticated separately with
`claude /login`.

Discovery never installs software. In particular, it does not execute `npx
-y`; a missing ACP adapter is reported with remediation instead.

## Custom ACP runtimes

Custom definitions live in `~/.reshard/runtimes.toml`. The desktop frontend
cannot submit a path to the Tauri probe command; the native backend reads this
trusted local file itself.

```toml
[[runtimes]]
id = "my-agent"
label = "My ACP Agent"
command = "/absolute/path/to/my-agent"
args = ["acp"]
versionArgs = ["--version"]

[runtimes.capabilities]
nativeAcp = true
adapterBacked = false
subscriptionCompatible = false
resumableSessions = true
enforceableToolApprovals = true
cancellation = true
modelSwitching = false
maximumParallelism = 1
executionLocus = "localProcess"
```

The command must be an absolute path or a bare executable name. Reshard probes
custom agents with an official ACP `initialize` exchange. Capability claims
are conservative by default; a custom runtime without a declared enforceable
approval boundary is shown as configuration-invalid under the default `ask`
policy.

## Setup storage and safety

Selected agents are written atomically to `~/.reshard/supervisor.toml` with the
permission policy fixed to `ask`. Phase 2 does not start the legacy gateway:
that per-message execution path cannot enforce an owner approval yet. The
profile becomes executable through the approval-aware supervisor introduced
in later phases.

Runtime inventory uploaded to the relay excludes local executable and project
paths. The server accepts only bounded readiness metadata from an authenticated
machine credential; human sessions and other machines cannot report on its
behalf.
