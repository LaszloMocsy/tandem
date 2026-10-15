use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation, ProtocolVersion,
};
use rmcp::service::{ClientLifecycleMode, ClientServiceExt, RoleClient, RunningService};
use rmcp::transport::StreamableHttpClientTransport;
use serde_json::{Value, json};
use tandem_board_core::service::{Actor, Config, NewTask, Service};
use tandem_board_core::store::{Status, TaskId};
use tandem_board_mcp::{McpServer, ServeError, endpoint_path};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

type Client = RunningService<RoleClient, ClientConfig>;

/// Which protocol a test client speaks.
#[derive(Debug, Clone, Copy)]
enum Protocol {
    /// 2026-07-28: no sessions, client info on every request.
    Latest,
    /// 2025-11-25: an `initialize` handshake and a session.
    Legacy,
}

const PROTOCOLS: [Protocol; 2] = [Protocol::Latest, Protocol::Legacy];

/// A project, its service and a running MCP server on a free port.
struct Env {
    _data: TempDir,
    _repo: TempDir,
    service: Arc<Service>,
    addr: SocketAddr,
    cancel: CancellationToken,
}

impl Env {
    async fn new() -> Self {
        let data = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        let service =
            Arc::new(Service::open(data.path(), "demo", repo.path(), Config::default()).unwrap());
        let projects = BTreeMap::from([("demo".to_owned(), Arc::clone(&service))]);
        let cancel = CancellationToken::new();
        let server = McpServer::bind("127.0.0.1:0".parse().unwrap(), &projects, cancel.clone())
            .await
            .unwrap();
        let addr = server.local_addr().unwrap();
        tokio::spawn(server.run());
        Self {
            _data: data,
            _repo: repo,
            service,
            addr,
            cancel,
        }
    }

    fn url(&self) -> String {
        format!("http://{}{}", self.addr, endpoint_path("demo"))
    }

    async fn client(&self, name: &str, protocol: Protocol) -> Client {
        let transport = StreamableHttpClientTransport::from_uri(self.url());
        let info = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new(name, "1.0"),
        );
        let lifecycle = match protocol {
            Protocol::Latest => ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
            Protocol::Legacy => ClientLifecycleMode::Initialize,
        };
        let info = match protocol {
            Protocol::Latest => info,
            Protocol::Legacy => info.with_protocol_version(ProtocolVersion::V_2025_11_25),
        };
        info.serve_with_lifecycle(transport, lifecycle)
            .await
            .unwrap_or_else(|e| panic!("{protocol:?} client failed to start: {e}"))
    }

    /// A task you created, so it is in todo.
    fn todo(&self, title: &str) -> TaskId {
        self.service
            .create_task(
                &Actor::User,
                NewTask {
                    title: title.into(),
                    ..NewTask::default()
                },
            )
            .unwrap()
            .id
    }

    fn file(&self, id: TaskId) -> String {
        std::fs::read_to_string(self.service.task_dir().join(id.file_name())).unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Calls a tool: `Ok` with the structured result, or `Err` with the error.
async fn call(client: &Client, tool: &str, args: Value) -> Result<Value, Value> {
    let Value::Object(args) = args else {
        panic!("arguments must be an object")
    };
    let result = client
        .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(args))
        .await
        .unwrap_or_else(|e| panic!("{tool} failed at the protocol level: {e}"));
    let value = result
        .structured_content
        .expect("every tool returns structured content");
    if result.is_error == Some(true) {
        Err(value["error"].clone())
    } else {
        Ok(value)
    }
}

/// The error code of a refused call.
async fn code(client: &Client, tool: &str, args: Value) -> String {
    match call(client, tool, args).await {
        Ok(value) => panic!("{tool} should fail, got {value}"),
        Err(error) => error["code"].as_str().unwrap().to_owned(),
    }
}

