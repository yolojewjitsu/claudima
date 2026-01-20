//! Persistent database for messages and members.

use crate::chatbot::message::ChatMessage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{info, warn};

/// How often to auto-save (every N changes).
const SAVE_INTERVAL: usize = 10;

/// Member status in the group.
/// Note: Telegram doesn't distinguish "kicked" from "left", so we only track Left/Banned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Member,
    Left,
    Banned,
}

/// A group member.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub user_id: i64,
    pub username: Option<String>,
    pub first_name: String,
    pub join_date: String,
    pub last_message_date: Option<String>,
    pub message_count: u32,
    pub status: MemberStatus,
}

/// Persistent database for the chatbot.
pub struct Database {
    /// All messages, indexed by message_id.
    messages: HashMap<i64, ChatMessage>,
    /// Messages in chronological order.
    message_order: Vec<i64>,
    /// All members, indexed by user_id.
    members: HashMap<i64, Member>,
    /// Path to the database file.
    path: Option<std::path::PathBuf>,
    /// Count of unsaved changes.
    unsaved_count: AtomicUsize,
}

impl Database {
    /// Create an empty database.
    pub fn new() -> Self {
        Self {
            messages: HashMap::new(),
            message_order: Vec::new(),
            members: HashMap::new(),
            path: None,
            unsaved_count: AtomicUsize::new(0),
        }
    }

    /// Create a database with a persistence path.
    pub fn with_path(path: std::path::PathBuf) -> Self {
        Self {
            messages: HashMap::new(),
            message_order: Vec::new(),
            members: HashMap::new(),
            path: Some(path),
            unsaved_count: AtomicUsize::new(0),
        }
    }

    /// Load from file if it exists, otherwise create empty.
    pub fn load_or_new(path: &Path) -> Self {
        if path.exists() {
            match Self::load(path) {
                Ok(db) => db,
                Err(e) => {
                    warn!("Failed to load database, starting fresh: {e}");
                    Self::with_path(path.to_path_buf())
                }
            }
        } else {
            info!("No existing database, starting fresh");
            Self::with_path(path.to_path_buf())
        }
    }

    /// Load from a JSON file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read database: {e}"))?;

        let state: DatabaseState = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to parse database: {e}"))?;

        let mut messages = HashMap::new();
        let mut message_order = Vec::new();

        for msg in state.messages {
            message_order.push(msg.message_id);
            messages.insert(msg.message_id, msg);
        }

        let members: HashMap<i64, Member> = state.members
            .into_iter()
            .map(|m| (m.user_id, m))
            .collect();

        info!(
            "Loaded database from {:?} ({} messages, {} members)",
            path,
            messages.len(),
            members.len()
        );

