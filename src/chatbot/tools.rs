//! Tool definitions for Claude to interact with the group.

use serde::{Deserialize, Serialize};

/// Tool definition for Claude.
#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Tool calls that Claude can make.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    /// Send a message to a chat.
    SendMessage {
        /// Target chat ID (required - use the chat_id from the message you're responding to)
        chat_id: i64,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reply_to_message_id: Option<i64>,
    },

    /// Get info about a user by ID or username.
    GetUserInfo {
        #[serde(skip_serializing_if = "Option::is_none")]
        user_id: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        username: Option<String>,
    },

    /// Execute a SQL SELECT query on the database.
    Query {
        /// SQL SELECT query. Must start with SELECT. Max 100 rows returned.
        sql: String,
    },

    /// Add a reaction emoji to a message.
    AddReaction {
        /// Target chat ID (use the chat_id from the message you're reacting to)
        chat_id: i64,
        /// Message ID to react to
        message_id: i64,
        /// Emoji to react with (e.g. "👍", "❤", "🔥", "😂")
        emoji: String,
    },

    /// Delete a message (admin action - use for spam/abuse).
    DeleteMessage {
        chat_id: i64,
        message_id: i64,
    },

    /// Mute a user temporarily (admin action).
    MuteUser {
        chat_id: i64,
        user_id: i64,
        /// Duration in minutes (1-1440, i.e. up to 24 hours)
        duration_minutes: i64,
    },

    /// Ban a user permanently (admin action - use for severe abuse).
    BanUser {
        chat_id: i64,
        user_id: i64,
    },

    /// Kick a user from the group (softer than ban - they can rejoin).
    KickUser {
        chat_id: i64,
        user_id: i64,
    },

    /// Get list of chat administrators.
    GetChatAdmins {
        chat_id: i64,
    },

    /// Get list of known members from the database.
    GetMembers {
        /// Filter: "all", "active", "inactive", "never_posted", "left", "banned" (default "all")
        #[serde(default)]
        filter: Option<String>,
        /// For "inactive" filter: minimum days since last message (default 30)
        #[serde(default)]
        days_inactive: Option<i64>,
        /// Maximum users to return (default 50)
        #[serde(default)]
        limit: Option<i64>,
    },

    /// Import members from a JSON file (backfill from browser extension export).
    ImportMembers {
        /// Path to JSON file containing member array
        file_path: String,
    },

    /// Send an image to a chat.
    SendPhoto {
        /// Target chat ID
        chat_id: i64,
        /// Text prompt to generate an AI image (uses Gemini/Nano Banana)
        prompt: String,
        /// Optional caption for the image
        #[serde(skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
        /// Optional message ID to reply to
        #[serde(skip_serializing_if = "Option::is_none")]
        reply_to_message_id: Option<i64>,
    },

    /// Send a voice message (TTS).
    SendVoice {
        /// Target chat ID
        chat_id: i64,
        /// Text to convert to speech
        text: String,
        /// Optional voice name (default: "af_heart" - American English female)
        #[serde(skip_serializing_if = "Option::is_none")]
        voice: Option<String>,
        /// Optional message ID to reply to
        #[serde(skip_serializing_if = "Option::is_none")]
        reply_to_message_id: Option<i64>,
    },

    // === Memory Tools ===

    /// Create a new memory file. Fails if file already exists.
    CreateMemory {
        /// Relative path within memories directory (e.g. "users/nodir.md")
        path: String,
        /// Content to write
        content: String,
    },

    /// Read a memory file with line numbers.
    ReadMemory {
        /// Relative path within memories directory
        path: String,
    },

    /// Edit a memory file. Requires the file to have been read first.
    EditMemory {
        /// Relative path within memories directory
        path: String,
        /// Exact string to find and replace
        old_string: String,
        /// Replacement string
        new_string: String,
    },

    /// List files in the memories directory.
    ListMemories {
        /// Optional subdirectory path (default: root of memories)
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },

    /// Search for a pattern across memory files (like grep).
    SearchMemories {
        /// Search pattern (substring match)
        pattern: String,
        /// Optional subdirectory to search in
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },

    /// Delete a memory file.
    DeleteMemory {
        /// Relative path within memories directory
        path: String,
    },

    /// Report a bug or issue to the developer (Claude Code).
    ReportBug {
        /// Description of the bug or issue
        description: String,
        /// Severity: "low", "medium", "high", "critical"
        #[serde(default)]
        severity: Option<String>,
    },

    /// Get YouTube video metadata from a URL.
    YoutubeInfo {
        /// YouTube URL (e.g. https://www.youtube.com/watch?v=xyz or https://youtu.be/xyz)
        url: String,
    },

    // === Reminder Tools ===

    /// Set a reminder to send a message at a future time.
    SetReminder {
        /// Chat ID where the reminder message will be sent
        chat_id: i64,
        /// The message to send when the reminder triggers
        message: String,
        /// When to trigger: relative ("+30m", "+2h", "+1d") or absolute ("2026-01-25 15:00")
        trigger_at: String,
        /// Optional cron expression for recurring reminders (e.g. "0 9 * * *" for daily at 9am)
        #[serde(skip_serializing_if = "Option::is_none")]
        repeat_cron: Option<String>,
    },

    /// List active reminders.
    ListReminders {
        /// Optional chat ID to filter by (omit for all chats)
        #[serde(skip_serializing_if = "Option::is_none")]
        chat_id: Option<i64>,
    },

    /// Cancel a reminder by ID.
    CancelReminder {
        /// The reminder ID to cancel
        reminder_id: i64,
    },

    // === Admin Tools (owner only) ===

    /// Add a user to the trusted DM users list. Owner only.
    AddTrustedUser {
        /// User ID to add to trusted list
        user_id: i64,
    },

    /// Remove a user from the trusted DM users list. Owner only.
    RemoveTrustedUser {
        /// User ID to remove from trusted list
        user_id: i64,
    },

    /// Do nothing - acknowledge a message without taking action.
    Noop,

    /// Signal that processing is complete.
    Done,

    /// Parse error - tool call couldn't be parsed. Error message will be sent back to model.
    #[serde(skip)]
    ParseError { message: String },
}

