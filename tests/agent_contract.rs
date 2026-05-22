use serde_json::Value;
use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn teams() -> Command {
    Command::new(env!("CARGO_BIN_EXE_teams"))
}

fn isolated_command() -> (Command, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cmd = teams();
    cmd.env("TEAMS_STATE_DIR", dir.path())
        .env("TEAMS_KEYRING_BACKEND", "file");
    (cmd, dir)
}

fn json_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout json")
}

fn json_stderr(output: &Output) -> Value {
    serde_json::from_slice(&output.stderr).expect("stderr json")
}

#[test]
fn parse_errors_are_json_on_stderr() {
    let output = teams().args(["--json", "read"]).output().expect("run");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let value = json_stderr(&output);
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "cli_parse_error");
}

#[test]
fn format_option_is_not_public() {
    let output = teams()
        .args([
            "--json",
            "send",
            "--dry-run",
            "--format",
            "markdown",
            "19:example-thread-id@thread.v2",
            "hello",
        ])
        .output()
        .expect("run");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let value = json_stderr(&output);
    assert_eq!(value["error"]["code"], "cli_parse_error");
}

#[test]
fn raw_resolve_does_not_require_login() {
    let (mut cmd, _dir) = isolated_command();
    let output = cmd
        .args(["--json", "resolve", "19:example-thread-id@thread.v2"])
        .output()
        .expect("run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value = json_stdout(&output);
    assert_eq!(value["ok"], true);
    assert_eq!(value["thread_id"], "19:example-thread-id@thread.v2");
}

#[test]
fn dry_run_does_not_echo_message_body() {
    let (mut cmd, _dir) = isolated_command();
    let output = cmd
        .args([
            "--json",
            "send",
            "--dry-run",
            "19:example-thread-id@thread.v2",
            "top secret body",
        ])
        .output()
        .expect("run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf8");
    assert!(!stdout.contains("top secret body"));
    let value = json_stdout(&output);
    assert_eq!(value["message"]["text_length"], 15);
}

#[test]
fn send_dry_run_reports_markdown_without_echoing_body() {
    let (mut cmd, _dir) = isolated_command();
    let output = cmd
        .args([
            "--json",
            "send",
            "--dry-run",
            "19:example-thread-id@thread.v2",
            "**top secret body**",
        ])
        .output()
        .expect("run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf8");
    assert!(!stdout.contains("top secret body"));
    let value = json_stdout(&output);
    assert_eq!(value["message"]["format"], "markdown");
    assert_eq!(value["message"]["markdown_converted"], true);
    assert_eq!(value["message"]["html_escaped"], true);
    assert!(value["message"]["html_length"].as_u64().unwrap() > 0);
}

#[test]
fn channel_post_dry_run_does_not_require_login() {
    let (mut cmd, _dir) = isolated_command();
    let output = cmd
        .args([
            "--json",
            "post",
            "channel",
            "--dry-run",
            "19:example-channel@thread.tacv2",
            "channel secret body",
        ])
        .output()
        .expect("run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf8");
    assert!(!stdout.contains("channel secret body"));
    let value = json_stdout(&output);
    assert_eq!(value["ok"], true);
    assert_eq!(value["posted"], false);
    assert_eq!(value["thread_id"], "19:example-channel@thread.tacv2");
    assert_eq!(value["message"]["text_length"], 19);
}

#[test]
fn channel_post_dry_run_can_resolve_without_message() {
    let (mut cmd, _dir) = isolated_command();
    let output = cmd
        .args([
            "--json",
            "post",
            "channel",
            "--dry-run",
            "19:example-channel@thread.tacv2",
        ])
        .output()
        .expect("run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value = json_stdout(&output);
    assert_eq!(value["ok"], true);
    assert_eq!(value["resolved"], true);
    assert_eq!(value["posted"], false);
    assert_eq!(
        value["confirm_thread_id"],
        "19:example-channel@thread.tacv2"
    );
    assert_eq!(value["thread_id"], "19:example-channel@thread.tacv2");
    assert!(value["message"].is_null());
}

#[test]
fn channel_post_dry_run_reports_markdown_without_echoing_body() {
    let (mut cmd, _dir) = isolated_command();
    let output = cmd
        .args([
            "--json",
            "post",
            "channel",
            "--dry-run",
            "19:example-channel@thread.tacv2",
            "**channel secret body**",
        ])
        .output()
        .expect("run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf8");
    assert!(!stdout.contains("channel secret body"));
    let value = json_stdout(&output);
    assert_eq!(value["message"]["format"], "markdown");
    assert_eq!(value["message"]["markdown_converted"], true);
    assert_eq!(value["message"]["html_escaped"], true);
}

#[test]
fn channel_post_card_dry_run_redacts_card_body() {
    let (mut cmd, dir) = isolated_command();
    let card_path = dir.path().join("card.json");
    fs::write(
        &card_path,
        r#"{
  "type": "AdaptiveCard",
  "version": "1.2",
  "body": [
    { "type": "TextBlock", "text": "card secret body" }
  ]
}"#,
    )
    .expect("card");

    let output = cmd
        .args(["--json", "post", "channel", "--dry-run", "--card-json"])
        .arg(&card_path)
        .arg("19:example-channel@thread.tacv2")
        .output()
        .expect("run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf8");
    assert!(!stdout.contains("card secret body"));
    let value = json_stdout(&output);
    assert_eq!(value["ok"], true);
    assert_eq!(
        value["card"]["content_type"],
        "application/vnd.microsoft.card.adaptive"
    );
    assert_eq!(value["card"]["version"], "1.2");
    assert_eq!(value["card"]["body_elements"], 1);
    assert!(value["message"].is_null());
}

#[test]
fn channel_post_card_actual_is_disabled_by_default() {
    let (mut cmd, dir) = isolated_command();
    let card_path = dir.path().join("card.json");
    fs::write(
        &card_path,
        r#"{
  "type": "AdaptiveCard",
  "version": "1.2",
  "body": []
}"#,
    )
    .expect("card");

    let output = cmd
        .args(["--json", "post", "channel", "--card-json"])
        .arg(&card_path)
        .arg("19:example-channel@thread.tacv2")
        .output()
        .expect("run");

    assert_eq!(output.status.code(), Some(2));
    let value = json_stderr(&output);
    assert_eq!(value["error"]["code"], "unsupported_card_post");
    assert_eq!(
        value["error"]["details"]["thread_id"],
        "19:example-channel@thread.tacv2"
    );
}

