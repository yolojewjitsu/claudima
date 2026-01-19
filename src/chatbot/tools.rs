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

    /// Get info about a user.
    GetUserInfo { user_id: i64 },

    /// Read messages from archive.
    ReadMessages {
        #[serde(skip_serializing_if = "Option::is_none")]
        last_n: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        from_date: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        to_date: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        username: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        limit: Option<i64>,
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

    /// Search the web for information.
    WebSearch {
        /// Search query
        query: String,
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

    /// Send a photo to a chat.
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

    /// Signal that processing is complete.
    Done,
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
            description: "Get information about a user by their ID".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "user_id": {
                        "type": "integer",
                        "description": "The user ID to look up"
                    }
                },
                "required": ["user_id"]
            }),
        },
        Tool {
            name: "read_messages".to_string(),
            description: "Read messages from the archive. Supports date ranges and username filtering.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "last_n": {
                        "type": "integer",
                        "description": "Get last N messages (ignores date filters)"
                    },
                    "from_date": {
                        "type": "string",
                        "description": "Messages after this date (e.g. '2024-01-15' or '2024-01-15 10:00')"
                    },
                    "to_date": {
                        "type": "string",
                        "description": "Messages before this date (e.g. '2024-01-20')"
                    },
                    "username": {
                        "type": "string",
                        "description": "Filter by username (case-insensitive partial match)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max messages to return (default 50)"
                    }
                }
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
            name: "web_search".to_string(),
            description: "Search the web for current information. Use this when you need to look up facts, news, or anything that might be outdated in your training data.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    }
                },
                "required": ["query"]
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
            description: "Import members from a JSON file (for backfilling from browser extension export). Only Nodir can use this.".to_string(),
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
        assert_eq!(tools.len(), 14);
        assert_eq!(tools[0].name, "send_message");
        assert_eq!(tools[1].name, "get_user_info");
        assert_eq!(tools[2].name, "read_messages");
        assert_eq!(tools[3].name, "add_reaction");
        assert_eq!(tools[4].name, "web_search");
        assert_eq!(tools[5].name, "delete_message");
        assert_eq!(tools[6].name, "mute_user");
        assert_eq!(tools[7].name, "ban_user");
        assert_eq!(tools[8].name, "kick_user");
        assert_eq!(tools[9].name, "get_chat_admins");
        assert_eq!(tools[10].name, "get_members");
        assert_eq!(tools[11].name, "import_members");
        assert_eq!(tools[12].name, "send_photo");
        assert_eq!(tools[13].name, "done");
    }
}
