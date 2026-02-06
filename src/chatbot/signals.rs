//! Signal tracking system for multi-bot research coordination.
//!
//! Signals represent opportunities discovered through research that progress
//! through stages: DETECTED → RESEARCHING → VALIDATED → ACTIONABLE → BUILDING → SHIPPED

use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{debug, error, info, warn};

/// Status of a tracked signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalStatus {
    /// Just discovered, needs investigation
    Detected,
    /// Currently being researched
    Researching,
    /// Confirmed market/demand exists
    Validated,
    /// Clear path to MVP identified
    Actionable,
    /// Implementation in progress
    Building,
    /// Launched and tracking metrics
    Shipped,
    /// Dropped - not worth pursuing
    Dropped,
}

impl std::fmt::Display for SignalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignalStatus::Detected => write!(f, "DETECTED"),
            SignalStatus::Researching => write!(f, "RESEARCHING"),
            SignalStatus::Validated => write!(f, "VALIDATED"),
            SignalStatus::Actionable => write!(f, "ACTIONABLE"),
            SignalStatus::Building => write!(f, "BUILDING"),
            SignalStatus::Shipped => write!(f, "SHIPPED"),
            SignalStatus::Dropped => write!(f, "DROPPED"),
        }
    }
}

/// A tracked signal/opportunity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    /// Unique identifier
    pub id: String,
    /// Short title
    pub title: String,
    /// Current status
    pub status: SignalStatus,
    /// Detailed notes (markdown)
    pub notes: String,
    /// When first detected (ISO8601)
    pub detected_at: String,
    /// Last update time (ISO8601)
    pub updated_at: String,
    /// Tags/categories
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Scan mode determines what type of research to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanMode {
    /// Wide search for new signals
    Discover,
    /// Deep dive into a specific signal
    DeepDive,
    /// Validate market/demand for a signal
    Validate,
    /// Plan MVP implementation
    Plan,
    /// Check for updates on existing signals
    FollowUp,
}

impl std::fmt::Display for ScanMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanMode::Discover => write!(f, "DISCOVER"),
            ScanMode::DeepDive => write!(f, "DEEP_DIVE"),
            ScanMode::Validate => write!(f, "VALIDATE"),
            ScanMode::Plan => write!(f, "PLAN"),
            ScanMode::FollowUp => write!(f, "FOLLOW_UP"),
        }
    }
}

impl ScanMode {
    /// Get the next mode in the rotation.
    pub fn next(self) -> Self {
        match self {
            ScanMode::Discover => ScanMode::DeepDive,
            ScanMode::DeepDive => ScanMode::Validate,
            ScanMode::Validate => ScanMode::Plan,
            ScanMode::Plan => ScanMode::FollowUp,
            ScanMode::FollowUp => ScanMode::Discover,
        }
    }

    /// Get instructions for this scan mode.
    pub fn instructions(&self) -> &'static str {
        match self {
            ScanMode::Discover => {
                "🔍 DISCOVER MODE: Hunt for NEW signals. Look for:\n\
                 - Emerging trends in AI/crypto/tech\n\
                 - Underserved markets or niches\n\
                 - Problems people complain about that could be productized\n\
                 - New APIs/tools that enable new products\n\
                 Use WebSearch to find fresh opportunities. If you find something interesting, add it as a signal."
            }
            ScanMode::DeepDive => {
                "🔬 DEEP DIVE MODE: Research an existing signal in depth. Pick one RESEARCHING signal and:\n\
                 - Find competitors and analyze their weaknesses\n\
                 - Estimate market size\n\
                 - Identify key features for MVP\n\
                 - Find potential customers/communities\n\
                 Update the signal notes with your findings."
            }
            ScanMode::Validate => {
                "✅ VALIDATE MODE: Confirm market demand for a signal. Pick one RESEARCHING signal and:\n\
                 - Search for people asking for this solution\n\
                 - Check Reddit/Twitter/HN for complaints about the problem\n\
                 - Look for failed attempts (why did they fail?)\n\
                 - Estimate willingness to pay\n\
                 If validated, update status to VALIDATED. If not promising, mark as DROPPED."
            }
            ScanMode::Plan => {
                "📋 PLAN MODE: Create MVP plan for a VALIDATED signal. Pick one and:\n\
                 - Define minimal feature set (what's the core value?)\n\
                 - Identify tech stack (fast to build, not perfect)\n\
                 - Estimate effort (weekend project? week? month?)\n\
                 - Define monetization (how will it make money?)\n\
                 If plan is clear, update status to ACTIONABLE with implementation notes."
            }
            ScanMode::FollowUp => {
                "🔄 FOLLOW UP MODE: Check for updates on tracked signals:\n\
                 - Any news about competitors?\n\
                 - Market changes?\n\
                 - New opportunities related to existing signals?\n\
                 - Any BUILDING signals need status update?\n\
                 Update notes with new findings."
            }
        }
    }
}

