# Agent Contract

This document describes the stable automation-facing behavior for `teams --json`.

## Streams

- Success output is JSON on stdout.
- Error output is JSON on stderr.
- Human login device-code prompts are written to stderr so `teams --json login` can keep final success JSON on stdout after authentication completes.
- Message bodies are not echoed by `send --dry-run --json`.

## Error Envelope

```json
{
  "ok": false,
  "error": {
    "code": "ambiguous_target",
    "message": "human-readable message",
    "exit_code": 2,
    "details": {}
  }
}
```

Common exit codes:

| Exit | Meaning |
| ---: | --- |
| 0 | Success |
| 1 | Unexpected local error |
| 2 | Invalid command, argument, target, alias, timestamp, or cache |
| 10 | Not logged in |
| 11 | Authentication, authorization, or Conditional Access failure |
| 20 | Teams endpoint not found |
| 30 | Rate limited after retries |
| 40 | Network, non-auth HTTP, or local IO failure |

Common error codes:

| Code | Meaning |
| --- | --- |
| `cli_parse_error` | Command-line parsing failed |
| `not_logged_in` | Run `teams login` first |
| `alias_config_error` | `aliases.toml` could not be read or parsed; resolution fails closed |
| `cache_corrupt` | `cache/chats.json` is invalid JSON |
| `target_not_found` | No matching existing chat was found |
| `ambiguous_target` | Multiple sendable chats matched |
| `unsupported_target` | Only channel/system entries matched |
| `self_chat_not_found` | Self notes could not be resolved |
| `target_confirmation_mismatch` | `--confirm-thread-id` did not match the resolved chat |
| `confirmation_required` | `send --json` needs `--confirm-thread-id` for name/title targets |
| `invalid_arguments` | Command arguments are incompatible or incomplete |
| `message_too_large` | Message body exceeded the CLI size cap |
| `invalid_alias` | Alias name failed validation |
| `invalid_thread_id` | Alias value was not a raw Teams thread id |
| `invalid_timestamp` | `--since` or `--before` is not RFC3339 |

## Success Envelopes

Most commands include `ok: true` plus command-specific fields. Examples below omit some nested metadata for brevity.

### Resolve

```json
{
  "ok": true,
  "resolved": true,
  "target": "user@example.com",
  "thread_id": "19:...",
  "source": "cached_chats",
  "chat": "19:...",
  "chat_summary": {
    "id": "19:...",
    "kind": "OneToOne",
    "title": "User Name",
    "members": []
  }
}
```

### Send Dry Run

```json
{
  "ok": true,
  "sent": false,
  "dry_run": true,
  "target": "user@example.com",
  "thread_id": "19:...",
  "message": {
    "content_type": "RichText/Html",
    "text_length": 17,
    "html_escaped": true
  }
}
```

### Send

```json
{
  "ok": true,
  "sent": true,
  "dry_run": false,
  "id": null,
  "client_message_id": "1780000000000",
  "thread_id": "19:...",
  "chat_summary": {}
}
```

`id` may be `null` even when Teams returned HTTP 201. Treat `sent: true` plus `client_message_id` as the success signal.

### Read

```json
{
  "ok": true,
  "read": true,
  "target": "user@example.com",
  "thread_id": "19:...",
  "count": 1,
  "limit": 20,
  "requested_limit": 20,
  "fetched_limit": 100,
  "since": "2026-05-21T00:00:00Z",
  "before": null,
  "messages": [
    {
      "id": "msg",
      "created_at": "2026-05-21T01:02:03Z",
      "sender_is_self": false,
      "content_text": "hello"
    }
  ]
}
```

`--since` is inclusive. `--before` is exclusive. Limits above 100 are clamped to 100. When a time filter is present, the CLI may fetch up to 100 recent messages, apply the filter, then truncate to `limit`.

### Search Chats

```json
{
  "ok": true,
  "query": "alex",
  "count": 1,
  "results": [
    {
      "score": 12,
      "matched": ["title:contains"],
      "chat": {
        "id": "19:...",
        "kind": "OneToOne",
        "title": "Alex",
        "member_count": 2
      }
    }
  ]
}
```

Search results intentionally omit `last_message_preview` and full member profiles. Use `resolve` or `read` after selecting a `thread_id`.

### Alias

```json
{
  "ok": true,
  "aliases_file": "aliases.toml",
  "aliases": {
    "support": "19:..."
  }
}
```

### Cache

```json
{
  "ok": true,
  "cache": {
    "file": "cache/chats.json",
    "exists": true,
    "schema_version": 2,
    "owner_present": true,
    "bytes": 12345,
    "modified": "2026-05-21T00:00:00Z",
    "chat_count": 20
  }
}
```

## Safe Send Flow

For autonomous agents, use this pattern:

```powershell
$resolved = teams --json resolve "user@example.com" | ConvertFrom-Json
teams --json send --dry-run --confirm-thread-id $resolved.thread_id "user@example.com" "draft text"
teams --json send --confirm-thread-id $resolved.thread_id "user@example.com" "final text"
```

For sensitive or multiline text, prefer stdin:

```powershell
Get-Content .\message.txt -Raw | teams --json send --stdin --confirm-thread-id $resolved.thread_id "user@example.com"
```

If resolution returns `ambiguous_target`, do not guess. Use a raw `thread_id` from the candidates or define an alias.

Attachment URLs in `read` output are redacted by default. Attachments expose `has_url: true` when Teams returned a URL-like field.
