# teams-cli

Unofficial Microsoft Teams CLI for personal automation. It uses undocumented Teams web APIs (`chatsvc`, `csa`, `authsvc`) and may break or violate tenant policy. Use at your own risk and keep request volume low.

## Features

- `teams login` — OAuth2 device-code login, then Teams Skype-token exchange.
- `teams logout` — removes local tokens and state. No server-side revocation is performed.
- `teams whoami` — prints cached identity and token expiry information.
- `teams list-chats [-n N] [--json]` — lists recent group chats using Teams web APIs.
- `teams resolve <target> [--json]` — resolves a send target without sending a message.
- `teams send <target> <message>` — sends plaintext as HTML to an existing 1:1, group, or self notes chat. The target can be a raw thread id, alias, `me`/`self`/`notes`, exact email, exact display name, or exact chat title.

## Build

```powershell
cd C:\Users\est\teams-cli
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo install --path .
```

The binary name is `teams`.

## Usage

```powershell
teams login
teams whoami
teams list-chats -n 20
teams list-chats -n 20 --json
teams resolve user@example.com --json
teams send --dry-run user@example.com "hello from CLI" --json
teams send "19:example-thread-id@thread.v2" "hello from CLI"
teams send user@example.com "hello from CLI"
teams send "Project room" "hello from CLI"
teams send me "note to myself"
teams logout
```

`list-chats --json` includes `members` as structured user metadata when Teams exposes it. For 1:1 chats, the CLI resolves both the signed-in user and the peer with `mri`, `object_id`, `display_name`, and `user_principal_name` when available.
When Teams exposes the self notes conversation, `send` resolves `me`, `self`, `myself`, `notes`, `self notes`, `saved messages`, or `chat with self` to the `48:notes` thread.
Some Teams send responses return `201 Created` without a server message id; in that case the CLI still treats the send as successful and prints the generated `client_message_id`.

## Agent-Friendly Interface

Use `--json` for machine-readable success output and structured errors. Success JSON is written to stdout. Error JSON is written to stderr and includes a stable `error.code`, human-readable `error.message`, numeric `error.exit_code`, and command-specific `error.details`.

```powershell
teams --json resolve user@example.com
teams --json send --dry-run user@example.com "hello from agent"
teams --json send user@example.com "hello from agent"
```

`resolve` and `send --dry-run` use the same target resolution as `send`, so an agent can verify the exact `thread_id` before sending. Ambiguous targets fail with `error.code = "ambiguous_target"` and include up to 10 candidate chats in `error.details.candidates`.

`teams login --tenant <tenant-id-or-domain>` overrides the default `organizations` tenant. If your tenant blocks device-code flow, login returns a Conditional Access error; browser-cookie/MSAL extraction fallback is not implemented in this MVP.

## Storage

- Secret tokens are stored in the OS keychain by default (`keyring` crate; Windows Credential Manager on Windows).
- Non-secret state is stored under the app config directory as `state.toml`.
- `TEAMS_STATE_DIR` overrides the state directory for tests/dev.
- `TEAMS_KEYRING_BACKEND=file` stores base64-encoded secrets in `secrets.json` under `TEAMS_STATE_DIR`. This is intended only for tests/headless development; prefer OS keychain for real accounts.

## Aliases

Create `aliases.toml` next to `state.toml`:

```toml
[aliases]
util = "19:example-thread-id@thread.v2"
```

Then send with:

```powershell
teams send util "hello"
```

`send` resolves non-id targets from the cached `list-chats` result first, then refreshes the latest 100 chats if needed. If a display name or title matches multiple chats, the CLI prints candidates and refuses to send until you use a raw thread id or alias.

## Known Limitations

- Microsoft Teams unofficial APIs are unsupported and can change without notice.
- Work/school accounts only; personal/MSA accounts are out of scope.
- Device-code flow may be blocked by Conditional Access.
- Channel posts, file uploads, reactions, and creating new 1:1 threads are not implemented.
- Group and channel roster expansion is not implemented; `members` is currently most complete for 1:1 chats.
- `send` can resolve only existing chats returned by `list-chats`; it does not create a new 1:1 conversation for an email that has no existing chat. Self notes require Teams to expose the `48:notes` thread.
- Logout deletes local state only; already-issued tokens expire naturally.
