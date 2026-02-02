use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use teloxide::types::{ChatId, UserId};

/// Errors that can occur when loading configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// Failed to read the config file.
    ReadFile { path: PathBuf, source: std::io::Error },
    /// Failed to parse JSON.
    ParseJson { path: PathBuf, source: serde_json::Error },
    /// Invalid regex pattern.
    InvalidRegex { pattern: String, source: regex::Error },
    /// Validation error.
    Validation(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadFile { path, source } => {
                write!(f, "failed to read config file '{}': {}", path.display(), source)
            }
            Self::ParseJson { path, source } => {
                write!(f, "failed to parse config file '{}': {}", path.display(), source)
            }
            Self::InvalidRegex { pattern, source } => {
                write!(f, "invalid regex pattern '{}': {}", pattern, source)
            }
            Self::Validation(msg) => write!(f, "config validation error: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReadFile { source, .. } => Some(source),
            Self::ParseJson { source, .. } => Some(source),
            Self::InvalidRegex { source, .. } => Some(source),
            Self::Validation(_) => None,
        }
    }
}

#[derive(Deserialize)]
struct ConfigFile {
    owner_ids: Vec<u64>,
    /// Users who can DM the bot but don't have owner privileges
    #[serde(default)]
    trusted_dm_users: Vec<u64>,
    /// Usernames of peer bots that can communicate with this bot (e.g., ["clauscout_bot", "clauoracle_bot"])
    #[serde(default)]
    peer_bots: Vec<String>,
    telegram_bot_token: String,
    /// OpenRouter API key for spam classification
    #[serde(default)]
    openrouter_api_key: String,
    /// Gemini API key for image generation
    #[serde(default)]
    gemini_api_key: String,
    #[serde(default)]
    allowed_groups: Vec<i64>,
    /// Primary chat ID for the bot (if not set, uses first allowed_group)
    primary_chat_id: Option<i64>,
    #[serde(default)]
    trusted_channels: Vec<i64>,
    #[serde(default)]
    spam_patterns: Vec<String>,
    #[serde(default)]
    safe_patterns: Vec<String>,
    #[serde(default = "default_max_strikes")]
    max_strikes: u8,
    #[serde(default)]
    dry_run: bool,
    log_chat_id: Option<i64>,
    /// Directory for state files (logs, context). Defaults to current directory.
    data_dir: Option<String>,
    /// Path to Whisper model file (.bin) for voice transcription.
    whisper_model_path: Option<String>,
    /// TTS endpoint for Kokoro-FastAPI (e.g., "http://localhost:8880").
    tts_endpoint: Option<String>,
    /// Custom personality/identity override for the bot.
    /// If set, replaces the default "You are Claudima" description.
    personality: Option<String>,
    /// Interval in minutes for scheduled scans (0 = disabled).
    #[serde(default)]
    scan_interval_minutes: u32,
}

fn default_max_strikes() -> u8 {
    3
}

