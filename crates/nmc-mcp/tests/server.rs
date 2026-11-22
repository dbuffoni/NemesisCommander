//! The MCP server over an in-process pipe: a fake client sends JSON-RPC
//! lines and reads the answers.

use std::fs;
use std::sync::Arc;

use nmc_mcp::{Grants, Server};
use nmc_vfs::VfsPath;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

struct Client {
    to_server: DuplexStream,
    from_server: tokio::io::Lines<BufReader<DuplexStream>>,
    next_id: u64,
}

impl Client {
    fn start(grants: Grants) -> Client {
        let (to_server, server_in) = tokio::io::duplex(1 << 20);
        let (server_out, from_server) = tokio::io::duplex(1 << 20);
        let server = Server::new(Arc::new(nmc_providers::default_registry()), grants);
        tokio::spawn(async move {
            server.serve(BufReader::new(server_in), server_out).await.unwrap();
        });
        Client { to_server, from_server: BufReader::new(from_server).lines(), next_id: 1 }
    }

    async fn send(&mut self, value: Value) {
        self.to_server.write_all(format!("{value}\n").as_bytes()).await.unwrap();
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await;
        let line = self.from_server.next_line().await.unwrap().expect("an answer");
        let answer: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(answer["id"], id);
        answer
    }

    /// Calls a tool; the result and whether it is an error.
    async fn tool(&mut self, name: &str, arguments: Value) -> (Value, bool) {
        let answer =
            self.request("tools/call", json!({ "name": name, "arguments": arguments })).await;
        let result = &answer["result"];
        (result["structuredContent"].clone(), result["isError"] == true)
    }

    async fn tool_error(&mut self, name: &str, arguments: Value) -> String {
        let answer =
            self.request("tools/call", json!({ "name": name, "arguments": arguments })).await;
        assert_eq!(answer["result"]["isError"], true, "{answer}");
        answer["result"]["content"][0]["text"].as_str().unwrap().to_owned()
    }
}

fn local(path: &std::path::Path) -> VfsPath {
    VfsPath::from_local(path).unwrap()
}

/// `read/` (granted for reading) and `work/` (granted for writing).
fn setup() -> (tempfile::TempDir, Client) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("read/sub")).unwrap();
    fs::create_dir_all(root.join("work")).unwrap();
    fs::create_dir_all(root.join("private")).unwrap();
    fs::write(root.join("read/a.txt"), "alpha needle\nbeta\n").unwrap();
    fs::write(root.join("read/.env"), "TOKEN=secret\n").unwrap();
    fs::write(root.join("read/sub/b.txt"), "bravo\n").unwrap();
    fs::write(root.join("private/p.txt"), "private\n").unwrap();
    let mut grants = Grants::none();
    grants.allow_read(local(&root.join("read")));
    grants.allow_write(local(&root.join("work")));
    (dir, Client::start(grants))
}