/// Signals store - manages the shared signals file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SignalsStore {
    pub signals: Vec<Signal>,
    /// Current scan mode (rotates each scan)
    pub current_mode: Option<ScanMode>,
    /// Focus topics for discovery (rotate through these)
    #[serde(default)]
    pub focus_topics: Vec<String>,
    /// Current focus index
    #[serde(default)]
    pub focus_index: usize,
}

impl SignalsStore {
    /// Load signals from shared directory.
    pub fn load(data_dir: &Path) -> Self {
        let shared_dir = data_dir.parent().unwrap_or(data_dir).join("shared");
        let signals_file = shared_dir.join("signals.json");

        if signals_file.exists() {
            match std::fs::read_to_string(&signals_file) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(store) => {
                        debug!("Loaded signals from {:?}", signals_file);
                        return store;
                    }
                    Err(e) => {
                        warn!("Failed to parse signals.json: {}", e);
                    }
                },
                Err(e) => {
                    warn!("Failed to read signals.json: {}", e);
                }
            }
        }

        // Return default with some initial focus topics
        Self {
            signals: vec![],
            current_mode: Some(ScanMode::Discover),
            focus_topics: vec![
                "AI agents and automation".to_string(),
                "Developer tools and APIs".to_string(),
                "Crypto/DeFi opportunities".to_string(),
                "SaaS micro-products".to_string(),
                "Content and media tools".to_string(),
            ],
            focus_index: 0,
        }
    }

    /// Save signals to shared directory.
    pub fn save(&self, data_dir: &Path) -> Result<(), std::io::Error> {
        let shared_dir = data_dir.parent().unwrap_or(data_dir).join("shared");
        std::fs::create_dir_all(&shared_dir)?;
        let signals_file = shared_dir.join("signals.json");

        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&signals_file, content)?;
        info!("Saved signals to {:?}", signals_file);
        Ok(())
    }

    /// Get current focus topic and advance to next.
    pub fn get_and_advance_focus(&mut self) -> Option<String> {
        if self.focus_topics.is_empty() {
            return None;
        }
        let topic = self.focus_topics[self.focus_index].clone();
        self.focus_index = (self.focus_index + 1) % self.focus_topics.len();
        Some(topic)
    }

    /// Get current scan mode and advance to next.
    pub fn get_and_advance_mode(&mut self) -> ScanMode {
        let mode = self.current_mode.unwrap_or(ScanMode::Discover);
        self.current_mode = Some(mode.next());
        mode
    }

    /// Add a new signal.
    pub fn add_signal(&mut self, title: String, notes: String, tags: Vec<String>) -> String {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let id = format!("sig_{}", chrono::Utc::now().timestamp_millis());

        let signal = Signal {
            id: id.clone(),
            title,
            status: SignalStatus::Detected,
            notes,
            detected_at: now.clone(),
            updated_at: now,
            tags,
        };

        self.signals.push(signal);
        info!("Added new signal: {}", id);
        id
    }

    /// Update a signal's status.
    pub fn update_status(&mut self, id: &str, status: SignalStatus) -> bool {
        if let Some(signal) = self.signals.iter_mut().find(|s| s.id == id) {
            signal.status = status;
            signal.updated_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            info!("Updated signal {} status to {}", id, status);
            true
        } else {
            warn!("Signal {} not found", id);
            false
        }
    }

    /// Update a signal's notes.
    pub fn update_notes(&mut self, id: &str, notes: String) -> bool {
        if let Some(signal) = self.signals.iter_mut().find(|s| s.id == id) {
            signal.notes = notes;
            signal.updated_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            info!("Updated signal {} notes", id);
            true
        } else {
            warn!("Signal {} not found", id);
            false
        }
    }

    /// Get signals by status.
    pub fn by_status(&self, status: SignalStatus) -> Vec<&Signal> {
        self.signals.iter().filter(|s| s.status == status).collect()
    }

    /// Get active signals (not dropped/shipped).
    pub fn active(&self) -> Vec<&Signal> {
        self.signals
            .iter()
            .filter(|s| !matches!(s.status, SignalStatus::Dropped | SignalStatus::Shipped))
            .collect()
    }

    /// Format signals for inclusion in scan message.
    pub fn format_for_prompt(&self) -> String {
        let active = self.active();
        if active.is_empty() {
            return "No signals being tracked yet. Start by discovering new opportunities!".to_string();
        }

        let mut result = String::from("## Currently Tracked Signals\n\n");

        for signal in active {
            result.push_str(&format!(
                "### {} [{}]\n**ID:** {}\n**Tags:** {}\n**Notes:** {}\n\n",
                signal.title,
                signal.status,
                signal.id,
                if signal.tags.is_empty() {
                    "none".to_string()
                } else {
                    signal.tags.join(", ")
                },
                signal.notes.lines().take(3).collect::<Vec<_>>().join(" ")
            ));
        }

        result
    }
}