pub struct Config {
    /// Owner IDs - first ID is the primary owner (used for chatbot config).
    pub owner_ids: Vec<UserId>,
    /// Users who can DM the bot but don't have owner privileges.
    /// Key = user_id, Value = optional username (for display).
    /// This is the single source of truth, shared with ChatbotConfig.
    pub trusted_dm_users: Arc<RwLock<HashMap<i64, Option<String>>>>,
    /// Path to the config file (for saving changes)
    pub config_path: PathBuf,
    pub telegram_bot_token: String,
    pub openrouter_api_key: String,
    pub gemini_api_key: String,
    pub allowed_groups: HashSet<ChatId>,
    /// Primary chat ID (first allowed_group or explicit override)
    pub primary_chat_id: i64,
    pub trusted_channels: HashSet<ChatId>,
    pub spam_patterns: Vec<Regex>,
    pub safe_patterns: Vec<Regex>,
    pub max_strikes: u8,
    pub dry_run: bool,
    pub log_chat_id: Option<ChatId>,
    /// Directory for state files (logs, context).
    pub data_dir: PathBuf,
    /// Path to Whisper model file (.bin) for voice transcription.
    pub whisper_model_path: Option<PathBuf>,
    /// TTS endpoint for Kokoro-FastAPI (e.g., "http://localhost:8880").
    pub tts_endpoint: Option<String>,
    /// Custom personality/identity override for the bot.
    pub personality: Option<String>,
    /// Interval in minutes for scheduled scans (0 = disabled).
    pub scan_interval_minutes: u32,
    /// Usernames of peer bots (without @) that can communicate with this bot.
    pub peer_bots: Vec<String>,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let config_path = path.as_ref().to_path_buf();
        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| ConfigError::ReadFile { path: config_path.clone(), source: e })?;
        let file: ConfigFile = serde_json::from_str(&content)
            .map_err(|e| ConfigError::ParseJson { path: config_path.clone(), source: e })?;

        // Validate required fields
        if file.owner_ids.is_empty() {
            return Err(ConfigError::Validation("owner_ids must contain at least one owner ID".into()));
        }
        if file.telegram_bot_token.is_empty() {
            return Err(ConfigError::Validation("telegram_bot_token is required".into()));
        }
        // Telegram tokens are formatted as {bot_id}:{secret} where bot_id is numeric
        let token_parts: Vec<&str> = file.telegram_bot_token.split(':').collect();
        if token_parts.len() != 2 || token_parts[0].parse::<u64>().is_err() || token_parts[1].is_empty() {
            return Err(ConfigError::Validation(
                "telegram_bot_token appears invalid (expected format: 123456789:ABCdefGHI...)".into()
            ));
        }

        let owner_ids = file.owner_ids.into_iter().map(UserId).collect();
        // Initialize with None usernames - main.rs will fetch from Telegram
        let trusted_dm_users = Arc::new(RwLock::new(
            file.trusted_dm_users.into_iter()
                .map(|id| (id as i64, None))
                .collect()
        ));
        // Get primary_chat_id: explicit config value or first allowed_group
        let primary_chat_id = file.primary_chat_id
            .unwrap_or_else(|| file.allowed_groups.first().copied().unwrap_or(0));
        let allowed_groups = file.allowed_groups.into_iter().map(ChatId).collect();
        let trusted_channels = file.trusted_channels.into_iter().map(ChatId).collect();

        let spam_patterns = if file.spam_patterns.is_empty() {
            default_spam_patterns()
        } else {
            file.spam_patterns
                .into_iter()
                .map(|p| Regex::new(&p).map_err(|e| ConfigError::InvalidRegex { pattern: p, source: e }))
                .collect::<Result<Vec<_>, _>>()?
        };

        let safe_patterns = if file.safe_patterns.is_empty() {
            default_safe_patterns()
        } else {
            file.safe_patterns
                .into_iter()
                .map(|p| Regex::new(&p).map_err(|e| ConfigError::InvalidRegex { pattern: p, source: e }))
                .collect::<Result<Vec<_>, _>>()?
        };

        let data_dir = file
            .data_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));

        Ok(Self {
            owner_ids,
            trusted_dm_users,
            config_path,
            telegram_bot_token: file.telegram_bot_token,
            openrouter_api_key: file.openrouter_api_key,
            gemini_api_key: file.gemini_api_key,
            allowed_groups,
            primary_chat_id,
            trusted_channels,
            spam_patterns,
            safe_patterns,
            max_strikes: file.max_strikes,
            dry_run: file.dry_run,
            log_chat_id: file.log_chat_id.map(ChatId),
            data_dir,
            whisper_model_path: file.whisper_model_path.map(PathBuf::from),
            tts_endpoint: file.tts_endpoint,
            personality: file.personality,
            scan_interval_minutes: file.scan_interval_minutes,
            peer_bots: file.peer_bots.into_iter().map(|s| s.trim_start_matches('@').to_lowercase()).collect(),
        })
    }

    pub fn is_owner(&self, user_id: UserId) -> bool {
        self.owner_ids.contains(&user_id)
    }

    /// Check if user can DM the bot (owners + trusted DM users)
    pub fn can_dm(&self, user_id: UserId) -> bool {
        self.owner_ids.contains(&user_id)
            || self.trusted_dm_users.read()
                .expect("trusted_dm_users lock poisoned")
                .contains_key(&(user_id.0 as i64))
    }

    pub fn is_trusted_channel(&self, chat_id: ChatId) -> bool {
        self.trusted_channels.contains(&chat_id)
    }
}

