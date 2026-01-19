mod chatbot;
mod classifier;
mod claude;
mod config;
mod prefilter;
mod telegram_log;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use teloxide::prelude::*;
use teloxide::types::ChatKind;
use tracing::{info, warn};
use tracing_subscriber::prelude::*;

use chatbot::{system_prompt, ChatMessage, ChatbotConfig, ChatbotEngine, ClaudeCode, ReplyTo, TelegramClient};
use classifier::{classify, Classification};
use claude::Client as ClaudeClient;
use config::Config;
use prefilter::{prefilter, PrefilterResult};

struct BotState {
    config: Config,
    claude: ClaudeClient,
    strikes: Mutex<HashMap<UserId, u8>>,
    chatbot: Option<ChatbotEngine>,
    dm_denied: Mutex<std::collections::HashSet<UserId>>,
}

impl BotState {
    async fn new(config: Config, bot: &Bot) -> Self {
        let claude = ClaudeClient::new(config.anthropic_api_key.clone());

        // Get bot info
        let (bot_user_id, bot_username) = match bot.get_me().await {
            Ok(me) => {
                info!("Bot user ID: {}, username: @{}", me.id, me.username());
                (me.id.0 as i64, Some(me.username().to_string()))
            }
            Err(e) => {
                warn!("Failed to get bot info: {e}");
                (0, None)
            }
        };

        // Create chatbot if enabled
        let chatbot = if !config.allowed_groups.is_empty() {
            let primary_chat_id = config.allowed_groups.iter().next().map(|id| id.0).unwrap_or(0);
            let owner_user_id = config.owner_ids.iter().next().map(|id| id.0 as i64);

            let chatbot_config = ChatbotConfig {
                primary_chat_id,
                bot_user_id,
                bot_username: bot_username.clone(),
                owner_user_id,
                debounce_ms: 1000,
                data_dir: Some(config.data_dir.clone()),
                gemini_api_key: if config.gemini_api_key.is_empty() { None } else { Some(config.gemini_api_key.clone()) },
            };

            // Start Claude Code with system prompt and session persistence
            let prompt = system_prompt(&chatbot_config);
            let session_file = Some(config.data_dir.join("session_id"));
            let claude_code = match ClaudeCode::start(prompt, session_file) {
                Ok(cc) => cc,
                Err(e) => {
                    panic!("Failed to start Claude Code: {}", e);
                }
            };

            let telegram = Arc::new(TelegramClient::new(bot.clone()));
            let mut engine = ChatbotEngine::new(chatbot_config, telegram, claude_code);
            engine.start_debouncer();
            engine.notify_owner("hey, just restarted").await;

            info!("Chatbot enabled (primary chat: {})", primary_chat_id);
            Some(engine)
        } else {
            info!("Chatbot disabled (no allowed_groups)");
            None
        };

        Self {
            config,
            claude,
            strikes: Mutex::new(HashMap::new()),
            chatbot,
            dm_denied: Mutex::new(std::collections::HashSet::new()),
        }
    }

    async fn add_strike(&self, user_id: UserId) -> u8 {
        let mut strikes = self.strikes.lock().await;
        let count = strikes.entry(user_id).or_insert(0);
        *count += 1;
        *count
    }
}

#[tokio::main]
async fn main() {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "claudir.json".to_string());
    let config = Config::load(&config_path);

    let bot = Bot::new(&config.telegram_bot_token);

    // Setup logging
    let log_dir = config.data_dir.join("logs");
    std::fs::create_dir_all(&log_dir).ok();
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("claudir.log"))
        .expect("Failed to open log file");
    let (non_blocking, _guard) = tracing_appender::non_blocking(log_file);

    let registry = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stdout)
                .with_filter(
                    tracing_subscriber::EnvFilter::from_default_env()
                        .add_directive(tracing::Level::INFO.into()),
                ),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_filter(
                    tracing_subscriber::EnvFilter::from_default_env()
                        .add_directive(tracing::Level::INFO.into()),
                ),
        );

    if let Some(log_chat_id) = config.log_chat_id {
        let tg_layer = telegram_log::TelegramLogLayer::new(bot.clone(), log_chat_id);
        registry.with(tg_layer).init();
    } else {
        registry.init();
    }

    info!("🚀 Starting claudir...");
    info!("Loaded config from {config_path}");
    info!("Owner IDs: {:?}", config.owner_ids);
    if config.dry_run {
        info!("DRY RUN mode enabled");
    }

    let state = Arc::new(BotState::new(config, &bot).await);

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(handle_new_message))
        .branch(Update::filter_edited_message().endpoint(handle_edited_message))
        .branch(Update::filter_chat_member().endpoint(handle_chat_member));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}

