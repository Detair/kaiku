//! Bot Event Types & Gateway Intents
//!
//! Shared enum for both webhook subscriptions and gateway intent filtering.

use serde::{Deserialize, Serialize};

/// Bot event types matching the `webhook_event_type` `PostgreSQL` enum.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema,
)]
#[sqlx(type_name = "webhook_event_type", rename_all = "snake_case")]
pub enum BotEventType {
    /// A message was created in a guild channel.
    #[serde(rename = "message.created")]
    #[sqlx(rename = "message.created")]
    MessageCreated,
    /// A user joined a guild.
    #[serde(rename = "member.joined")]
    #[sqlx(rename = "member.joined")]
    MemberJoined,
    /// A user left a guild.
    #[serde(rename = "member.left")]
    #[sqlx(rename = "member.left")]
    MemberLeft,
    /// A slash command was invoked.
    #[serde(rename = "command.invoked")]
    #[sqlx(rename = "command.invoked")]
    CommandInvoked,
}

impl BotEventType {
    /// Parse from a string (e.g., `"message.created"`).
    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "message.created" => Some(Self::MessageCreated),
            "member.joined" => Some(Self::MemberJoined),
            "member.left" => Some(Self::MemberLeft),
            "command.invoked" => Some(Self::CommandInvoked),
            _ => None,
        }
    }

    /// Convert to the dot-separated string form.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::MessageCreated => "message.created",
            Self::MemberJoined => "member.joined",
            Self::MemberLeft => "member.left",
            Self::CommandInvoked => "command.invoked",
        }
    }
}

impl std::fmt::Display for BotEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Gateway intents for event filtering.
///
/// Each intent maps to a set of `BotEventType` values. Stored as string array
/// in `bot_applications.gateway_intents`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GatewayIntent {
    /// Receive `MessageCreated` events.
    Messages,
    /// Receive `MemberJoined` and `MemberLeft` events.
    Members,
    /// Receive `CommandInvoked` events (always enabled by default).
    Commands,
}

impl GatewayIntent {
    /// Parse from a string (e.g., `"messages"`).
    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "messages" => Some(Self::Messages),
            "members" => Some(Self::Members),
            "commands" => Some(Self::Commands),
            _ => None,
        }
    }

    /// Convert to string form.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::Members => "members",
            Self::Commands => "commands",
        }
    }

    /// Returns the event types covered by this intent.
    pub const fn event_types(&self) -> &'static [BotEventType] {
        match self {
            Self::Messages => &[BotEventType::MessageCreated],
            Self::Members => &[BotEventType::MemberJoined, BotEventType::MemberLeft],
            Self::Commands => &[BotEventType::CommandInvoked],
        }
    }

    /// All valid intent names.
    pub const ALL: &'static [&'static str] = &["messages", "members", "commands"];

    /// Check whether a set of intents permits receiving a given event type.
    pub fn intents_permit_event(intents: &[String], event: &BotEventType) -> bool {
        match event {
            BotEventType::MessageCreated => intents.iter().any(|i| i == "messages"),
            BotEventType::MemberJoined | BotEventType::MemberLeft => {
                intents.iter().any(|i| i == "members")
            }
            BotEventType::CommandInvoked => {
                // Commands are always permitted (default intent)
                true
            }
        }
    }
}