/// Generate the scan message with mode rotation and signal context.
pub fn generate_scan_message(data_dir: &Path) -> String {
    let mut store = SignalsStore::load(data_dir);

    let mode = store.get_and_advance_mode();
    let focus = store.get_and_advance_focus();
    let signals_context = store.format_for_prompt();

    // Save updated state (mode/focus rotation)
    if let Err(e) = store.save(data_dir) {
        error!("Failed to save signals state: {}", e);
    }

    let focus_line = match (mode, focus) {
        (ScanMode::Discover, Some(topic)) => format!("\n🎯 **Focus topic this scan:** {}\n", topic),
        _ => String::new(),
    };

    format!(
        "[SCAN] Scheduled research scan.\n\n\
         ## Current Mode: {}\n\n\
         {}\n\
         {}\n\n\
         ---\n\n\
         {}\n\n\
         ---\n\n\
         **Tools available:**\n\
         - `add_signal(title, notes, tags)` - Track a new opportunity\n\
         - `update_signal(id, status, notes)` - Update signal status/notes\n\
         - `list_signals()` - See all tracked signals\n\
         - WebSearch - Research the web\n\n\
         Share your findings with @peer_bot after researching.",
        mode,
        mode.instructions(),
        focus_line,
        signals_context
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_scan_mode_rotation() {
        let mut mode = ScanMode::Discover;
        assert_eq!(mode.next(), ScanMode::DeepDive);
        mode = mode.next();
        assert_eq!(mode.next(), ScanMode::Validate);
    }

    #[test]
    fn test_signal_store_save_load() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path().join("bot");
        std::fs::create_dir_all(&data_dir).unwrap();

        let mut store = SignalsStore::default();
        store.add_signal(
            "Test Signal".to_string(),
            "Some notes".to_string(),
            vec!["ai".to_string()],
        );

        store.save(&data_dir).unwrap();

        let loaded = SignalsStore::load(&data_dir);
        assert_eq!(loaded.signals.len(), 1);
        assert_eq!(loaded.signals[0].title, "Test Signal");
    }
}
