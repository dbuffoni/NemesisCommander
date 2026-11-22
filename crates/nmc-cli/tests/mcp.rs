//! `nmc mcp` as a child process, the way an assistant starts it.
#![cfg(feature = "mcp")]

use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::TempDir;

fn run(dir: &TempDir, args: &[&str], requests: &[Value]) -> Vec<Value> {
    let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
    let output = Command::cargo_bin("nmc")
        .unwrap()
        .env("NMC_CONFIG_DIR", dir.path().join("config"))
        .env("NMC_DATA_DIR", dir.path().join("data"))
        .arg("mcp")
        .args(args)
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|_| panic!("not JSON on stdout: {l}")))
        .collect()
}

fn call(id: u64, tool: &str, arguments: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": tool, "arguments": arguments } })
}

#[test]
fn the_assistant_reads_what_was_granted_and_nothing_else() {
    let dir = TempDir::new().unwrap();
    let shared = dir.path().join("shared");
    let secret = dir.path().join("secret");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::create_dir_all(&secret).unwrap();
    std::fs::write(shared.join("hello.txt"), "hello from a file").unwrap();
    std::fs::write(secret.join("x.txt"), "no").unwrap();
    let log = dir.path().join("calls.log");

    let answers = run(
        &dir,
        &["--read", shared.to_str().unwrap(), "--log", log.to_str().unwrap()],
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            call(2, "read", json!({ "path": shared.join("hello.txt") })),
            call(3, "read", json!({ "path": secret.join("x.txt") })),
            call(4, "trash", json!({ "paths": [shared.join("hello.txt")] })),
        ],
    );
    // The notification gets no answer: four answers for five messages.
    assert_eq!(answers.len(), 4, "{answers:?}");
    assert_eq!(answers[0]["result"]["serverInfo"]["name"], "nmc");
    assert_eq!(answers[1]["result"]["structuredContent"]["content"], "hello from a file");
    assert_eq!(answers[2]["result"]["isError"], true);
    assert_eq!(answers[3]["result"]["isError"], true);
    assert!(shared.join("hello.txt").exists(), "read-only: nothing was trashed");
    let calls = std::fs::read_to_string(log).unwrap();
    assert_eq!(calls.lines().count(), 3, "{calls}");
    assert!(calls.contains("trash"));
}

#[test]
fn a_grants_file_adds_folders() {
    let dir = TempDir::new().unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let grants = dir.path().join("grants.json");
    std::fs::write(&grants, json!({ "write": [work] }).to_string()).unwrap();
    let answers = run(
        &dir,
        &["--grants", grants.to_str().unwrap()],
        &[call(1, "mkdir", json!({ "path": work.join("made") }))],
    );
    assert_eq!(answers[0]["result"]["isError"], false, "{answers:?}");
    assert!(work.join("made").is_dir());
}