fn default_spam_patterns() -> Vec<Regex> {
    vec![
        r"(?i)crypto.*profit",
        r"(?i)earn.*\$\d+.*day",
        r"(?i)click.*link.*bio",
        r"(?i)dm.*me.*for",
        r"(?i)investment.*opportunity",
        r"(?i)make.*money.*fast",
        r"(?i)forex.*trading",
        r"(?i)t\.me/\S+",
    ]
    .into_iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
}

fn default_safe_patterns() -> Vec<Regex> {
    vec![r"^[^a-zA-Z]*$", r"^\S{1,20}$", r"(?i)^(hi|hello|thanks)"]
        .into_iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_config(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file
    }

    fn assert_err<T>(result: Result<T, ConfigError>) -> ConfigError {
        match result {
            Ok(_) => panic!("expected error, got Ok"),
            Err(e) => e,
        }
    }

    #[test]
    fn test_valid_config() {
        let file = write_config(r#"{
            "owner_ids": [123456],
            "telegram_bot_token": "123456789:ABCdefGHIjklMNOpqrsTUVwxyz"
        }"#);
        let config = Config::load(file.path()).expect("should load valid config");
        assert_eq!(config.owner_ids.len(), 1);
        assert_eq!(config.owner_ids[0], UserId(123456));
    }

    #[test]
    fn test_empty_owner_ids() {
        let file = write_config(r#"{
            "owner_ids": [],
            "telegram_bot_token": "123456789:ABCdef"
        }"#);
        let err = assert_err(Config::load(file.path()));
        assert!(matches!(err, ConfigError::Validation(_)));
        assert!(err.to_string().contains("owner_ids"));
    }

    #[test]
    fn test_empty_token() {
        let file = write_config(r#"{
            "owner_ids": [123],
            "telegram_bot_token": ""
        }"#);
        let err = assert_err(Config::load(file.path()));
        assert!(matches!(err, ConfigError::Validation(_)));
        assert!(err.to_string().contains("telegram_bot_token"));
    }

    #[test]
    fn test_invalid_token_format_no_colon() {
        let file = write_config(r#"{
            "owner_ids": [123],
            "telegram_bot_token": "invalid_token_no_colon"
        }"#);
        let err = assert_err(Config::load(file.path()));
        assert!(matches!(err, ConfigError::Validation(_)));
        assert!(err.to_string().contains("invalid"));
    }

    #[test]
    fn test_invalid_token_format_non_numeric_id() {
        let file = write_config(r#"{
            "owner_ids": [123],
            "telegram_bot_token": "notanumber:ABCdef"
        }"#);
        let err = assert_err(Config::load(file.path()));
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn test_invalid_token_format_empty_secret() {
        let file = write_config(r#"{
            "owner_ids": [123],
            "telegram_bot_token": "123456789:"
        }"#);
        let err = assert_err(Config::load(file.path()));
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn test_invalid_regex_pattern() {
        let file = write_config(r#"{
            "owner_ids": [123],
            "telegram_bot_token": "123456789:ABCdef",
            "spam_patterns": ["[invalid(regex"]
        }"#);
        let err = assert_err(Config::load(file.path()));
        assert!(matches!(err, ConfigError::InvalidRegex { .. }));
    }

    #[test]
    fn test_file_not_found() {
        let err = assert_err(Config::load("/nonexistent/path/config.json"));
        assert!(matches!(err, ConfigError::ReadFile { .. }));
    }

    #[test]
    fn test_invalid_json() {
        let file = write_config("{ invalid json }");
        let err = assert_err(Config::load(file.path()));
        assert!(matches!(err, ConfigError::ParseJson { .. }));
    }
}
