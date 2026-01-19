use crate::claude::{Client, Message, Model, Role};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    Spam,
    NotSpam,
}

pub async fn classify(text: &str, client: &Client) -> Result<Classification, String> {
    let prompt = format!(
        r#"You are a spam classifier for a Telegram group. Analyze this message and respond with exactly one word: SPAM or NOT_SPAM.

Spam includes:
- Crypto/forex/investment scams
- Unsolicited promotions
- Phishing attempts
- Invite links to other groups/channels
- "Get rich quick" schemes
- Adult content promotion

NOT spam includes:
- Normal conversation
- Questions and answers
- Opinions and discussions
- Sharing relevant content

Message to classify:
"{text}"

Respond with exactly one word: SPAM or NOT_SPAM"#
    );

    let response = client
        .message(
            Model::Haiku,
            &[Message {
                role: Role::User,
                content: prompt,
            }],
            10,
        )
        .await
        .map_err(|e| e.to_string())?;

    let result = response.trim().to_uppercase();

    if result.contains("SPAM") && !result.contains("NOT") {
        Ok(Classification::Spam)
    } else {
        Ok(Classification::NotSpam)
    }
}
