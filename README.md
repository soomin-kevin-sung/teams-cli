# teams-cli

Unofficial Microsoft Teams CLI for personal automation. It uses undocumented Teams web APIs (`chatsvc`, `csa`, `authsvc`) and may break or violate tenant policy. Use at your own risk and keep request volume low.

## Features

- `teams login` — OAuth2 device-code login, then Teams Skype-token exchange.
- `teams logout` — removes local tokens and state. No server-side revocation is performed.
- `teams whoami` — prints cached identity and token expiry information.
- `teams list-chats [-n N] [--json]` — lists recent group chats using Teams web APIs.
- `teams send <chat-id-or-alias> <message>` — sends plaintext as HTML to an existing 1:1 or group chat thread.

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
teams send "19:example-thread-id@thread.v2" "hello from CLI"
teams logout
```

`list-chats --json` includes `members` as structured user metadata when Teams exposes it. For 1:1 chats, the CLI resolves both the signed-in user and the peer with `mri`, `object_id`, `display_name`, and `user_principal_name` when available.

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

## Known Limitations

- Microsoft Teams unofficial APIs are unsupported and can change without notice.
- Work/school accounts only; personal/MSA accounts are out of scope.
- Device-code flow may be blocked by Conditional Access.
- Channel posts, file uploads, reactions, and creating new 1:1 threads are not implemented.
- Group and channel roster expansion is not implemented; `members` is currently most complete for 1:1 chats.
- `send` accepts raw `19:...`/`48:...` thread IDs or aliases only; UPN/display-name lookup is planned for a later version.
- Logout deletes local state only; already-issued tokens expire naturally.