fn keys(schema: &serde_json::Map<String, Value>, field: &str) -> BTreeSet<String> {
    match &schema.get(field) {
        Some(Value::Object(map)) => map.keys().cloned().collect(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|i| i.as_str().unwrap().to_owned())
            .collect(),
        _ => BTreeSet::new(),
    }
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[tokio::test]
async fn every_tool_has_exactly_the_specs_arguments() {
    let env = Env::new().await;
    let client = env.client("claude-code", Protocol::Latest).await;
    let tools = client.list_all_tools().await.unwrap();
    let server = client.peer_info().unwrap();
    assert_eq!(
        server.server_info.as_ref().map(|i| i.name.as_str()),
        Some("tandem")
    );
    assert!(
        server
            .instructions
            .as_deref()
            .unwrap()
            .contains("Commit rules:")
    );

    // (tool, every argument, required arguments). `agent` is the id
    // start_task issues: required where only the lease holder may act.
    let expected: [(&str, &[&str], &[&str]); 11] = [
        (
            "list_tasks",
            &["status", "label", "parent", "query", "ready_only", "agent"],
            &[],
        ),
        ("get_task", &["id", "agent"], &["id"]),
        ("next_task", &["label", "parent", "agent"], &[]),
        (
            "create_task",
            &[
                "title",
                "description",
                "acceptance_criteria",
                "priority",
                "parent",
                "labels",
                "depends_on",
                "agent",
            ],
            &["title", "description", "acceptance_criteria"],
        ),
        (
            "update_task",
            &[
                "id",
                "title",
                "description",
                "acceptance_criteria",
                "relevant_files",
                "priority",
                "parent",
                "labels",
                "depends_on",
                "agent",
            ],
            &["id"],
        ),
        ("start_task", &["id"], &["id"]),
        (
            "log_progress",
            &["id", "message", "agent"],
            &["id", "message", "agent"],
        ),
        (
            "set_blocked",
            &["id", "reason", "agent"],
            &["id", "reason", "agent"],
        ),
        (
            "add_changelog_entry",
            &["id", "summary", "files", "agent"],
            &["id", "summary", "agent"],
        ),
        (
            "finish_task",
            &[
                "id",
                "summary",
                "files_changed",
                "follow_ups",
                "met",
                "unmet",
                "agent",
            ],
            &["id", "summary", "files_changed", "agent"],
        ),
        (
            "release_task",
            &["id", "note", "agent"],
            &["id", "note", "agent"],
        ),
    ];
    assert_eq!(
        tools
            .iter()
            .map(|t| t.name.to_string())
            .collect::<BTreeSet<_>>(),
        expected.iter().map(|(name, ..)| name.to_string()).collect()
    );
    for (name, all, required) in expected {
        let tool = tools.iter().find(|t| t.name == name).unwrap();
        let schema = &tool.input_schema;
        assert_eq!(keys(schema, "properties"), set(all), "{name}");
        assert_eq!(keys(schema, "required"), set(required), "{name}");
        assert_eq!(
            schema.get("additionalProperties"),
            Some(&json!(false)),
            "{name}"
        );
    }

    // Tools that write task files repeat the commit rules.
    for tool in &tools {
        let description = tool.description.as_deref().unwrap_or_default();
        let writes = [
            "create_task",
            "update_task",
            "start_task",
            "add_changelog_entry",
            "finish_task",
            "release_task",
        ]
        .contains(&tool.name.as_ref());
        assert_eq!(
            description.contains("Commit rules: include the task file in your first commit"),
            writes,
            "{}",
            tool.name
        );
    }
}

#[tokio::test]
async fn an_agent_works_a_task_through_on_either_protocol() {
    for protocol in PROTOCOLS {
        let env = Env::new().await;
        let client = env.client("Claude Code", protocol).await;

        // The agent proposes a task; it waits in backlog for approval.
        let created = call(
            &client,
            "create_task",
            json!({
                "title": "Fix token refresh race",
                "description": "Parallel 401s each refresh the token.",
                "acceptance_criteria": ["One refresh at a time", "A test for two 401s"],
                "labels": ["auth"],
            }),
        )
        .await
        .unwrap();
        let id: TaskId = created["id"].as_str().unwrap().parse().unwrap();
        let path = format!(".tandem/{id}.md");
        assert_eq!(created["path"], json!(path));
        assert_eq!(created["status"], json!("backlog"));
        assert_eq!(
            created["reminder"],
            json!(format!("Include {path} in your next commit."))
        );
        assert!(
            env.file(id).contains("· claude-code · created"),
            "{protocol:?}"
        );
        assert_eq!(
            code(&client, "start_task", json!({ "id": id.as_str() })).await,
            "invalid_transition"
        );

        // You approve it on the board; now it can start.
        env.service.approve(id).unwrap();
        let next = call(&client, "next_task", json!({})).await.unwrap();
        assert_eq!(next["task"]["id"], json!(id.as_str()));
        let started = call(&client, "start_task", json!({ "id": id.as_str() }))
            .await
            .unwrap();
        let agent = started["agent"].as_str().unwrap().to_owned();
        assert!(agent.starts_with("claude-code-"), "{agent}");
        assert_eq!(started["status"], json!("in_progress"));
        assert_eq!(
            started["reminder"],
            json!(format!(
                "Include {path} (status in_progress) in your first commit for this task, \
                 with a `Task: {id}` trailer."
            ))
        );

        // Holder calls carry the issued id.
        call(
            &client,
            "log_progress",
            json!({ "id": id.as_str(), "message": "Found the race.", "agent": agent }),
        )
        .await
        .unwrap();
        call(
            &client,
            "set_blocked",
            json!({ "id": id.as_str(), "reason": "Which API?", "agent": agent }),
        )
        .await
        .unwrap();
        let listed = call(&client, "list_tasks", json!({ "agent": agent }))
            .await
            .unwrap();
        assert_eq!(listed["tasks"][0]["lease_holder"], json!(agent));
        assert_eq!(listed["tasks"][0]["blocked"], json!("Which API?"));
        call(
            &client,
            "set_blocked",
            json!({ "id": id.as_str(), "reason": "", "agent": agent }),
        )
        .await
        .unwrap();
        let detail = call(
            &client,
            "get_task",
            json!({ "id": id.as_str(), "agent": agent }),
        )
        .await
        .unwrap();
        assert_eq!(detail["markdown"], json!(env.file(id)));
        assert_eq!(detail["work_log"][0]["message"], json!("Found the race."));
        assert_eq!(detail["work_log"][0]["agent"], json!(agent));

        // Tick or explain.
        let finish = |unmet: Value| {
            json!({
                "id": id.as_str(),
                "summary": "Serialized the refresh.",
                "files_changed": ["src/auth/session.ts"],
                "met": [1],
                "unmet": unmet,
                "agent": agent,
            })
        };
        assert_eq!(
            code(&client, "finish_task", finish(json!([]))).await,
            "criteria_unmet"
        );
        let finished = call(
            &client,
            "finish_task",
            finish(json!([{ "criterion": 2, "reason": "needs a staging account" }])),
        )
        .await
        .unwrap();
        assert_eq!(finished["status"], json!("finished"));
        assert_eq!(
            finished["reminder"],
            json!(format!(
                "Include {path} (status finished) in your final commit for this task, with \
                 a `Task: {id}` trailer."
            ))
        );
        let file = env.file(id);
        assert!(file.contains("- [x] One refresh at a time"));
        assert!(file.contains(&format!("· {agent} · finished\nSerialized the refresh.\n")));
        assert!(file.contains("Unmet: 2 (needs a staging account)"));
    }
}

#[tokio::test]
async fn holder_tools_need_the_issued_agent_id() {
    let env = Env::new().await;
    let client = env.client("claude-code", Protocol::Latest).await;
    let other = env.client("claude-code", Protocol::Latest).await;
    let id = env.todo("Task");
    let started = call(&client, "start_task", json!({ "id": id.as_str() }))
        .await
        .unwrap();
    let agent = started["agent"].as_str().unwrap();

    // Starting again gets a new id, so even the holder is refused.
    assert_eq!(
        code(&client, "start_task", json!({ "id": id.as_str() })).await,
        "claimed_by_other"
    );
    // An id the server never issued, including the bare client name.
    for fake in ["claude-code-0000", "made-up"] {
        let args = json!({ "id": id.as_str(), "message": "x", "agent": fake });
        assert_eq!(code(&other, "log_progress", args).await, "invalid_argument");
    }
    // Another agent's issued id is known but doesn't hold this task.
    let second = env.todo("Second");
    let theirs = call(&other, "start_task", json!({ "id": second.as_str() }))
        .await
        .unwrap();
    let args = json!({ "id": id.as_str(), "note": "mine", "agent": theirs["agent"] });
    assert_eq!(code(&other, "release_task", args).await, "not_holder");
    // Arguments that don't fit the schema get the same structured error.
    let args = json!({ "id": id.as_str(), "message": "x" });
    assert_eq!(
        code(&client, "log_progress", args).await,
        "invalid_argument"
    );
    let args = json!({ "id": id.as_str(), "surprise": true });
    assert_eq!(code(&client, "get_task", args).await, "invalid_argument");
    // The holder still can.
    call(
        &client,
        "release_task",
        json!({ "id": id.as_str(), "note": "Out of time.", "agent": agent }),
    )
    .await
    .unwrap();
    assert_eq!(
        env.service
            .list_tasks(&Actor::User, &Default::default())
            .unwrap()[0]
            .status,
        Status::Todo
    );
}

#[tokio::test]
async fn errors_carry_the_specs_codes() {
    let env = Env::new().await;
    let client = env.client("claude-code", Protocol::Latest).await;
    let missing = "zzzzzzz";
    assert_eq!(
        code(&client, "get_task", json!({ "id": missing })).await,
        "not_found"
    );
    assert_eq!(
        code(&client, "get_task", json!({ "id": "nope" })).await,
        "invalid_argument"
    );
    let error = call(&client, "get_task", json!({ "id": missing }))
        .await
        .unwrap_err();
    assert_eq!(
        error["message"],
        json!(format!("task `{missing}` does not exist"))
    );

    let top = env.todo("Top");
    let child = call(
        &client,
        "create_task",
        json!({
            "title": "Child",
            "description": "",
            "acceptance_criteria": [],
            "parent": top.as_str(),
        }),
    )
    .await
    .unwrap();
    let create = |parent: &str| {
        json!({
            "title": "New",
            "description": "",
            "acceptance_criteria": [],
            "parent": parent,
        })
    };
    assert_eq!(
        code(&client, "create_task", create(missing)).await,
        "parent_not_found"
    );
    // You move the top task under its child: a loop.
    let edit = json!({ "id": top.as_str(), "parent": child["id"] });
    assert_eq!(code(&client, "update_task", edit).await, "cycle_detected");
    // null moves a task to the top level.
    let edit = json!({ "id": child["id"], "parent": null });
    call(&client, "update_task", edit).await.unwrap();
    assert!(
        !env.file(child["id"].as_str().unwrap().parse().unwrap())
            .contains("parent:")
    );

    let dep = env.todo("Dependency");
    let blocked = env
        .service
        .create_task(
            &Actor::User,
            NewTask {
                title: "Blocked".into(),
                depends_on: vec![dep],
                ..NewTask::default()
            },
        )
        .unwrap()
        .id;
    assert_eq!(
        code(&client, "start_task", json!({ "id": blocked.as_str() })).await,
        "deps_not_done"
    );
}

#[tokio::test]
async fn two_agents_racing_for_a_task_have_one_winner() {
    let env = Env::new().await;
    // Two instances of the same client: only issued ids tell them apart.
    let first = env.client("claude-code", Protocol::Latest).await;
    let second = env.client("claude-code", Protocol::Legacy).await;
    for round in 0..20 {
        let id = env.todo(&format!("Task {round}"));
        let args = json!({ "id": id.as_str() });
        let (a, b) = tokio::join!(
            call(&first, "start_task", args.clone()),
            call(&second, "start_task", args)
        );
        let (winner, loser) = match (a, b) {
            (Ok(winner), Err(loser)) | (Err(loser), Ok(winner)) => (winner, loser),
            other => panic!("round {round}: expected one winner, got {other:?}"),
        };
        assert_eq!(loser["code"], json!("claimed_by_other"));
        let file = env.file(id);
        assert_eq!(file.matches("· started").count(), 1);
        assert!(file.contains(winner["agent"].as_str().unwrap()));
    }
}

#[tokio::test]
async fn only_project_endpoints_on_localhost_answer() {
    let env = Env::new().await;
    let http = reqwest::Client::new();
    let initialize = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "curl", "version": "1" },
        },
    });
    let post = |url: String| {
        http.post(url)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .body(initialize.to_string())
    };
    let ok = post(env.url()).send().await.unwrap();
    assert!(ok.status().is_success(), "{}", ok.status());

    let base = format!("http://{}", env.addr);
    for path in ["/p/other/mcp", "/mcp", "/p/demo", "/"] {
        let response = post(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(response.status(), 404, "{path}");
    }
    // DNS rebinding: a request naming another host.
    let rebound = post(env.url())
        .header("Host", "evil.example")
        .send()
        .await
        .unwrap();
    assert!(rebound.status().is_client_error(), "{}", rebound.status());
    // A web page calling from a browser.
    let browser = post(env.url())
        .header("Origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert!(browser.status().is_client_error(), "{}", browser.status());
}

#[tokio::test]
async fn the_server_only_listens_on_localhost() {
    let projects = BTreeMap::new();
    let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
    assert!(matches!(
        McpServer::bind(addr, &projects, CancellationToken::new()).await,
        Err(ServeError::NotLoopback(_))
    ));
    assert_eq!(tandem_board_mcp::DEFAULT_ADDR.to_string(), "127.0.0.1:4317");
    assert_eq!(endpoint_path("tandem"), "/p/tandem/mcp");
}
