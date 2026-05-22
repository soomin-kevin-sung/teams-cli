# teams-cli

Unofficial Microsoft Teams CLI for personal automation. It uses undocumented Teams web APIs (`chatsvc`, `csa`, `authsvc`) and may break or violate tenant policy. Use at your own risk and keep request volume low.

## Features

- `teams login` - OAuth2 device-code login, then Teams Skype-token exchange.
- `teams logout` - removes local tokens, state, and cached chat metadata. No server-side revocation is performed.
- `teams whoami` - prints cached identity and token expiry information.
- `teams list-chats [-n N] [--include-preview] [--json]` - lists recent chats and refreshes the local chat cache. JSON output redacts last-message previews unless `--include-preview` is set.
- `teams search-chats <query> [-n N] [--json]` - searches cached or recent chats by title, member metadata, email, or thread id.
- `teams resolve <target> [--json]` - resolves a send/read target without sending a message.
- `teams read <target> [-n N] [--since RFC3339] [--before RFC3339] [--json]` - reads recent messages from an existing chat.
- `teams send [--dry-run] [--stdin] [--format text|html|markdown] [--confirm-thread-id ID] <target> [message] [--json]` - sends rich text to an existing 1:1, group, or self notes chat.
- `teams post channel [--dry-run] [--stdin] [--format text|html|markdown] [--confirm-thread-id ID] <channel> [message] [--json]` - posts rich text to a channel root thread.
- `teams alias <list|set|remove>` - manages local aliases for stable thread ids.
- `teams cache <info|refresh|clear>` - manages local chat metadata used by target resolution.

Targets can be a raw thread id, alias, `me`/`self`/`notes`, exact email, exact display name, or exact chat title.

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
teams list-chats -n 20 --json
teams search-chats "alex" --json
teams resolve user@example.com --json
teams read user@example.com -n 20 --since 2026-05-21T00:00:00Z --json
teams send --dry-run user@example.com "hello from CLI" --json
teams send --dry-run --format markdown user@example.com "**hello** from CLI" --json
teams send --dry-run --confirm-thread-id "19:example@thread.v2" "19:example@thread.v2" "hello" --json
"hello from stdin" | teams send --stdin --confirm-thread-id "19:example@thread.v2" "19:example@thread.v2" --json
teams post channel --dry-run "19:example@thread.tacv2" --json
teams post channel --dry-run "Announcements" --json
teams post channel --dry-run --format html "Announcements" "<strong>hello</strong>" --json
teams post channel --dry-run --card-json .\card.json "Announcements" --json
teams alias set support "19:example-thread-id@thread.v2"
teams send support "hello from alias"
teams cache info --json
teams logout
```

`list-chats --json` includes `members` as structured user metadata when Teams exposes it. For 1:1 chats, the CLI resolves both the signed-in user and the peer with `mri`, `object_id`, `display_name`, and `user_principal_name` when available.

When Teams exposes the self notes conversation, `send`, `read`, and `resolve` can use `me`, `self`, `myself`, `notes`, `self notes`, `saved messages`, or `chat with self`. The most common self notes thread id is `48:notes`.

Some Teams send responses return `201 Created` without a server message id. The CLI treats that as success and returns the generated `client_message_id`; this is not a warning or failure.

## Agent-Friendly Interface

Use `--json` for machine-readable success output and structured errors, including command-line parse errors. Success JSON is written to stdout. Error JSON is written to stderr and includes a stable `error.code`, human-readable `error.message`, numeric `error.exit_code`, and command-specific `error.details`. HTTP response bodies from Teams are not printed in errors.

```powershell
teams --json resolve user@example.com
teams --json send --dry-run user@example.com "hello from agent"
teams --json send --dry-run --confirm-thread-id "19:example@thread.v2" "19:example@thread.v2" "hello"
teams --json post channel --dry-run "Announcements"
teams --json read user@example.com -n 20 --before 2026-05-22T00:00:00Z
```

Recommended agent flow:

1. Run `teams --json search-chats <name-or-email>` or `teams --json resolve <target>`.
2. If resolution is ambiguous, choose from `error.details.candidates` or create an alias with `teams alias set`.
3. Run `teams --json send --dry-run --confirm-thread-id <thread_id> <target> <message>` before sending.
4. Send with the same `--confirm-thread-id` so a cache refresh or name collision cannot redirect the message.

`send --dry-run --json` and `post channel --dry-run --json` report message length, selected format, and rendered HTML length without echoing the full message text. `--stdin` lets agents pass longer or sensitive message bodies through stdin instead of command-line arguments.
Use `--format text` for escaped plain text, `--format markdown` for safe Markdown-to-HTML conversion, and `--format html` only when the caller intentionally supplies Teams-compatible HTML.
For actual `send --json` calls, name/title targets require `--confirm-thread-id`; raw thread ids and aliases can be sent without that extra confirmation.
For actual `post channel --json` calls, resolved channel title targets also require `--confirm-thread-id`; raw channel thread ids and aliases can be posted without that extra confirmation.
Use `post channel --dry-run --json <channel-name>` to resolve a channel title to its raw `19:...@thread.tacv2` id before posting. The message argument is optional for channel dry-runs.
Use `--card-json FILE` only for Adaptive Card dry-run validation. Actual card posting over the undocumented chat-service endpoint is disabled by default because Teams accepts app cards as `RichText/Media_Card` SWIFT messages, and the current user client is not allowed to send that message type.

Raw thread ids and aliases can be resolved without a logged-in session. Cached name/title lookup requires login so the cache can be matched to the current tenant and user. Invalid `aliases.toml` fails closed with `error.code = "alias_config_error"` instead of falling back to live name matching.

See [docs/agent-contract.md](docs/agent-contract.md) for the JSON contract and exit codes.

## Storage

- Secret tokens are stored in the OS keychain by default (`keyring` crate; Windows Credential Manager on Windows).
- Non-secret state is stored under the app config directory as `state.toml`.
- Chat metadata cache is stored under `cache/chats.json` and is bound to the tenant/user that refreshed it.
- Aliases are stored in `aliases.toml`.
- `TEAMS_STATE_DIR` overrides the state directory for tests/dev.
- `TEAMS_KEYRING_BACKEND=file` stores base64-encoded secrets in `secrets.json` under `TEAMS_STATE_DIR`. This is intended only for tests/headless development; prefer OS keychain for real accounts.

## Aliases

```powershell
teams alias set util "19:example-thread-id@thread.v2"
teams alias list --json
teams send util "hello"
teams alias remove util
```

Aliases are local only. Alias names may contain ASCII letters, numbers, `.`, `-`, and `_`.

## Known Limitations

- Microsoft Teams unofficial APIs are unsupported and can change without notice.
- Work/school accounts only; personal/MSA accounts are out of scope.
- Device-code flow may be blocked by Conditional Access.
- Channel posts support a raw `19:...@thread.tacv2` id, an alias, or an exact channel title from Teams chat-service metadata. File uploads, reactions, and creating new 1:1 threads are not implemented.
- Group and channel roster expansion is best-effort; `members` is currently most complete for 1:1 chats.
- `send` can resolve only existing chats returned by `list-chats` or `cache refresh`; it does not create a new 1:1 conversation for an email that has no existing chat.
- `read` uses undocumented Teams message endpoints and normalizes common response shapes; some rich cards, reactions, and specialized attachments may be simplified.
- `read -n` values above 100 are clamped to 100. With time filters, the CLI fetches up to 100 recent messages before filtering.
- Logout deletes local state only; already-issued tokens expire naturally.
