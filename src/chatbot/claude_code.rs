//! Claude Code CLI - simple message relay with session persistence.
//!
//! Spawns a persistent Claude Code process and relays messages to it.
//! Claude Code maintains conversation history internally.
//! Uses --resume to continue previous sessions across restarts.
//!
//! SECURITY: Uses `--tools ""` to disable all built-in tools.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::tools::ToolCall;

/// JSON schema for structured output - tool_calls array.
const TOOL_CALLS_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "tool_calls": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "tool": { "type": "string" },
          "chat_id": { "type": "integer" },
          "text": { "type": "string" },
          "reply_to_message_id": { "type": "integer" },
          "user_id": { "type": "integer" },
          "message_id": { "type": "integer" },
          "emoji": { "type": "string" },
          "last_n": { "type": "integer" },
          "from_date": { "type": "string" },
          "to_date": { "type": "string" },
          "username": { "type": "string" },
          "limit": { "type": "integer" },
          "query": { "type": "string" },
          "duration_minutes": { "type": "integer" },
          "days_inactive": { "type": "integer" },
          "filter": { "type": "string" },
          "file_path": { "type": "string" }
        },
        "required": ["tool"]
      }
    }
  },
  "required": ["tool_calls"]
}"#;

/// Tool call with ID for tracking.
#[derive(Debug, Clone)]
pub struct ToolCallWithId {
    pub id: String,
    pub call: ToolCall,
}

/// Tool execution result.
#[derive(Debug)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

/// Response from Claude Code.
#[derive(Debug)]
pub struct Response {
    pub tool_calls: Vec<ToolCallWithId>,
    /// True if context compaction occurred during this response.
    pub compacted: bool,
}

/// Claude Code client - maintains persistent subprocess.
pub struct ClaudeCode {
    tx: mpsc::Sender<WorkerMessage>,
    rx: mpsc::Receiver<Response>,
}

enum WorkerMessage {
    UserMessage(String),
    ToolResults(Vec<ToolResult>),
}

impl ClaudeCode {
    /// Start Claude Code, optionally resuming a previous session.
    /// If session_file exists, resume that session. Otherwise start fresh with system_prompt.
    pub fn start(system_prompt: String, session_file: Option<PathBuf>) -> Result<Self, String> {
        let (msg_tx, msg_rx) = mpsc::channel::<WorkerMessage>(32);
        let (resp_tx, resp_rx) = mpsc::channel::<Response>(32);

        // Check for existing session
        let resume_session = session_file.as_ref().and_then(|p| load_session_id(p));

        std::thread::spawn(move || {
            if let Err(e) = worker_loop(system_prompt, resume_session, session_file, msg_rx, resp_tx) {
                error!("Claude Code worker died: {}", e);
            }
        });

        Ok(Self { tx: msg_tx, rx: resp_rx })
    }

    /// Send a user message and get response.
    pub async fn send_message(&mut self, content: String) -> Result<Response, String> {
        self.tx
            .send(WorkerMessage::UserMessage(content))
            .await
            .map_err(|_| "Worker channel closed")?;

        self.rx
            .recv()
            .await
            .ok_or_else(|| "Response channel closed".to_string())
    }

    /// Send tool results and get next response.
    pub async fn send_tool_results(&mut self, results: Vec<ToolResult>) -> Result<Response, String> {
        self.tx
            .send(WorkerMessage::ToolResults(results))
            .await
            .map_err(|_| "Worker channel closed")?;

        self.rx
            .recv()
            .await
            .ok_or_else(|| "Response channel closed".to_string())
    }
}

#[derive(Serialize)]
struct InputMessage {
    #[serde(rename = "type")]
    msg_type: String,
    message: InputContent,
}

#[derive(Serialize)]
struct InputContent {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum OutputMessage {
    #[serde(rename = "system")]
    System {
        #[serde(default)]
        tools: Vec<String>,
        #[serde(default)]
        session_id: Option<String>,
    },
    #[serde(rename = "assistant")]
    Assistant {
        #[serde(default)]
        message: Option<AssistantMessage>,
    },
    #[serde(rename = "result")]
    Result {
        #[serde(default)]
        total_cost_usd: f64,
        #[serde(default)]
        structured_output: Option<StructuredOutput>,
        #[serde(default)]
        session_id: Option<String>,
    },
    #[serde(other)]
    Other,
}

/// Assistant message with context management info.
#[derive(Debug, Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    context_management: Option<ContextManagement>,
}