#[tokio::test]
async fn it_introduces_itself_and_lists_its_tools() {
    let (_dir, mut client) = setup();
    let answer = client
        .request("initialize", json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }))
        .await;
    assert_eq!(answer["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(answer["result"]["serverInfo"]["name"], "nmc");
    assert!(answer["result"]["instructions"].as_str().unwrap().contains("untrusted"));
    client.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;

    let answer = client.request("tools/list", json!({})).await;
    let names: Vec<&str> = answer["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for tool in
        ["list", "stat", "read", "search", "hash", "mkdir", "copy", "move", "rename", "trash"]
    {
        assert!(names.contains(&tool), "{tool} in {names:?}");
    }
    let answer = client.request("nonsense", json!({})).await;
    assert_eq!(answer["error"]["code"], -32601);
    assert_eq!(client.request("ping", json!({})).await["result"], json!({}));
}

#[tokio::test]
async fn reading_tools_work_inside_the_grant() {
    let (dir, mut client) = setup();
    let read = dir.path().canonicalize().unwrap().join("read");
    let (listing, error) = client.tool("list", json!({ "path": read })).await;
    assert!(!error);
    assert_eq!(listing["total"], 3);
    let (read_a, _) = client.tool("read", json!({ "path": read.join("a.txt") })).await;
    assert_eq!(read_a["content"], "alpha needle\nbeta\n");
    assert_eq!(read_a["truncated"], false);
    // A window of the file, with where to go on.
    let (part, _) =
        client.tool("read", json!({ "path": read.join("a.txt"), "offset": 6, "length": 6 })).await;
    assert_eq!(part["content"], "needle");
    assert_eq!(part["next_offset"], 12);
    let (hits, _) = client.tool("search", json!({ "root": read, "content": "needle" })).await;
    assert_eq!(hits["hits"][0]["line"], 1);
    let (hash, _) =
        client.tool("hash", json!({ "path": read.join("sub/b.txt"), "algorithm": "sha256" })).await;
    assert_eq!(hash["hash"].as_str().unwrap().len(), 64);
}

#[tokio::test]
async fn secrets_are_never_read() {
    let (dir, mut client) = setup();
    let read = dir.path().canonicalize().unwrap().join("read");
    let message = client.tool_error("read", json!({ "path": read.join(".env") })).await;
    assert!(message.contains("secrets"), "{message}");
    // Nor do searches show their lines.
    let (hits, _) = client.tool("search", json!({ "root": read, "content": "TOKEN" })).await;
    for hit in hits["hits"].as_array().unwrap() {
        assert!(hit.get("match").is_none(), "{hit}");
    }
}

#[tokio::test]
async fn nothing_outside_the_grants_is_reachable() {
    let (dir, mut client) = setup();
    let root = dir.path().canonicalize().unwrap();
    for path in [root.join("private/p.txt"), root.join("read/../private/p.txt")] {
        let message = client.tool_error("read", json!({ "path": path })).await;
        assert!(message.contains("not allowed"), "{message}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("private"), root.join("read/escape")).unwrap();
        let message =
            client.tool_error("read", json!({ "path": root.join("read/escape/p.txt") })).await;
        assert!(message.contains("not allowed"), "{message}");
        let message = client.tool_error("list", json!({ "path": root.join("read/escape") })).await;
        assert!(message.contains("not allowed"), "{message}");
    }
    // Read-only by default: no change in a folder granted for reading.
    for (tool, arguments) in [
        ("mkdir", json!({ "path": root.join("read/new") })),
        ("rename", json!({ "from": root.join("read/a.txt"), "to": root.join("read/z.txt") })),
        ("trash", json!({ "paths": [root.join("read/a.txt")] })),
        (
            "copy",
            json!({ "sources": [root.join("read/a.txt")], "destination": root.join("read/sub") }),
        ),
    ] {
        let message = client.tool_error(tool, arguments).await;
        assert!(message.contains("--write"), "{tool}: {message}");
    }
    assert!(root.join("read/a.txt").exists() && !root.join("read/new").exists());
}

#[tokio::test]
async fn changes_stay_in_the_write_folders_and_dry_run_changes_nothing() {
    let (dir, mut client) = setup();
    let root = dir.path().canonicalize().unwrap();
    let work = root.join("work");

    // Copying from a read-only folder into a write folder is allowed.
    let args =
        json!({ "sources": [root.join("read/a.txt")], "destination": work, "dry_run": true });
    let (dry, error) = client.tool("copy", args).await;
    assert!(!error);
    assert_eq!(dry["dry_run"], true);
    assert!(!work.join("a.txt").exists(), "a dry run copies nothing");
    let (done, _) = client
        .tool("copy", json!({ "sources": [root.join("read/a.txt")], "destination": work }))
        .await;
    assert_eq!(done["done"], 1);
    assert_eq!(fs::read_to_string(work.join("a.txt")).unwrap(), "alpha needle\nbeta\n");

    // An existing file is not replaced.
    fs::write(work.join("a.txt"), "mine").unwrap();
    let (again, _) = client
        .tool("copy", json!({ "sources": [root.join("read/a.txt")], "destination": work }))
        .await;
    assert_ne!(
        again["failed"].as_array().map_or(0, Vec::len)
            + again["skipped"].as_u64().unwrap() as usize,
        0,
        "{again}"
    );
    assert_eq!(fs::read_to_string(work.join("a.txt")).unwrap(), "mine");

    let (_, error) = client.tool("mkdir", json!({ "path": work.join("x/y") })).await;
    assert!(!error);
    assert!(work.join("x/y").is_dir());
    let (_, error) = client
        .tool("rename", json!({ "from": work.join("a.txt"), "to": work.join("renamed.txt") }))
        .await;
    assert!(!error);
    assert!(work.join("renamed.txt").exists() && !work.join("a.txt").exists());
    let message = client
        .tool_error(
            "rename",
            json!({ "from": work.join("renamed.txt"), "to": root.join("read/o.txt") }),
        )
        .await;
    assert!(message.contains("--write"), "{message}");
}

#[tokio::test]
async fn bad_requests_are_answered_not_fatal() {
    let (_dir, mut client) = setup();
    client.to_server.write_all(b"this is not json\n").await.unwrap();
    let line = client.from_server.next_line().await.unwrap().unwrap();
    let answer: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(answer["error"]["code"], -32700);
    let message = client.tool_error("read", json!({})).await;
    assert!(message.contains("missing argument"), "{message}");
    let message = client.tool_error("explode", json!({})).await;
    assert!(message.contains("unknown tool"), "{message}");
}