/// Get the tool definitions for Claude.
pub fn get_tool_definitions() -> Vec<Tool> {
    vec![
        Tool {
            name: "send_message".to_string(),
            description: "Send a message to a chat. Use the chat_id from the message you're responding to.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "chat_id": {
                        "type": "integer",
                        "description": "Target chat ID (use the chat_id from the incoming message)"
                    },
                    "text": {
                        "type": "string",
                        "description": "The message text to send"
                    },
                    "reply_to_message_id": {
                        "type": "integer",
                        "description": "Optional message ID to reply to"
                    }
                },
                "required": ["chat_id", "text"]
            }),
        },
        Tool {
            name: "get_user_info".to_string(),
            description: "Get detailed information about a user including their profile photo. Returns: user_id, username, first_name, last_name, is_bot, is_premium, language_code, status (owner/administrator/member/restricted/banned), custom_title, and profile_photo_base64. Username lookup only works for users seen in the group.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "user_id": {
                        "type": "integer",
                        "description": "The user ID to look up"
                    },
                    "username": {
                        "type": "string",
                        "description": "Username to look up (case-insensitive partial match)"
                    }
                }
            }),
        },
        Tool {
            name: "query".to_string(),
            description: "Execute a SQL SELECT query on the database. Tables: 'messages' (message_id, chat_id, user_id, username, timestamp, text, reply_to_id, reply_to_username, reply_to_text) and 'users' (user_id, username, first_name, join_date, last_message_date, message_count, status). Indexes exist on timestamp, user_id, username. Max 100 rows returned, text truncated to 100 chars.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "sql": {
                        "type": "string",
                        "description": "SQL SELECT query. Only SELECT is allowed. Examples: 'SELECT * FROM messages ORDER BY timestamp DESC LIMIT 10', 'SELECT username, message_count FROM users WHERE status = \"member\" ORDER BY message_count DESC LIMIT 20'"
                    }
                },
                "required": ["sql"]
            }),
        },
        Tool {
            name: "add_reaction".to_string(),
            description: "Add an emoji reaction to a message. Use sparingly - only when a reaction is more appropriate than a reply.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "chat_id": {
                        "type": "integer",
                        "description": "Target chat ID (use the chat_id from the message)"
                    },
                    "message_id": {
                        "type": "integer",
                        "description": "Message ID to react to"
                    },
                    "emoji": {
                        "type": "string",
                        "description": "Emoji to react with (e.g. 👍, ❤, 🔥, 😂, 🎉, 👀, 🤔)"
                    }
                },
                "required": ["chat_id", "message_id", "emoji"]
            }),
        },
        Tool {
            name: "delete_message".to_string(),
            description: "Delete a message. Use for spam, abuse, or rule violations. Owner will be notified.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "chat_id": { "type": "integer", "description": "Chat ID" },
                    "message_id": { "type": "integer", "description": "Message ID to delete" }
                },
                "required": ["chat_id", "message_id"]
            }),
        },
        Tool {
            name: "mute_user".to_string(),
            description: "Temporarily mute a user (prevent them from posting). Use for minor violations. Duration 1-1440 minutes. Owner will be notified.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "chat_id": { "type": "integer", "description": "Chat ID" },
                    "user_id": { "type": "integer", "description": "User ID to mute" },
                    "duration_minutes": { "type": "integer", "description": "Duration in minutes (1-1440)" }
                },
                "required": ["chat_id", "user_id", "duration_minutes"]
            }),
        },
        Tool {
            name: "ban_user".to_string(),
            description: "Permanently ban a user. Use only for severe abuse (spam bots, repeated violations). Owner will be notified.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "chat_id": { "type": "integer", "description": "Chat ID" },
                    "user_id": { "type": "integer", "description": "User ID to ban" }
                },
                "required": ["chat_id", "user_id"]
            }),
        },
        Tool {
            name: "kick_user".to_string(),
            description: "Kick a user from the group. Softer than ban - they can rejoin via invite link. Use for inactive members or minor issues. Owner will be notified.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "chat_id": { "type": "integer", "description": "Chat ID" },
                    "user_id": { "type": "integer", "description": "User ID to kick" }
                },
                "required": ["chat_id", "user_id"]
            }),
        },
        Tool {
            name: "get_chat_admins".to_string(),
            description: "Get list of chat administrators.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "chat_id": { "type": "integer", "description": "Chat ID" }
                },
                "required": ["chat_id"]
            }),
        },
        Tool {
            name: "get_members".to_string(),
            description: "Get list of known members from the database. Only includes members tracked since this feature was enabled.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "filter": {
                        "type": "string",
                        "description": "Filter: 'all', 'active', 'inactive', 'never_posted', 'left', 'banned' (default 'all')",
                        "enum": ["all", "active", "inactive", "never_posted", "left", "banned"]
                    },
                    "days_inactive": { "type": "integer", "description": "For 'inactive' filter: min days since last post (default 30)" },
                    "limit": { "type": "integer", "description": "Max users to return (default 50)" }
                }
            }),
        },
        Tool {
            name: "import_members".to_string(),
            description: "Import members from a JSON file (for backfilling from browser extension export). Only Dima can use this.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "Path to JSON file with member array" }
                },
                "required": ["file_path"]
            }),
        },
        Tool {
            name: "send_photo".to_string(),
            description: "Generate an AI image and send it to a chat. Uses Gemini/Nano Banana for image generation.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "chat_id": { "type": "integer", "description": "Target chat ID" },
                    "prompt": { "type": "string", "description": "Text prompt describing the image to generate" },
                    "caption": { "type": "string", "description": "Optional caption for the image" },
                    "reply_to_message_id": { "type": "integer", "description": "Optional message ID to reply to" }
                },
                "required": ["chat_id", "prompt"]
            }),
        },
        Tool {
            name: "send_voice".to_string(),
            description: "Send a voice message using text-to-speech. Use this to speak to users instead of typing. Good for greetings, announcements, or when a voice reply feels more personal.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "chat_id": { "type": "integer", "description": "Target chat ID" },
                    "text": { "type": "string", "description": "Text to convert to speech" },
                    "voice": { "type": "string", "description": "Voice name (default: 'af_heart' - American English female). Options: af_heart, af_bella, am_adam, am_michael" },
                    "reply_to_message_id": { "type": "integer", "description": "Optional message ID to reply to" }
                },
                "required": ["chat_id", "text"]
            }),
        },
        // === Memory Tools ===
        Tool {
            name: "create_memory".to_string(),
            description: "Create a new memory file. Fails if file already exists - use edit_memory to modify existing files.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path within memories directory (e.g. 'users/nodir.md')" },
                    "content": { "type": "string", "description": "Content to write to the file" }
                },
                "required": ["path", "content"]
            }),
        },
        Tool {
            name: "read_memory".to_string(),
            description: "Read a memory file. Returns content with line numbers. Must read before editing.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path within memories directory" }
                },
                "required": ["path"]
            }),
        },
        Tool {
            name: "edit_memory".to_string(),
            description: "Edit a memory file by replacing a string. File must have been read first in this session.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path within memories directory" },
                    "old_string": { "type": "string", "description": "Exact string to find and replace" },
                    "new_string": { "type": "string", "description": "Replacement string" }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
        Tool {
            name: "list_memories".to_string(),
            description: "List files in the memories directory.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Optional subdirectory path (default: root)" }
                }
            }),
        },
        Tool {
            name: "search_memories".to_string(),
            description: "Search for a pattern across memory files (like grep).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Search pattern (substring match)" },
                    "path": { "type": "string", "description": "Optional subdirectory to search in" }
                },
                "required": ["pattern"]
            }),
        },
        Tool {
            name: "delete_memory".to_string(),
            description: "Delete a memory file.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path within memories directory" }
                },
                "required": ["path"]
            }),
        },
        Tool {
            name: "report_bug".to_string(),
            description: "Report a bug or issue to the developer (Claude Code). Use this when you encounter unexpected behavior, errors, or problems you can't resolve. The developer monitors these reports and will fix issues.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "Detailed description of the bug or issue" },
                    "severity": { "type": "string", "description": "Severity level: low, medium, high, or critical" }
                },
                "required": ["description"]
            }),
        },
        Tool {
            name: "youtube_info".to_string(),
            description: "Get metadata about a YouTube video (title, author, thumbnail). Works with youtube.com and youtu.be URLs.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "YouTube video URL (e.g. https://www.youtube.com/watch?v=xyz or https://youtu.be/xyz)" }
                },
                "required": ["url"]
            }),
        },
        Tool {
            name: "noop".to_string(),
            description: "Do nothing - use this to acknowledge a system message or notification without taking any action.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        // === Reminder Tools ===
        Tool {
            name: "set_reminder".to_string(),
            description: "Set a reminder to send a message at a future time. Use for scheduling messages, alerts, or recurring announcements.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "chat_id": { "type": "integer", "description": "Chat ID where the reminder will be sent" },
                    "message": { "type": "string", "description": "The message to send when the reminder triggers" },
                    "trigger_at": { "type": "string", "description": "When to trigger: relative ('+30m', '+2h', '+1d') or absolute ('2026-01-25 15:00')" },
                    "repeat_cron": { "type": "string", "description": "Optional 7-field cron (sec min hour day month dow year). E.g. '0 0 9 * * * *' for daily 9am, '0 0 0 * * 1 *' for Mondays" }
                },
                "required": ["chat_id", "message", "trigger_at"]
            }),
        },
        Tool {
            name: "list_reminders".to_string(),
            description: "List active reminders. Returns ID, message, trigger time, and whether it's recurring.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "chat_id": { "type": "integer", "description": "Optional chat ID to filter by (omit for all)" }
                }
            }),
        },
        Tool {
            name: "cancel_reminder".to_string(),
            description: "Cancel a reminder by its ID. Get the ID from list_reminders.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "reminder_id": { "type": "integer", "description": "The reminder ID to cancel" }
                },
                "required": ["reminder_id"]
            }),
        },
        // === Admin Tools (owner only) ===
        Tool {
            name: "add_trusted_user".to_string(),
            description: "Add a user to the trusted DM users list. Only the owner can use this. After adding, the bot will restart to apply changes.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "user_id": { "type": "integer", "description": "User ID to add to trusted list" }
                },
                "required": ["user_id"]
            }),
        },
        Tool {
            name: "remove_trusted_user".to_string(),
            description: "Remove a user from the trusted DM users list. Only the owner can use this. After removing, the bot will restart to apply changes.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "user_id": { "type": "integer", "description": "User ID to remove from trusted list" }
                },
                "required": ["user_id"]
            }),
        },
        Tool {
            name: "done".to_string(),
            description: "Signal that you're done processing. Call this when you have nothing more to do. You don't have to respond to every message - if there's nothing to say, just call done.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_call_serialize() {
        let call = ToolCall::SendMessage {
            chat_id: -12345,
            text: "hello".to_string(),
            reply_to_message_id: Some(123),
        };

        let json = serde_json::to_string(&call).unwrap();
        assert!(json.contains("send_message"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn test_tool_call_deserialize() {
        let json = r#"{"tool": "send_message", "chat_id": -12345, "text": "hello", "reply_to_message_id": 123}"#;
        let call: ToolCall = serde_json::from_str(json).unwrap();

        match call {
            ToolCall::SendMessage {
                chat_id,
                text,
                reply_to_message_id,
            } => {
                assert_eq!(chat_id, -12345);
                assert_eq!(text, "hello");
                assert_eq!(reply_to_message_id, Some(123));
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_get_tool_definitions() {
        let tools = get_tool_definitions();
        assert_eq!(tools.len(), 28);
        assert_eq!(tools[0].name, "send_message");
        assert_eq!(tools[1].name, "get_user_info");
        assert_eq!(tools[2].name, "query");
        assert_eq!(tools[3].name, "add_reaction");
        assert_eq!(tools[4].name, "delete_message");
        assert_eq!(tools[5].name, "mute_user");
        assert_eq!(tools[6].name, "ban_user");
        assert_eq!(tools[7].name, "kick_user");
        assert_eq!(tools[8].name, "get_chat_admins");
        assert_eq!(tools[9].name, "get_members");
        assert_eq!(tools[10].name, "import_members");
        assert_eq!(tools[11].name, "send_photo");
        assert_eq!(tools[12].name, "send_voice");
        assert_eq!(tools[13].name, "create_memory");
        assert_eq!(tools[14].name, "read_memory");
        assert_eq!(tools[15].name, "edit_memory");
        assert_eq!(tools[16].name, "list_memories");
        assert_eq!(tools[17].name, "search_memories");
        assert_eq!(tools[18].name, "delete_memory");
        assert_eq!(tools[19].name, "report_bug");
        assert_eq!(tools[20].name, "youtube_info");
        assert_eq!(tools[21].name, "noop");
        assert_eq!(tools[22].name, "set_reminder");
        assert_eq!(tools[23].name, "list_reminders");
        assert_eq!(tools[24].name, "cancel_reminder");
        assert_eq!(tools[25].name, "add_trusted_user");
        assert_eq!(tools[26].name, "remove_trusted_user");
        assert_eq!(tools[27].name, "done");
    }
}
