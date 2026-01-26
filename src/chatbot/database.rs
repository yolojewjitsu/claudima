//! Persistent SQLite database for messages, members, and reminders.

use crate::chatbot::message::{ChatMessage, ReplyTo};
use crate::chatbot::reminders::Reminder;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;
use tracing::{info, warn, debug};

/// Member status in the group.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberStatus {
    Member,
    Left,
    Banned,
}

impl MemberStatus {
    fn from_str(s: &str) -> Self {
        match s {
            "left" => MemberStatus::Left,
            "banned" => MemberStatus::Banned,
            _ => MemberStatus::Member,
        }
    }
}

/// A group member.
#[derive(Debug, Clone)]
pub struct Member {
    pub user_id: i64,
    pub username: Option<String>,
    pub first_name: String,
    pub join_date: String,
    pub last_message_date: Option<String>,
    pub message_count: u32,
    pub status: MemberStatus,
}

/// Persistent SQLite database for the chatbot.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Create a new in-memory database.
    pub fn new() -> Self {
        let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
        let db = Self { conn: Mutex::new(conn) };
        db.init_schema();
        db
    }

    /// Load from file if it exists, otherwise create new.
    pub fn load_or_new(path: &Path) -> Self {
        // Check if we need to migrate from JSON
        let json_path = path.with_extension("json");
        let db_exists = path.exists();

        let conn = Connection::open(path).expect("Failed to open database");
        let db = Self { conn: Mutex::new(conn) };
        db.init_schema();

        // Migrate from JSON if database is new and JSON exists
        if !db_exists && json_path.exists() {
            info!("Migrating from JSON database: {:?}", json_path);
            if let Err(e) = db.migrate_from_json(&json_path) {
                warn!("Migration failed: {e}");
            }
        }

        let (msg_count, member_count) = db.get_counts();
        info!("Loaded database from {:?} ({} messages, {} members)", path, msg_count, member_count);

        db
    }

    fn init_schema(&self) {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(r#"
            CREATE TABLE IF NOT EXISTS messages (
                message_id INTEGER PRIMARY KEY,
                chat_id INTEGER NOT NULL,
                user_id INTEGER NOT NULL,
                username TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                text TEXT NOT NULL,
                reply_to_id INTEGER,
                reply_to_username TEXT,
                reply_to_text TEXT
            );

            CREATE TABLE IF NOT EXISTS users (
                user_id INTEGER PRIMARY KEY,
                username TEXT,
                first_name TEXT NOT NULL,
                join_date TEXT NOT NULL,
                last_message_date TEXT,
                message_count INTEGER DEFAULT 0,
                status TEXT DEFAULT 'member'
            );

            CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp);
            CREATE INDEX IF NOT EXISTS idx_messages_user_id ON messages(user_id);
            CREATE INDEX IF NOT EXISTS idx_messages_username ON messages(username);
            CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
            CREATE INDEX IF NOT EXISTS idx_users_status ON users(status);

            CREATE TABLE IF NOT EXISTS reminders (
                id INTEGER PRIMARY KEY,
                chat_id INTEGER NOT NULL,
                user_id INTEGER NOT NULL,
                message TEXT NOT NULL,
                trigger_at TEXT NOT NULL,
                repeat_cron TEXT,
                created_at TEXT NOT NULL,
                last_triggered_at TEXT,
                active INTEGER DEFAULT 1
            );
            CREATE INDEX IF NOT EXISTS idx_reminders_active ON reminders(trigger_at) WHERE active = 1;
        "#).expect("Failed to initialize database schema");
    }

    fn get_counts(&self) -> (usize, usize) {
        let conn = self.conn.lock().unwrap();
        let msg_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages", [], |row| row.get(0)
        ).unwrap_or(0);
        let member_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM users", [], |row| row.get(0)
        ).unwrap_or(0);
        (msg_count as usize, member_count as usize)
    }

    /// Migrate data from a JSON database file.
    fn migrate_from_json(&self, json_path: &Path) -> Result<(), String> {
        use serde::{Deserialize, Serialize};

        #[derive(Serialize, Deserialize)]
        struct JsonMember {
            user_id: i64,
            username: Option<String>,
            first_name: String,
            join_date: String,
            last_message_date: Option<String>,
            message_count: u32,
            #[serde(default = "default_status")]
            status: String,
        }

        fn default_status() -> String { "member".to_string() }

        #[derive(Serialize, Deserialize)]
        struct JsonReplyTo {
            message_id: i64,
            username: String,
            text: String,
        }

        #[derive(Serialize, Deserialize)]
        struct JsonMessage {
            message_id: i64,
            chat_id: i64,
            user_id: i64,
            username: String,
            timestamp: String,
            text: String,
            reply_to: Option<JsonReplyTo>,
        }

        #[derive(Serialize, Deserialize)]
        struct JsonDatabase {
            messages: Vec<JsonMessage>,
            #[serde(default)]
            members: Vec<JsonMember>,
        }

        let json = std::fs::read_to_string(json_path)
            .map_err(|e| format!("Failed to read JSON: {e}"))?;

        let data: JsonDatabase = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to parse JSON: {e}"))?;

        let conn = self.conn.lock().unwrap();

        // Import members
        for m in &data.members {
            conn.execute(
                "INSERT OR REPLACE INTO users (user_id, username, first_name, join_date, last_message_date, message_count, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![m.user_id, m.username, m.first_name, m.join_date, m.last_message_date, m.message_count, m.status]
            ).map_err(|e| format!("Failed to insert member: {e}"))?;
        }

        // Import messages
        for msg in &data.messages {
            let (reply_id, reply_user, reply_text) = match &msg.reply_to {
                Some(r) => (Some(r.message_id), Some(r.username.clone()), Some(r.text.clone())),
                None => (None, None, None),
            };

            conn.execute(
                "INSERT OR REPLACE INTO messages (message_id, chat_id, user_id, username, timestamp, text, reply_to_id, reply_to_username, reply_to_text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![msg.message_id, msg.chat_id, msg.user_id, msg.username, msg.timestamp, msg.text, reply_id, reply_user, reply_text]
            ).map_err(|e| format!("Failed to insert message: {e}"))?;
        }

        info!("Migrated {} messages and {} members from JSON", data.messages.len(), data.members.len());
        Ok(())
    }

    /// Save is a no-op for SQLite (auto-committed).
    pub fn save(&self) -> Result<(), String> {
        Ok(())
    }

    // ==================== MESSAGE METHODS ====================

    /// Add a message to the database.
    pub fn add_message(&mut self, msg: ChatMessage) {
        let conn = self.conn.lock().unwrap();

        // Insert or update user
        conn.execute(
            "INSERT INTO users (user_id, username, first_name, join_date, last_message_date, message_count, status)
             VALUES (?1, ?2, ?2, ?3, ?3, 1, 'member')
             ON CONFLICT(user_id) DO UPDATE SET
                username = COALESCE(?2, username),
                last_message_date = ?3,
                message_count = message_count + 1",
            params![msg.user_id, msg.username, msg.timestamp]
        ).unwrap_or_else(|e| {
            warn!("Failed to update user: {e}");
            0
        });

        // Insert message
        let (reply_id, reply_user, reply_text) = match &msg.reply_to {
            Some(r) => (Some(r.message_id), Some(r.username.clone()), Some(r.text.clone())),
            None => (None, None, None),
        };

        conn.execute(
            "INSERT OR REPLACE INTO messages (message_id, chat_id, user_id, username, timestamp, text, reply_to_id, reply_to_username, reply_to_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![msg.message_id, msg.chat_id, msg.user_id, msg.username, msg.timestamp, msg.text, reply_id, reply_user, reply_text]
        ).unwrap_or_else(|e| {
            warn!("Failed to insert message: {e}");
            0
        });
    }

    /// Total message count.
    #[cfg(test)]
    pub fn message_count(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    /// Get recent messages up to a token budget.
    pub fn get_recent_by_tokens(&self, max_tokens: usize) -> Vec<ChatMessage> {
        let chars_budget = max_tokens * 4;
        let conn = self.conn.lock().unwrap();

        // Get recent messages in reverse order
        let mut stmt = conn.prepare(
            "SELECT message_id, chat_id, user_id, username, timestamp, text, reply_to_id, reply_to_username, reply_to_text
             FROM messages ORDER BY timestamp DESC, message_id DESC"
        ).unwrap();

        let mut total_chars = 0;
        let mut result: Vec<ChatMessage> = Vec::new();

        let rows = stmt.query_map([], |row| {
            let reply_to = row.get::<_, Option<i64>>(6)?.map(|id| ReplyTo {
                message_id: id,
                username: row.get::<_, String>(7).unwrap_or_default(),
                text: row.get::<_, String>(8).unwrap_or_default(),
            });

            Ok(ChatMessage {
                message_id: row.get(0)?,
                chat_id: row.get(1)?,
                user_id: row.get(2)?,
                username: row.get(3)?,
                timestamp: row.get(4)?,
                text: row.get(5)?,
                reply_to,
                image: None,
                voice_transcription: None,
                documents: vec![],
            })
        }).unwrap();

        for msg in rows.flatten() {
            let msg_chars = msg.format().len();
            if total_chars + msg_chars > chars_budget && !result.is_empty() {
                break;
            }
            total_chars += msg_chars;
            result.push(msg);
        }

        result.reverse();
        result
    }

    /// Execute a raw SELECT query and return results as formatted strings.
    /// SECURITY: Only SELECT queries are allowed.
    pub fn query(&self, sql: &str) -> Result<String, String> {
        let sql_trimmed = sql.trim();

        // Validate it's a SELECT query
        if !sql_trimmed.to_uppercase().starts_with("SELECT") {
            return Err("Only SELECT queries are allowed".to_string());
        }

        // Block dangerous patterns
        let sql_upper = sql_trimmed.to_uppercase();
        for pattern in ["INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "ATTACH", "DETACH"] {
            if sql_upper.contains(pattern) {
                return Err(format!("Query contains forbidden keyword: {pattern}"));
            }
        }

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(sql_trimmed)
            .map_err(|e| format!("Query error: {e}"))?;

        let column_count = stmt.column_count();
        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        let mut rows = stmt.query([])
            .map_err(|e| format!("Query execution error: {e}"))?;

        let mut results: Vec<String> = Vec::new();
        let mut row_count = 0;
        const MAX_ROWS: usize = 100;

        while let Some(row) = rows.next().map_err(|e| format!("Row fetch error: {e}"))? {
            if row_count >= MAX_ROWS {
                results.push(format!("... (truncated, showing first {} rows)", MAX_ROWS));
                break;
            }

            let mut values: Vec<String> = Vec::new();
            for (i, col_name) in column_names.iter().enumerate().take(column_count) {
                let value: String = row.get::<_, rusqlite::types::Value>(i)
                    .map(|v| match v {
                        rusqlite::types::Value::Null => "NULL".to_string(),
                        rusqlite::types::Value::Integer(n) => n.to_string(),
                        rusqlite::types::Value::Real(f) => f.to_string(),
                        rusqlite::types::Value::Text(s) => {
                            // Use chars() to respect UTF-8 character boundaries
                            if s.chars().count() > 100 {
                                format!("{}...", s.chars().take(100).collect::<String>())
                            } else {
                                s
                            }
                        }
                        rusqlite::types::Value::Blob(b) => format!("<blob {} bytes>", b.len()),
                    })
                    .unwrap_or_else(|_| "?".to_string());
                values.push(format!("{}: {}", col_name, value));
            }
            results.push(values.join(" | "));
            row_count += 1;
        }

        if results.is_empty() {
            Ok("No results".to_string())
        } else {
            Ok(format!("{} row(s):\n{}", row_count, results.join("\n")))
        }
    }

    // ==================== MEMBER METHODS ====================

    /// Import members from a JSON array.
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

        let conn = self.conn.lock().unwrap();
        let timestamp = "imported";
        let mut count = 0;

        for m in imported {
            let first_name = m.first_name
                .or_else(|| m.username.clone())
                .unwrap_or_else(|| format!("User{}", m.user_id));

            let result = conn.execute(
                "INSERT OR IGNORE INTO users (user_id, username, first_name, join_date, status) VALUES (?1, ?2, ?3, ?4, 'member')",
                params![m.user_id, m.username, first_name, timestamp]
            );

            if let Ok(n) = result {
                count += n;
            }
        }

        if count > 0 {
            info!("📥 Imported {} new members", count);
        }

        Ok(count)
    }

    /// Record a member joining.
    pub fn member_joined(&mut self, user_id: i64, username: Option<String>, first_name: String, timestamp: String) {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT INTO users (user_id, username, first_name, join_date, status)
             VALUES (?1, ?2, ?3, ?4, 'member')
             ON CONFLICT(user_id) DO UPDATE SET
                username = ?2,
                first_name = ?3,
                status = 'member'",
            params![user_id, username, first_name, timestamp]
        ).unwrap_or_else(|e| {
            warn!("Failed to record member join: {e}");
            0
        });

        info!("👋 Member joined: {} ({})", first_name, user_id);
    }

    /// Record a member leaving.
    pub fn member_left(&mut self, user_id: i64) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET status = 'left' WHERE user_id = ?1",
            params![user_id]
        ).unwrap_or_else(|e| {
            warn!("Failed to record member left: {e}");
            0
        });
        debug!("👋 Member left: {}", user_id);
    }

    /// Record a member being banned.
    pub fn member_banned(&mut self, user_id: i64) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET status = 'banned' WHERE user_id = ?1",
            params![user_id]
        ).unwrap_or_else(|e| {
            warn!("Failed to record member banned: {e}");
            0
        });
        info!("🚫 Member banned: {}", user_id);
    }

    /// Find a user by username (case-insensitive partial match).
    pub fn find_user_by_username(&self, username: &str) -> Option<Member> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", username.to_lowercase());

        conn.query_row(
            "SELECT user_id, username, first_name, join_date, last_message_date, message_count, status
             FROM users WHERE LOWER(username) LIKE ?1 LIMIT 1",
            params![pattern],
            |row| Ok(Member {
                user_id: row.get(0)?,
                username: row.get(1)?,
                first_name: row.get(2)?,
                join_date: row.get(3)?,
                last_message_date: row.get(4)?,
                message_count: row.get::<_, i64>(5)? as u32,
                status: MemberStatus::from_str(&row.get::<_, String>(6)?),
            })
        ).ok()
    }

    /// Get members with optional filter.
    pub fn get_members(&self, filter: Option<&str>, days_inactive: Option<i64>, limit: usize) -> Vec<Member> {
        let conn = self.conn.lock().unwrap();
        let days = days_inactive.unwrap_or(30);
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
        let cutoff_str = cutoff.format("%Y-%m-%d %H:%M").to_string();

        let filter_str = filter.unwrap_or("all");
        let sql = match filter_str {
            "active" => "SELECT * FROM users WHERE status = 'member' AND last_message_date IS NOT NULL ORDER BY last_message_date ASC LIMIT ?1",
            "inactive" => "SELECT * FROM users WHERE status = 'member' AND (last_message_date IS NULL OR last_message_date < ?2) ORDER BY COALESCE(last_message_date, join_date) ASC LIMIT ?1",
            "never_posted" => "SELECT * FROM users WHERE status = 'member' AND last_message_date IS NULL ORDER BY join_date ASC LIMIT ?1",
            "left" => "SELECT * FROM users WHERE status = 'left' ORDER BY join_date ASC LIMIT ?1",
            "banned" => "SELECT * FROM users WHERE status = 'banned' ORDER BY join_date ASC LIMIT ?1",
            _ => "SELECT * FROM users ORDER BY COALESCE(last_message_date, join_date) ASC LIMIT ?1",
        };

        let mut stmt = conn.prepare(sql).unwrap();
        let limit_i64 = limit as i64;

        let mut results = Vec::new();
        let mut rows = if filter_str == "inactive" {
            stmt.query(params![limit_i64, cutoff_str]).unwrap()
        } else {
            stmt.query(params![limit_i64]).unwrap()
        };

        while let Ok(Some(row)) = rows.next() {
            if let Ok(member) = (|| -> rusqlite::Result<Member> {
                Ok(Member {
                    user_id: row.get(0)?,
                    username: row.get(1)?,
                    first_name: row.get(2)?,
                    join_date: row.get(3)?,
                    last_message_date: row.get(4)?,
                    message_count: row.get::<_, i64>(5)? as u32,
                    status: MemberStatus::from_str(&row.get::<_, String>(6)?),
                })
            })() {
                results.push(member);
            }
        }

        results
    }

    /// Get member count (active members only).
    pub fn member_count(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM users WHERE status = 'member'",
            [],
            |row| row.get::<_, i64>(0)
        ).unwrap_or(0) as usize
    }

    /// Get total members ever seen.
    pub fn total_members_seen(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    // ==================== REMINDER METHODS ====================

    /// Create a new reminder. Returns the reminder ID.
    pub fn create_reminder(
        &mut self,
        chat_id: i64,
        user_id: i64,
        message: &str,
        trigger_at: DateTime<Utc>,
        repeat_cron: Option<&str>,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let trigger_str = trigger_at.to_rfc3339();

        conn.execute(
            "INSERT INTO reminders (chat_id, user_id, message, trigger_at, repeat_cron, created_at, active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
            params![chat_id, user_id, message, trigger_str, repeat_cron, now]
        ).map_err(|e| format!("Failed to create reminder: {e}"))?;

        let id = conn.last_insert_rowid();
        info!("Created reminder #{} for chat {} at {}", id, chat_id, trigger_at);
        Ok(id)
    }

    /// List active reminders, optionally filtered by chat_id.
    pub fn list_reminders(&self, chat_id: Option<i64>) -> Vec<Reminder> {
        let conn = self.conn.lock().unwrap();

        let sql = match chat_id {
            Some(_) => "SELECT id, chat_id, user_id, message, trigger_at, repeat_cron, created_at, last_triggered_at, active
                        FROM reminders WHERE active = 1 AND chat_id = ?1 ORDER BY trigger_at ASC",
            None => "SELECT id, chat_id, user_id, message, trigger_at, repeat_cron, created_at, last_triggered_at, active
                     FROM reminders WHERE active = 1 ORDER BY trigger_at ASC",
        };

        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to prepare list_reminders query: {e}");
                return Vec::new();
            }
        };

        let rows = if let Some(cid) = chat_id {
            stmt.query(params![cid])
        } else {
            stmt.query([])
        };

        let mut results = Vec::new();
        if let Ok(mut rows) = rows {
            while let Ok(Some(row)) = rows.next() {
                if let Ok(reminder) = Self::row_to_reminder(row) {
                    results.push(reminder);
                }
            }
        }
        results
    }

    /// Cancel a reminder by ID. Returns true if found and cancelled.
    pub fn cancel_reminder(&mut self, reminder_id: i64) -> Result<bool, String> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "UPDATE reminders SET active = 0 WHERE id = ?1 AND active = 1",
            params![reminder_id]
        ).map_err(|e| format!("Failed to cancel reminder: {e}"))?;

        if rows > 0 {
            info!("Cancelled reminder #{}", reminder_id);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get all due reminders (trigger_at <= now AND active = 1).
    pub fn get_due_reminders(&self) -> Vec<Reminder> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let mut stmt = match conn.prepare(
            "SELECT id, chat_id, user_id, message, trigger_at, repeat_cron, created_at, last_triggered_at, active
             FROM reminders WHERE active = 1 AND trigger_at <= ?1 ORDER BY trigger_at ASC"
        ) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to prepare get_due_reminders query: {e}");
                return Vec::new();
            }
        };

        let mut results = Vec::new();
        if let Ok(mut rows) = stmt.query(params![now]) {
            while let Ok(Some(row)) = rows.next() {
                if let Ok(reminder) = Self::row_to_reminder(row) {
                    results.push(reminder);
                }
            }
        }
        results
    }

    /// Mark a one-time reminder as completed (active = 0).
    pub fn mark_reminder_completed(&mut self, reminder_id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE reminders SET active = 0, last_triggered_at = ?1 WHERE id = ?2",
            params![now, reminder_id]
        ).map_err(|e| format!("Failed to mark reminder completed: {e}"))?;
        debug!("Marked reminder #{} as completed", reminder_id);
        Ok(())
    }

    /// Reschedule a recurring reminder to its next trigger time.
    pub fn reschedule_reminder(&mut self, reminder_id: i64, next_trigger: DateTime<Utc>) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let trigger_str = next_trigger.to_rfc3339();
        conn.execute(
            "UPDATE reminders SET trigger_at = ?1, last_triggered_at = ?2 WHERE id = ?3",
            params![trigger_str, now, reminder_id]
        ).map_err(|e| format!("Failed to reschedule reminder: {e}"))?;
        debug!("Rescheduled reminder #{} to {}", reminder_id, next_trigger);
        Ok(())
    }

    /// Convert a database row to a Reminder struct.
    fn row_to_reminder(row: &rusqlite::Row) -> rusqlite::Result<Reminder> {
        let trigger_str: String = row.get(4)?;
        let created_str: String = row.get(6)?;
        let last_triggered_str: Option<String> = row.get(7)?;

        let trigger_at = DateTime::parse_from_rfc3339(&trigger_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let created_at = DateTime::parse_from_rfc3339(&created_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let last_triggered_at = last_triggered_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        });

        Ok(Reminder {
            id: row.get(0)?,
            chat_id: row.get(1)?,
            user_id: row.get(2)?,
            message: row.get(3)?,
            trigger_at,
            repeat_cron: row.get(5)?,
            created_at,
            last_triggered_at,
            active: row.get::<_, i64>(8)? == 1,
        })
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
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
            documents: vec![],
        }
    }

    #[test]
    fn test_add_message_creates_member() {
        let mut db = Database::new();
        db.add_message(make_msg(1, 100, "alice", "2024-01-15 10:00", "hello"));

        assert_eq!(db.message_count(), 1);
        assert!(db.find_user_by_username("alice").is_some());
    }

    #[test]
    fn test_query_basic() {
        let mut db = Database::new();
        db.add_message(make_msg(1, 100, "alice", "2024-01-15 10:00", "hello"));
        db.add_message(make_msg(2, 101, "bob", "2024-01-15 10:01", "world"));

        let result = db.query("SELECT COUNT(*) as count FROM messages").unwrap();
        assert!(result.contains("2"));
    }

    #[test]
    fn test_query_rejects_insert() {
        let db = Database::new();
        let result = db.query("INSERT INTO messages VALUES (1,2,3,'a','b','c',NULL,NULL,NULL)");
        assert!(result.is_err());
    }

    #[test]
    fn test_query_rejects_drop() {
        let db = Database::new();
        let result = db.query("SELECT * FROM messages; DROP TABLE messages");
        assert!(result.is_err());
    }

    #[test]
    fn test_member_status_changes() {
        let mut db = Database::new();
        db.member_joined(100, Some("testuser".to_string()), "Test".to_string(), "2024-01-15 10:00".to_string());

        let member = db.find_user_by_username("testuser").unwrap();
        assert_eq!(member.status, MemberStatus::Member);

        db.member_left(100);
        let member = db.find_user_by_username("testuser").unwrap();
        assert_eq!(member.status, MemberStatus::Left);

        db.member_joined(100, Some("testuser".to_string()), "Test".to_string(), "2024-01-16 10:00".to_string());
        let member = db.find_user_by_username("testuser").unwrap();
        assert_eq!(member.status, MemberStatus::Member);

        db.member_banned(100);
        let member = db.find_user_by_username("testuser").unwrap();
        assert_eq!(member.status, MemberStatus::Banned);
    }

    #[test]
    fn test_create_and_list_reminders() {
        let mut db = Database::new();
        let trigger = Utc::now() + chrono::Duration::hours(1);

        let id = db.create_reminder(-12345, 100, "Test reminder", trigger, None).unwrap();
        assert!(id > 0);

        let reminders = db.list_reminders(Some(-12345));
        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].message, "Test reminder");
        assert_eq!(reminders[0].chat_id, -12345);
    }

    #[test]
    fn test_cancel_reminder() {
        let mut db = Database::new();
        let trigger = Utc::now() + chrono::Duration::hours(1);

        let id = db.create_reminder(-12345, 100, "Test", trigger, None).unwrap();
        assert_eq!(db.list_reminders(None).len(), 1);

        let cancelled = db.cancel_reminder(id).unwrap();
        assert!(cancelled);
        assert_eq!(db.list_reminders(None).len(), 0);
    }

    #[test]
    fn test_due_reminders() {
        let mut db = Database::new();
        let past = Utc::now() - chrono::Duration::hours(1);
        let future = Utc::now() + chrono::Duration::hours(1);

        db.create_reminder(-12345, 100, "Past", past, None).unwrap();
        db.create_reminder(-12345, 100, "Future", future, None).unwrap();

        let due = db.get_due_reminders();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].message, "Past");
    }
}