/// Context management info from compaction.
#[derive(Debug, Deserialize)]
struct ContextManagement {
    #[serde(default)]
    truncated_content_length: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct StructuredOutput {
    tool_calls: Vec<RawToolCall>,
}

#[derive(Debug, Deserialize)]
struct RawToolCall {
    tool: String,
    #[serde(default)]
    chat_id: Option<i64>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    reply_to_message_id: Option<i64>,
    #[serde(default)]
    user_id: Option<i64>,
    #[serde(default)]
    message_id: Option<i64>,
    #[serde(default)]
    emoji: Option<String>,
    #[serde(default)]
    last_n: Option<i64>,
    #[serde(default)]
    from_date: Option<String>,
    #[serde(default)]
    to_date: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    duration_minutes: Option<i64>,
    #[serde(default)]
    days_inactive: Option<i64>,
    #[serde(default)]
    filter: Option<String>,
    #[serde(default)]
    file_path: Option<String>,
}

impl RawToolCall {
    /// Convert raw tool call to typed ToolCall.
    /// Returns None if required fields are missing (logs warning).
    fn to_tool_call(&self) -> Option<ToolCall> {
        let result = match self.tool.as_str() {
            "send_message" => Some(ToolCall::SendMessage {
                chat_id: self.chat_id?,
                text: self.text.clone().unwrap_or_default(),
                reply_to_message_id: self.reply_to_message_id,
            }),
            "get_user_info" => Some(ToolCall::GetUserInfo {
                user_id: self.user_id?,
            }),
            "read_messages" => Some(ToolCall::ReadMessages {
                last_n: self.last_n,
                from_date: self.from_date.clone(),
                to_date: self.to_date.clone(),
                username: self.username.clone(),
                limit: self.limit,
            }),
            "add_reaction" => Some(ToolCall::AddReaction {
                chat_id: self.chat_id?,
                message_id: self.message_id?,
                emoji: self.emoji.clone().unwrap_or_default(),
            }),
            "web_search" => Some(ToolCall::WebSearch {
                query: self.query.clone()?,
            }),
            "delete_message" => Some(ToolCall::DeleteMessage {
                chat_id: self.chat_id?,
                message_id: self.message_id?,
            }),
            "mute_user" => Some(ToolCall::MuteUser {
                chat_id: self.chat_id?,
                user_id: self.user_id?,
                duration_minutes: self.duration_minutes.unwrap_or(5),
            }),
            "ban_user" => Some(ToolCall::BanUser {
                chat_id: self.chat_id?,
                user_id: self.user_id?,
            }),
            "kick_user" => Some(ToolCall::KickUser {
                chat_id: self.chat_id?,
                user_id: self.user_id?,
            }),
            "get_chat_admins" => Some(ToolCall::GetChatAdmins {
                chat_id: self.chat_id?,
            }),
            "get_members" => Some(ToolCall::GetMembers {
                filter: self.filter.clone(),
                days_inactive: self.days_inactive,
                limit: self.limit,
            }),
            "import_members" => Some(ToolCall::ImportMembers {
                file_path: self.file_path.clone()?,
            }),
            "send_photo" => Some(ToolCall::SendPhoto {
                chat_id: self.chat_id?,
                prompt: self.text.clone()?, // Claude uses "text" for the prompt
                caption: None, // Could add caption field later
                reply_to_message_id: self.reply_to_message_id,
            }),
            "done" => Some(ToolCall::Done),
            _ => {
                warn!("Unknown tool: {}", self.tool);
                None
            }
        };

        if result.is_none() && self.tool != "done" {
            warn!("Tool '{}' missing required fields: {:?}", self.tool, self);
        }

        result
    }
}

fn load_session_id(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn save_session_id(path: &Path, session_id: &str) {
    if let Err(e) = std::fs::write(path, session_id) {
        warn!("Failed to save session ID: {}", e);
    } else {
        info!("Saved session ID to {:?}", path);
    }
}

fn worker_loop(
    system_prompt: String,
    resume_session: Option<String>,
    session_file: Option<PathBuf>,
    mut msg_rx: mpsc::Receiver<WorkerMessage>,
    resp_tx: mpsc::Sender<Response>,
) -> Result<(), String> {
    let mut process = spawn_process(resume_session.as_deref())?;
    let mut stdin = process.stdin.take().ok_or("No stdin")?;
    let stdout = process.stdout.take().ok_or("No stdout")?;

    info!("🚀 Claude Code started (PID {})", process.id());

    let (out_tx, mut out_rx) = mpsc::channel::<OutputMessage>(100);

    // Stdout reader thread
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) if !l.is_empty() => l,
                Ok(_) => continue,
                Err(e) => {
                    warn!("Read error: {}", e);
                    break;
                }
            };