        Ok(Self {
            messages,
            message_order,
            members,
            path: Some(path.to_path_buf()),
            unsaved_count: AtomicUsize::new(0),
        })
    }

    /// Save to the configured path.
    pub fn save(&self) -> Result<(), String> {
        let Some(ref path) = self.path else {
            return Ok(());
        };

        let messages: Vec<ChatMessage> = self.message_order.iter()
            .filter_map(|id| self.messages.get(id).cloned())
            .collect();

        let members: Vec<Member> = self.members.values().cloned().collect();

        let state = DatabaseState { messages, members };

        let json = serde_json::to_string_pretty(&state)
            .map_err(|e| format!("Failed to serialize database: {e}"))?;

        std::fs::write(path, json)
            .map_err(|e| format!("Failed to write database: {e}"))?;

        self.unsaved_count.store(0, Ordering::Relaxed);
        info!("💾 Saved database ({} messages, {} members)", self.messages.len(), self.members.len());
        Ok(())
    }

    fn mark_dirty(&self) {
        let count = self.unsaved_count.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= SAVE_INTERVAL
            && let Err(e) = self.save()
        {
            warn!("Auto-save failed: {e}");
        }
    }

    // ==================== MESSAGE METHODS ====================

    /// Add a message to the database.
    pub fn add_message(&mut self, msg: ChatMessage) {
        // Update member's last message date
        if let Some(member) = self.members.get_mut(&msg.user_id) {
            member.last_message_date = Some(msg.timestamp.clone());
            member.message_count += 1;
            // Update username if changed
            if member.username.as_deref() != Some(&msg.username) {
                member.username = Some(msg.username.clone());
            }
        } else {
            // Auto-create member if not exists (saw them post)
            self.members.insert(msg.user_id, Member {
                user_id: msg.user_id,
                username: Some(msg.username.clone()),
                first_name: msg.username.clone(),
                join_date: msg.timestamp.clone(), // Best guess
                last_message_date: Some(msg.timestamp.clone()),
                message_count: 1,
                status: MemberStatus::Member,
            });
        }

        if !self.messages.contains_key(&msg.message_id) {
            self.message_order.push(msg.message_id);
        }
        self.messages.insert(msg.message_id, msg);
        self.mark_dirty();
    }

    /// Total message count.
    #[cfg(test)]
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Get recent messages up to a token budget.
    pub fn get_recent_by_tokens(&self, max_tokens: usize) -> Vec<ChatMessage> {
        let chars_budget = max_tokens * 4;
        let mut total_chars = 0;
        let mut result: Vec<ChatMessage> = Vec::new();

        for id in self.message_order.iter().rev() {
            if let Some(msg) = self.messages.get(id) {
                let msg_chars = msg.format().len();
                if total_chars + msg_chars > chars_budget && !result.is_empty() {
                    break;
                }
                total_chars += msg_chars;
                result.push(msg.clone());
            }
        }

        result.reverse();
        result
    }

    /// Read messages with filters.
    pub fn read_messages(
        &self,
        last_n: Option<i64>,
        from_date: Option<&str>,
        to_date: Option<&str>,
        username: Option<&str>,
        limit: Option<i64>,
    ) -> Vec<&ChatMessage> {
        let all_messages: Vec<&ChatMessage> = self.message_order.iter()
            .filter_map(|id| self.messages.get(id))
            .collect();

        let filtered: Vec<&ChatMessage> = if let Some(n) = last_n {
            let n = n as usize;
            let recent = if all_messages.len() > n {
                all_messages[all_messages.len() - n..].to_vec()
            } else {
                all_messages
            };

            if let Some(user) = username {
                let user_lower = user.to_lowercase();
                recent.into_iter()
                    .filter(|m| m.username.to_lowercase().contains(&user_lower))
                    .collect()
            } else {
                recent
            }
        } else {
            all_messages
                .into_iter()
                .filter(|m| {
                    let after_from = from_date
                        .map(|d| m.timestamp.as_str() >= d)
                        .unwrap_or(true);
                    let before_to = to_date
                        .map(|d| m.timestamp.as_str() <= d)
                        .unwrap_or(true);
                    let matches_user = username
                        .map(|u| m.username.to_lowercase().contains(&u.to_lowercase()))
                        .unwrap_or(true);

                    after_from && before_to && matches_user
                })
                .collect()
        };

        let limit = limit.unwrap_or(50) as usize;
        if filtered.len() > limit {
            filtered[filtered.len() - limit..].to_vec()
        } else {
            filtered
        }
    }

    // ==================== MEMBER METHODS ====================

    /// Import members from a JSON array (e.g., from browser extension export).
    /// Expected format: [{"user_id": 123, "username": "foo", "first_name": "Foo"}, ...]
    /// Returns count of imported members.
    pub fn import_members(&mut self, members_json: &str) -> Result<usize, String> {
        #[derive(serde::Deserialize)]
        struct ImportMember {
            #[serde(alias = "id")]
            user_id: i64,
            #[serde(default)]
            username: Option<String>,
            #[serde(default, alias = "name")]
            first_name: Option<String>,
        }

        let imported: Vec<ImportMember> = serde_json::from_str(members_json)
            .map_err(|e| format!("Failed to parse members JSON: {e}"))?;

        let timestamp = "imported".to_string();
        let mut count = 0;

        for m in imported {
            if let std::collections::hash_map::Entry::Vacant(e) = self.members.entry(m.user_id) {
                let first_name = m.first_name
                    .or_else(|| m.username.clone())
                    .unwrap_or_else(|| format!("User{}", m.user_id));

                e.insert(Member {
                    user_id: m.user_id,
                    username: m.username,
                    first_name,
                    join_date: timestamp.clone(),
                    last_message_date: None,
                    message_count: 0,
                    status: MemberStatus::Member,
                });
                count += 1;
            }
        }

        if count > 0 {
            info!("📥 Imported {} new members", count);
            self.mark_dirty();
        }

        Ok(count)
    }

    /// Record a member joining.
    pub fn member_joined(&mut self, user_id: i64, username: Option<String>, first_name: String, timestamp: String) {
        if let Some(member) = self.members.get_mut(&user_id) {
            // Re-joined
            member.status = MemberStatus::Member;
            member.username = username;
            member.first_name = first_name;
            info!("👋 Member rejoined: {} ({})", member.first_name, user_id);
        } else {
            // New member
            self.members.insert(user_id, Member {
                user_id,
                username,
                first_name: first_name.clone(),
                join_date: timestamp,
                last_message_date: None,
                message_count: 0,
                status: MemberStatus::Member,
            });
            info!("👋 New member: {} ({})", first_name, user_id);
        }
        self.mark_dirty();
    }

    /// Record a member leaving.
    pub fn member_left(&mut self, user_id: i64) {
        if let Some(member) = self.members.get_mut(&user_id) {
            member.status = MemberStatus::Left;
            info!("👋 Member left: {} ({})", member.first_name, user_id);
        }
        self.mark_dirty();
    }

    /// Record a member being banned.
    pub fn member_banned(&mut self, user_id: i64) {
        if let Some(member) = self.members.get_mut(&user_id) {
            member.status = MemberStatus::Banned;
            info!("🚫 Member banned: {} ({})", member.first_name, user_id);
        }
        self.mark_dirty();
    }

    /// Find a user by username (case-insensitive partial match).
    /// Returns the first match or None.
    pub fn find_user_by_username(&self, username: &str) -> Option<&Member> {
        let username_lower = username.to_lowercase();
        self.members.values().find(|m| {
            m.username.as_ref()
                .map(|u| u.to_lowercase().contains(&username_lower))
                .unwrap_or(false)
        })
    }

    /// Get members with optional filter.
    /// filter: "all", "active", "inactive", "never_posted", "left", "banned"
    pub fn get_members(&self, filter: Option<&str>, days_inactive: Option<i64>, limit: usize) -> Vec<&Member> {
        let days = days_inactive.unwrap_or(30);
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
        let cutoff_str = cutoff.format("%Y-%m-%d %H:%M").to_string();

        let mut members: Vec<&Member> = self.members.values()
            .filter(|m| {
                match filter.unwrap_or("all") {
                    "all" => true,
                    "active" => {
                        m.status == MemberStatus::Member && m.last_message_date.is_some()
                    }
                    "inactive" => {
                        m.status == MemberStatus::Member && match &m.last_message_date {
                            None => true,
                            Some(date) => date.as_str() < cutoff_str.as_str(),
                        }
                    }
                    "never_posted" => {
                        m.status == MemberStatus::Member && m.last_message_date.is_none()
                    }
                    "left" => m.status == MemberStatus::Left,
                    "banned" => m.status == MemberStatus::Banned,
                    _ => true,
                }
            })
            .collect();

        // Sort by last activity (never posted first, then oldest)
        members.sort_by(|a, b| {
            match (&a.last_message_date, &b.last_message_date) {
                (None, None) => a.join_date.cmp(&b.join_date),
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (Some(a_date), Some(b_date)) => a_date.cmp(b_date),
            }
        });

        members.into_iter().take(limit).collect()
    }

    /// Get member count (active members only).
    pub fn member_count(&self) -> usize {
        self.members.values().filter(|m| m.status == MemberStatus::Member).count()
    }

    /// Get total members ever seen.
    pub fn total_members_seen(&self) -> usize {
        self.members.len()
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable database state.
#[derive(Serialize, Deserialize)]
struct DatabaseState {
    messages: Vec<ChatMessage>,
    #[serde(default)]
    members: Vec<Member>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(id: i64, user_id: i64, username: &str, timestamp: &str, text: &str) -> ChatMessage {
        ChatMessage {
            message_id: id,
            chat_id: -12345,
            user_id,
            username: username.to_string(),
            timestamp: timestamp.to_string(),
            text: text.to_string(),
            reply_to: None,
            image: None,
            voice_transcription: None,
        }
    }

    #[test]
    fn test_add_message_creates_member() {
        let mut db = Database::new();
        db.add_message(make_msg(1, 100, "alice", "2024-01-15 10:00", "hello"));

        assert_eq!(db.message_count(), 1);
        assert!(db.members.get(&100).is_some());
        assert_eq!(db.members.get(&100).unwrap().message_count, 1);
    }

    #[test]
    fn test_member_joined() {
        let mut db = Database::new();
        db.member_joined(100, Some("alice".to_string()), "Alice".to_string(), "2024-01-15 10:00".to_string());

        let member = db.members.get(&100).unwrap();
        assert_eq!(member.first_name, "Alice");
        assert_eq!(member.status, MemberStatus::Member);
        assert!(member.last_message_date.is_none());
    }

    #[test]
    fn test_get_members_filters() {
        let mut db = Database::new();

        // Member who joined but never posted
        db.member_joined(1, Some("lurker".to_string()), "Lurker".to_string(), "2024-01-01 10:00".to_string());

        // Member who posted recently
        db.member_joined(2, Some("active".to_string()), "Active".to_string(), "2024-01-01 10:00".to_string());
        db.add_message(make_msg(1, 2, "active", "2026-01-15 10:00", "hi"));

        // Member who posted long ago
        db.member_joined(3, Some("old".to_string()), "Old".to_string(), "2024-01-01 10:00".to_string());
        db.add_message(make_msg(2, 3, "old", "2024-06-01 10:00", "ancient"));

        // Test "inactive" filter
        let inactive = db.get_members(Some("inactive"), Some(30), 10);
        assert_eq!(inactive.len(), 2);
        assert_eq!(inactive[0].user_id, 1); // Lurker first (never posted)
        assert_eq!(inactive[1].user_id, 3); // Old second

        // Test "never_posted" filter
        let never_posted = db.get_members(Some("never_posted"), None, 10);
        assert_eq!(never_posted.len(), 1);
        assert_eq!(never_posted[0].user_id, 1);

        // Test "active" filter
        let active = db.get_members(Some("active"), None, 10);
        assert_eq!(active.len(), 2); // Both who posted are "active" (have last_message_date)

        // Test "all" filter
        let all = db.get_members(Some("all"), None, 10);
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_member_status_changes() {
        let mut db = Database::new();
        db.member_joined(100, None, "Test".to_string(), "2024-01-15 10:00".to_string());

        assert_eq!(db.members.get(&100).unwrap().status, MemberStatus::Member);

        db.member_left(100);
        assert_eq!(db.members.get(&100).unwrap().status, MemberStatus::Left);

        db.member_joined(100, None, "Test".to_string(), "2024-01-16 10:00".to_string());
        assert_eq!(db.members.get(&100).unwrap().status, MemberStatus::Member);

        db.member_banned(100);
        assert_eq!(db.members.get(&100).unwrap().status, MemberStatus::Banned);
    }
}