#[test]
fn channel_post_rejects_non_channel_raw_thread() {
    let (mut cmd, _dir) = isolated_command();
    let output = cmd
        .args([
            "--json",
            "post",
            "channel",
            "--dry-run",
            "19:example-chat@thread.v2",
            "hello",
        ])
        .output()
        .expect("run");

    assert_eq!(output.status.code(), Some(2));
    let value = json_stderr(&output);
    assert_eq!(value["error"]["code"], "invalid_channel_target");
}

#[test]
fn dry_run_enforces_confirm_thread_id() {
    let (mut cmd, _dir) = isolated_command();
    let output = cmd
        .args([
            "--json",
            "send",
            "--dry-run",
            "--confirm-thread-id",
            "19:other@thread.v2",
            "19:example-thread-id@thread.v2",
            "hello",
        ])
        .output()
        .expect("run");

    assert_eq!(output.status.code(), Some(2));
    let value = json_stderr(&output);
    assert_eq!(value["error"]["code"], "target_confirmation_mismatch");
}

#[test]
fn alias_commands_are_json_and_local() {
    let (mut set_cmd, dir) = isolated_command();
    let set_output = set_cmd
        .args([
            "--json",
            "alias",
            "set",
            "team",
            "19:example-thread-id@thread.v2",
        ])
        .output()
        .expect("run");
    assert!(set_output.status.success());
    let set_stdout = String::from_utf8(set_output.stdout.clone()).expect("utf8");
    assert!(!set_stdout.contains(dir.path().to_string_lossy().as_ref()));
    assert_eq!(json_stdout(&set_output)["ok"], true);

    let mut list_cmd = teams();
    let list_output = list_cmd
        .env("TEAMS_STATE_DIR", dir.path())
        .env("TEAMS_KEYRING_BACKEND", "file")
        .args(["--json", "alias", "list"])
        .output()
        .expect("run");
    assert!(list_output.status.success());
    let list_stdout = String::from_utf8(list_output.stdout.clone()).expect("utf8");
    assert!(!list_stdout.contains(dir.path().to_string_lossy().as_ref()));
    assert_eq!(
        json_stdout(&list_output)["aliases"]["team"],
        "19:example-thread-id@thread.v2"
    );
}

#[test]
fn malformed_alias_value_fails_closed() {
    let (mut cmd, dir) = isolated_command();
    fs::write(
        dir.path().join("aliases.toml"),
        "[aliases]\nbad = 'not-a-thread'\n",
    )
    .expect("aliases");

    let output = cmd
        .args(["--json", "resolve", "bad"])
        .output()
        .expect("run");

    assert_eq!(output.status.code(), Some(2));
    let value = json_stderr(&output);
    assert_eq!(value["error"]["code"], "alias_config_error");
    assert_eq!(value["error"]["details"]["reason"], "invalid_value");
    assert!(!String::from_utf8(output.stderr)
        .expect("utf8")
        .contains("not-a-thread"));
}

#[test]
fn cache_info_is_json_and_local() {
    let (mut cmd, _dir) = isolated_command();
    let output = cmd.args(["--json", "cache", "info"]).output().expect("run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf8");
    assert!(!stdout.contains(_dir.path().to_string_lossy().as_ref()));
    let value = json_stdout(&output);
    assert_eq!(value["ok"], true);
    assert_eq!(value["cache"]["exists"], false);
}

#[test]
fn cache_info_rejects_corrupt_cache() {
    let (mut cmd, dir) = isolated_command();
    let cache_dir = dir.path().join("cache");
    fs::create_dir_all(&cache_dir).expect("cache dir");
    fs::write(cache_dir.join("chats.json"), "{not json").expect("cache");

    let output = cmd.args(["--json", "cache", "info"]).output().expect("run");

    assert_eq!(output.status.code(), Some(2));
    let value = json_stderr(&output);
    assert_eq!(value["error"]["code"], "cache_corrupt");
}

#[test]
fn stdin_dry_run_reports_length_without_echoing_body() {
    let (mut cmd, _dir) = isolated_command();
    let mut child = cmd
        .args([
            "--json",
            "send",
            "--dry-run",
            "--stdin",
            "19:example-thread-id@thread.v2",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"secret from stdin")
        .expect("write");
    let output = child.wait_with_output().expect("output");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf8");
    assert!(!stdout.contains("secret from stdin"));
    let value = json_stdout(&output);
    assert_eq!(value["message"]["text_length"], 17);
}