            match serde_json::from_str::<OutputMessage>(&line) {
                Ok(msg) => {
                    if out_tx.blocking_send(msg).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    debug!("Parse error: {} ({})", e, &line[..line.len().min(80)]);
                }
            }
        }
    });

    // Send first message to trigger Claude Code output
    // Claude Code only outputs after receiving input
    let is_resumed = resume_session.is_some();
    let first_message = if is_resumed {
        "Session resumed. Ready for new messages.".to_string()
    } else {
        system_prompt.clone()
    };
    send_message(&mut stdin, &first_message)?;

    // Now wait for system message (comes first in output)
    let mut session_id: Option<String> = None;
    loop {
        match out_rx.blocking_recv() {
            Some(OutputMessage::System { tools, session_id: sid }) => {
                let non_schema: Vec<_> = tools.iter().filter(|t| *t != "StructuredOutput").collect();
                if !non_schema.is_empty() {
                    error!("SECURITY: Unexpected tools: {:?}", non_schema);
                    return Err("Security violation".to_string());
                }
                if let Some(sid) = sid {
                    info!("Got session ID: {}", sid);
                    session_id = Some(sid);
                }
                info!("🤖 Claude Code session ready");
                break;
            }
            Some(_) => continue,
            None => return Err("Output channel closed".to_string()),
        }
    }

    // Wait for result of first message
    let (_, new_sid) = wait_for_result(&mut out_rx)?;
    if let Some(sid) = new_sid {
        session_id = Some(sid);
    }
    info!("First message processed, ready for chat");

    // Save session ID if we have one and a file path
    if let (Some(sid), Some(path)) = (&session_id, &session_file) {
        save_session_id(path, sid);
    }

    // Main loop
    while let Some(msg) = msg_rx.blocking_recv() {
        let content = match msg {
            WorkerMessage::UserMessage(content) => content,
            WorkerMessage::ToolResults(results) => format_tool_results(&results),
        };

        send_message(&mut stdin, &content)?;
        let (response, new_sid) = wait_for_result(&mut out_rx)?;

        // Update session ID if changed
        if let Some(sid) = new_sid {
            if session_id.as_ref() != Some(&sid) {
                session_id = Some(sid.clone());
                if let Some(ref path) = session_file {
                    save_session_id(path, &sid);
                }
            }
        }

        if resp_tx.blocking_send(response).is_err() {
            break;
        }
    }

    info!("Claude Code worker shutting down");
    drop(stdin);
    let _ = process.wait();
    Ok(())
}

fn spawn_process(resume_session: Option<&str>) -> Result<Child, String> {
    let schema: serde_json::Value = serde_json::from_str(TOOL_CALLS_SCHEMA)
        .map_err(|e| format!("Bad schema: {}", e))?;
    let schema_str = serde_json::to_string(&schema)
        .map_err(|e| format!("Failed to serialize schema: {}", e))?;

    let mut cmd = Command::new("claude");
    cmd.args([
        "--print",
        "--input-format", "stream-json",
        "--output-format", "stream-json",
        "--verbose",
        "--model", "opus",
        "--tools", "",  // SECURITY: disable all tools
        "--json-schema", &schema_str,
    ]);

    // Add --resume if we have a session to resume
    if let Some(session_id) = resume_session {
        info!("Resuming session: {}", session_id);
        cmd.args(["--resume", session_id]);
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Spawn failed: {}", e))
}

fn send_message(stdin: &mut ChildStdin, content: &str) -> Result<(), String> {
    let msg = InputMessage {
        msg_type: "user".to_string(),
        message: InputContent {
            role: "user".to_string(),
            content: content.to_string(),
        },
    };

    let json = serde_json::to_string(&msg).map_err(|e| format!("Serialize: {}", e))?;
    stdin.write_all(json.as_bytes()).map_err(|e| format!("Write: {}", e))?;
    stdin.write_all(b"\n").map_err(|e| format!("Write newline: {}", e))?;
    stdin.flush().map_err(|e| format!("Flush: {}", e))?;

    debug!("Sent message (len={})", content.len());
    Ok(())
}

/// Wait for result, return (Response, Option<session_id>)
fn wait_for_result(out_rx: &mut mpsc::Receiver<OutputMessage>) -> Result<(Response, Option<String>), String> {
    let mut compacted = false;

    loop {
        match out_rx.blocking_recv() {
            Some(OutputMessage::Assistant { message }) => {
                // Check for context compaction
                if let Some(msg) = message {
                    if let Some(ctx) = msg.context_management {
                        if ctx.truncated_content_length.is_some() {
                            warn!("Context compaction detected!");
                            compacted = true;
                        }
                    }
                }
            }
            Some(OutputMessage::Result { total_cost_usd, structured_output, session_id }) => {
                info!("🤖 Response (cost: ${:.4})", total_cost_usd);

                let tool_calls = match structured_output {
                    Some(so) => {
                        so.tool_calls
                            .iter()
                            .enumerate()
                            .filter_map(|(i, tc)| {
                                tc.to_tool_call().map(|call| ToolCallWithId {
                                    id: format!("tool_{}", i),
                                    call,
                                })
                            })
                            .collect()
                    }
                    None => {
                        warn!("No structured output");
                        Vec::new()
                    }
                };

                info!("Got {} tool call(s){}", tool_calls.len(), if compacted { " (after compaction)" } else { "" });
                return Ok((Response { tool_calls, compacted }, session_id));
            }
            Some(OutputMessage::System { .. }) => continue,
            Some(OutputMessage::Other) => continue,
            None => return Err("Output channel closed".to_string()),
        }
    }
}

fn format_tool_results(results: &[ToolResult]) -> String {
    let mut s = String::from("Tool results:\n");
    for r in results {
        s.push_str(&format!(
            "- {}: {}{}\n",
            r.tool_use_id,
            r.content,
            if r.is_error { " (ERROR)" } else { "" }
        ));
    }
    s
}