async fn handle_new_message(bot: Bot, msg: Message, state: Arc<BotState>) -> ResponseResult<()> {
    let is_group = matches!(msg.chat.kind, ChatKind::Public(_));
    let is_private = matches!(msg.chat.kind, ChatKind::Private(_));

    let user = match msg.from {
        Some(ref u) => u,
        None => return Ok(()),
    };

    let username = user.username.as_deref().unwrap_or(&user.first_name);

    // Handle DMs
    if is_private {
        if state.config.is_owner(user.id) {
            info!("📨 DM from {} ({})", username, user.id);
            if let Some(ref chatbot) = state.chatbot {
                let chat_msg = telegram_to_chat_message(&msg);
                chatbot.handle_message(chat_msg).await;
            }
            return Ok(());
        } else {
            let mut denied = state.dm_denied.lock().await;
            if !denied.contains(&user.id) {
                denied.insert(user.id);
                info!("DM from non-trusted user {} ({}) - denial", username, user.id);
                bot.send_message(msg.chat.id, "Access denied.").await.ok();
            }
            return Ok(());
        }
    }

    if !is_group {
        return Ok(());
    }

    // Check allowed group
    if !state.config.allowed_groups.is_empty()
        && !state.config.allowed_groups.contains(&msg.chat.id)
    {
        return Ok(());
    }

    let text = match msg.text() {
        Some(t) => t,
        None => return Ok(()),
    };

    // Pass to chatbot
    if let Some(ref chatbot) = state.chatbot {
        let chat_msg = telegram_to_chat_message(&msg);
        chatbot.handle_message(chat_msg).await;
    }

    // Skip spam filter for owners
    if state.config.is_owner(user.id) {
        info!("Skip spam filter for owner {username} ({})", user.id);
        return Ok(());
    }

    // Skip spam filter for trusted channels
    if let Some(ref sender_chat) = msg.sender_chat
        && state.config.is_trusted_channel(sender_chat.id)
    {
        info!("Skip spam filter for trusted channel {}", sender_chat.id);
        return Ok(());
    }

    // Spam filtering
    let prefilter_result = prefilter(text, &state.config);
    let text_preview: String = text.chars().take(100).collect();
    info!("Message from {username} ({}): \"{text_preview}\" → {:?}", user.id, prefilter_result);

    let is_spam = match prefilter_result {
        PrefilterResult::ObviousSpam => true,
        PrefilterResult::ObviousSafe => false,
        PrefilterResult::Ambiguous => {
            match classify(text, &state.claude).await {
                Ok(Classification::Spam) => {
                    info!("Haiku: spam");
                    true
                }
                Ok(Classification::NotSpam) => {
                    info!("Haiku: not spam");
                    false
                }
                Err(e) => {
                    warn!("Classification error: {e}");
                    false
                }
            }
        }
    };

    if is_spam {
        let dry = state.config.dry_run;

        if dry {
            info!("[DRY RUN] Would delete message {}", msg.id);
        } else if let Err(e) = bot.delete_message(msg.chat.id, msg.id).await {
            warn!("Failed to delete: {e}");
        }

        let strikes = state.add_strike(user.id).await;
        info!("{username} has {strikes} strike(s)");

        if strikes >= state.config.max_strikes {
            if dry {
                info!("[DRY RUN] Would ban {username}");
            } else {
                info!("Banning {username}");
                if let Err(e) = bot.ban_chat_member(msg.chat.id, user.id).await {
                    warn!("Failed to ban: {e}");
                }
            }
        }
    }

    Ok(())
}

fn telegram_to_chat_message(msg: &Message) -> ChatMessage {
    let user = msg.from.as_ref();
    let user_id = user.map(|u| u.id.0 as i64).unwrap_or(0);
    let username = user
        .and_then(|u| u.username.as_deref())
        .unwrap_or_else(|| user.map(|u| u.first_name.as_str()).unwrap_or("unknown"))
        .to_string();

    let timestamp = msg.date.format("%Y-%m-%d %H:%M").to_string();
    let text = msg.text().unwrap_or("").to_string();

    let reply_to = msg.reply_to_message().map(|reply| {
        let reply_user = reply.from.as_ref();
        let reply_username = reply_user
            .and_then(|u| u.username.as_deref())
            .unwrap_or_else(|| reply_user.map(|u| u.first_name.as_str()).unwrap_or("unknown"))
            .to_string();

        ReplyTo {
            message_id: reply.id.0 as i64,
            username: reply_username,
            text: reply.text().unwrap_or("").to_string(),
        }
    });

    ChatMessage {
        message_id: msg.id.0 as i64,
        chat_id: msg.chat.id.0,
        user_id,
        username,
        timestamp,
        text,
        reply_to,
    }
}

async fn handle_edited_message(msg: Message, state: Arc<BotState>) -> ResponseResult<()> {
    let is_group = matches!(msg.chat.kind, ChatKind::Public(_));
    if !is_group {
        return Ok(());
    }

    if !state.config.allowed_groups.is_empty()
        && !state.config.allowed_groups.contains(&msg.chat.id)
    {
        return Ok(());
    }

    let text = match msg.text() {
        Some(t) => t,
        None => return Ok(()),
    };

    if let Some(ref chatbot) = state.chatbot {
        chatbot.handle_edit(msg.id.0 as i64, text).await;
    }

    Ok(())
}

async fn handle_chat_member(update: teloxide::types::ChatMemberUpdated, state: Arc<BotState>) -> ResponseResult<()> {
    // Only track for allowed groups
    if !state.config.allowed_groups.is_empty()
        && !state.config.allowed_groups.contains(&update.chat.id)
    {
        return Ok(());
    }

    let Some(ref chatbot) = state.chatbot else {
        return Ok(());
    };

    let user = &update.new_chat_member.user;
    let user_id = user.id.0 as i64;
    let username = user.username.clone();
    let first_name = user.first_name.clone();

    use teloxide::types::ChatMemberStatus;
    match update.new_chat_member.status() {
        ChatMemberStatus::Member | ChatMemberStatus::Administrator | ChatMemberStatus::Owner => {
            // User joined or was added
            if matches!(update.old_chat_member.status(), ChatMemberStatus::Left | ChatMemberStatus::Banned) {
                info!("👋 Member joined: {} ({})", first_name, user_id);
                chatbot.handle_member_joined(user_id, username, first_name).await;
            }
        }
        ChatMemberStatus::Left => {
            info!("👋 Member left: {} ({})", first_name, user_id);
            chatbot.handle_member_left(user_id).await;
        }
        ChatMemberStatus::Banned => {
            info!("🚫 Member banned: {} ({})", first_name, user_id);
            chatbot.handle_member_banned(user_id).await;
        }
        _ => {}
    }

    Ok(())
}
