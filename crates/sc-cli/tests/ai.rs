//! `sc ai new` against a fake Claude API: it asks, tests with `sc test`,
//! sends the report back when the test fails, and saves what passes.
#![cfg(feature = "ai")]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::{Value, json};
use tempfile::TempDir;

/// The requests the fake API got: headers and JSON body.
type Seen = Arc<Mutex<Vec<(String, Value)>>>;

/// A server that answers each request with the next of `answers` (the
/// text Claude would write); returns its URL and the requests it got.
fn fake_api(answers: Vec<Value>) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let record = seen.clone();
    std::thread::spawn(move || {
        for (answer, stream) in answers.into_iter().zip(listener.incoming()) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let (mut headers, mut length) = (String::new(), 0);
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                headers.push_str(&line);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            record.lock().unwrap().push((headers, serde_json::from_slice(&body).unwrap()));
            let reply = json!({
                "id": "msg_1", "type": "message", "role": "assistant",
                "model": "claude-opus-5-5", "stop_reason": "end_turn",
                "content": [
                    { "type": "thinking", "thinking": "", "signature": "sig" },
                    { "type": "text", "text": answer.to_string() }
                ]
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
                 connection: close\r\n\r\n{reply}",
                reply.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (url, seen)
}

fn sc(dir: &TempDir, url: &str) -> Command {
    let mut command = Command::cargo_bin("sc").unwrap();
    command
        .current_dir(dir.path())
        .env("SC_CONFIG_DIR", dir.path().join("config"))
        .env("SC_DATA_DIR", dir.path().join("data"))
        .env("SC_AI_URL", url)
        .env("SC_TSC", "no-such-tsc")
        .env("ANTHROPIC_API_KEY", "test-key");
    command
}

#[test]
fn a_failing_script_is_fixed_then_saved() {
    let broken = json!({
        "summary": "Removes empty files.",
        "files": [{ "path": "remove-empty.ts", "content": "throw new Error('not yet');" }]
    });
    let fixed = json!({
        "summary": "Removes empty files.",
        "files": [{ "path": "remove-empty.ts", "content": r#"
            const folder = sc.context?.panel ?? sc.args[0];
            for (const entry of await sc.list(folder)) {
              if (entry.kind === "file" && entry.size === 0) await sc.remove(entry.path);
            }
            sc.print("done");
        "# }]
    });
    let (url, seen) = fake_api(vec![broken, fixed]);
    let dir = TempDir::new().unwrap();
    sc(&dir, &url).args(["ai", "new", "remove", "empty", "files"]).assert().success().stdout(
        contains("try 2 of 3")
            .and(contains("Removes empty files."))
            .and(contains("removed panel/empty.log"))
            .and(contains("(F9, Scripts)")),
    );
    assert!(dir.path().join("data/scripts/remove-empty.ts").exists());

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    let (headers, first) = &seen[0];
    assert!(headers.contains("x-api-key: test-key"), "{headers}");
    assert!(headers.contains("anthropic-version: 2026-10-21"), "{headers}");
    assert_eq!(first["model"], "claude-opus-5-5");
    assert_eq!(first["fallbacks"], "default");
    assert_eq!(first["output_config"]["format"]["type"], "json_schema");
    assert!(first["messages"][0]["content"].as_str().unwrap().contains("remove empty files"));
    assert!(first["system"].as_str().unwrap().contains("declare namespace sc"));
    // The second request carries Claude's first answer unchanged and the
    // failed test report.
    let messages = seen[1].1["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1]["content"][0]["signature"], "sig");
    assert!(messages[2]["content"].as_str().unwrap().contains("not yet"));

    // The same name again is refused unless --force.
    let again = json!({
        "summary": "s",
        "files": [{ "path": "remove-empty.ts", "content": "sc.print('x');" }]
    });
    let (url, _) = fake_api(vec![again]);
    sc(&dir, &url)
        .args(["ai", "new", "--tries", "1", "again"])
        .assert()
        .failure()
        .stderr(contains("--force"));
}

#[test]
fn without_a_key_it_says_how_to_add_one() {
    let dir = TempDir::new().unwrap();
    let mut command = sc(&dir, "http://127.0.0.1:9");
    command.env_remove("ANTHROPIC_API_KEY");
    command.args(["ai", "new", "x"]).assert().failure().stderr(contains("sc ai key"));
    sc(&dir, "http://127.0.0.1:9")
        .env_remove("ANTHROPIC_API_KEY")
        .args(["ai", "key"])
        .write_stdin("saved-key\n")
        .assert()
        .success();
    let saved = fs::read_to_string(dir.path().join("config/anthropic-api-key")).unwrap();
    assert_eq!(saved, "saved-key");
}

#[test]
fn plugins_are_installed_when_they_pass() {
    let plugin = json!({
        "summary": "Counts the selection.",
        "files": [
            { "path": "plugin.json", "content": r#"{ "name": "count-selected", "version": "1.0.0",
                "commands": [{ "id": "count", "title": "Count selected", "category": "Select" }],
                "permissions": { "read": ["$SELECTION"] } }"# },
            { "path": "main.ts", "content": "sc.print(`${sc.context!.selection.length} selected`);" }
        ]
    });
    let (url, _) = fake_api(vec![plugin]);
    let dir = TempDir::new().unwrap();
    sc(&dir, &url)
        .args(["ai", "new", "--plugin", "count", "the", "selection"])
        .assert()
        .success()
        .stdout(contains("5 selected").and(contains("installed plugin count-selected")));
    assert!(dir.path().join("data/plugins/count-selected/plugin.json").exists());
}

/// `sc ai rename` asks for a rule, checks it on the files, sends the error
/// back when it fails, and prints the rule and the new names.
#[test]
fn rename_rules_are_written_and_checked() {
    let rule = |template: &str| {
        json!({
            "explanation": "Puts the year first.", "template": template,
            "find": "^(.*)_(\\d{4})$", "replace": "${2}_$1", "regex": true, "case": "Lower",
            "counter_start": 1, "counter_step": 1, "counter_width": 1
        })
    };
    let (url, seen) = fake_api(vec![rule("[Q][X]"), rule("[N][X]")]);
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("Trip_2024.JPG"), "x").unwrap();
    fs::write(dir.path().join("Party_2023.JPG"), "x").unwrap();
    let output = sc(&dir, &url)
        .args(["ai", "rename", "--json", "year first", "Trip_2024.JPG", "Party_2023.JPG"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["names"], json!(["trip_2024.jpg", "party_2023.jpg"]));
    assert_eq!(report["rule"]["template"], "[N][X]");
    assert!(dir.path().join("Trip_2024.JPG").exists(), "nothing is renamed");

    let seen = seen.lock().unwrap();
    let first = &seen[0].1;
    assert!(first["messages"][0]["content"].as_str().unwrap().contains("Trip_2024.JPG"));
    assert_eq!(first["output_config"]["format"]["schema"]["required"][1], "template");
    let retry = seen[1].1["messages"][2]["content"].as_str().unwrap();
    assert!(retry.contains("fails"), "{retry}");
}
