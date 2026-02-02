//! Peer-to-peer messaging between bot instances.
//!
//! Telegram bots cannot receive messages from other bots through the Bot API.
//! This module provides inter-bot communication through a shared filesystem.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// A message sent between peer bots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMessage {
    /// Original Telegram message ID
    pub message_id: i64,
    /// Chat where the message was sent
    pub chat_id: i64,
    /// Bot username that sent this message (without @)
    pub from_bot: String,
    /// Target bot username (without @)
    pub to_bot: String,
    /// The message text
    pub text: String,
    /// Timestamp when the message was created
    pub timestamp: String,
    /// Reply-to message ID if this is a reply
    pub reply_to_message_id: Option<i64>,
}

/// Get the shared directory for peer messages.
/// Uses the parent of data_dir (e.g., /home/dev/claudima/data/shared/)
pub fn shared_dir(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .unwrap_or(data_dir)
        .join("shared")
}

/// Write a peer message to the shared directory.
pub fn send_peer_message(
    data_dir: &Path,
    message: &PeerMessage,
) -> Result<(), String> {
    let dir = shared_dir(data_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create shared dir: {e}"))?;

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let filename = format!("{}_{}_to_{}.json", timestamp_ms, message.from_bot, message.to_bot);
    let path = dir.join(&filename);

    let json = serde_json::to_string_pretty(message)
        .map_err(|e| format!("Failed to serialize peer message: {e}"))?;

    std::fs::write(&path, json)
        .map_err(|e| format!("Failed to write peer message: {e}"))?;

    debug!("📨 Sent peer message to @{}: {}", message.to_bot, path.display());
    Ok(())
}

/// Read and consume peer messages for this bot.
/// Returns messages and deletes the files after reading.
pub fn receive_peer_messages(
    data_dir: &Path,
    my_username: &str,
) -> Vec<PeerMessage> {
    let dir = shared_dir(data_dir);
    if !dir.exists() {
        return vec![];
    }

    let my_username_lower = my_username.to_lowercase().trim_start_matches('@').to_string();
    let mut messages = vec![];

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("Failed to read shared dir: {e}");
            return vec![];
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        // Check if this message is for us: {timestamp}_{from}_to_{to}.json
        if !filename.ends_with(".json") {
            continue;
        }

        // Parse filename to check target
        let parts: Vec<&str> = filename.trim_end_matches(".json").split("_to_").collect();
        if parts.len() != 2 {
            continue;
        }

        let to_bot = parts[1].to_lowercase();
        if to_bot != my_username_lower {
            continue;
        }

        // Read and parse the message
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                match serde_json::from_str::<PeerMessage>(&content) {
                    Ok(msg) => {
                        info!("📬 Received peer message from @{}", msg.from_bot);
                        messages.push(msg);
                        // Delete after reading
                        if let Err(e) = std::fs::remove_file(&path) {
                            warn!("Failed to delete peer message file: {e}");
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse peer message {}: {e}", path.display());
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read peer message {}: {e}", path.display());
            }
        }
    }

    // Sort by timestamp
    messages.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    messages
}

/// Check if a message text mentions any of the peer bots.
/// Returns a list of mentioned bot usernames (without @).
pub fn find_mentioned_peers(text: &str, peer_bots: &[String]) -> Vec<String> {
    let text_lower = text.to_lowercase();
    peer_bots
        .iter()
        .filter(|bot| {
            // Check for @username mention
            let mention = format!("@{}", bot.to_lowercase());
            text_lower.contains(&mention)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_mentioned_peers() {
        let peers = vec!["clauscout_bot".to_string(), "clauoracle_bot".to_string()];

        let mentions = find_mentioned_peers("Hey @clauscout_bot check this", &peers);
        assert_eq!(mentions, vec!["clauscout_bot"]);

        let mentions = find_mentioned_peers("@clauscout_bot @clauoracle_bot discuss", &peers);
        assert_eq!(mentions.len(), 2);

        let mentions = find_mentioned_peers("No mentions here", &peers);
        assert!(mentions.is_empty());

        // Case insensitive
        let mentions = find_mentioned_peers("Hey @ClauScout_Bot", &peers);
        assert_eq!(mentions, vec!["clauscout_bot"]);
    }
}
