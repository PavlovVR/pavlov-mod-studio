//! The people around a mod: its team, comments and recent activity.
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;

use crate::api::{Api, GAME_ID};

const MAX_COMMENT: usize = 10_000;
const RECENT_EVENTS: &str = "30";

fn mod_path(mod_id: u64) -> String {
    format!("/games/{GAME_ID}/mods/{mod_id}")
}

fn text_of(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

#[derive(Serialize)]
pub struct TeamMember {
    pub user_id: u64,
    pub username: String,
    pub avatar: Option<String>,
    pub profile_url: Option<String>,
    pub level: u64,
    pub position: String,
    pub pending: bool,
}

/// mod.io has no API for changing a team, so this is read-only. The website manages it.
pub fn team(api: &Api, mod_id: u64) -> Result<Vec<TeamMember>> {
    let path = format!("{}/team", mod_path(mod_id));
    let items = api.pages("list team", &path, &[])?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let user = &item["user"];
            Some(TeamMember {
                user_id: user["id"].as_u64()?,
                username: text_of(&user["username"]),
                avatar: user["avatar"]["thumb_50x50"].as_str().map(str::to_string),
                profile_url: user["profile_url"].as_str().map(str::to_string),
                level: item["level"].as_u64().unwrap_or(0),
                position: text_of(&item["position"]),
                pending: item["invite_pending"].as_i64() == Some(1),
            })
        })
        .collect())
}

#[derive(Serialize)]
pub struct Comment {
    pub id: u64,
    pub reply_id: u64,
    pub username: String,
    pub avatar: Option<String>,
    pub date_added: i64,
    pub karma: i64,
    pub content: String,
}

fn to_comment(item: &Value) -> Option<Comment> {
    let user = &item["user"];
    Some(Comment {
        id: item["id"].as_u64()?,
        reply_id: item["reply_id"].as_u64().unwrap_or(0),
        username: text_of(&user["username"]),
        avatar: user["avatar"]["thumb_50x50"].as_str().map(str::to_string),
        date_added: item["date_added"].as_i64().unwrap_or(0),
        karma: item["karma"].as_i64().unwrap_or(0),
        content: text_of(&item["content"]),
    })
}

pub fn comments(api: &Api, mod_id: u64) -> Result<Vec<Comment>> {
    let path = format!("{}/comments", mod_path(mod_id));
    let items = api.pages("list comments", &path, &[("_sort", "-id")])?;
    Ok(items.iter().filter_map(to_comment).collect())
}

fn checked(content: &str) -> Result<&str> {
    let content = content.trim();
    if content.is_empty() {
        bail!("A comment can't be empty");
    }
    if content.chars().count() > MAX_COMMENT {
        bail!("A comment can't be over 10,000 characters");
    }
    Ok(content)
}

/// Post a comment, or a reply when `reply_id` names one.
pub fn add_comment(api: &Api, mod_id: u64, content: &str, reply_id: Option<u64>) -> Result<()> {
    let content = checked(content)?.to_string();
    let path = format!("{}/comments", mod_path(mod_id));
    let mut fields = vec![("content", content)];
    if let Some(reply_id) = reply_id {
        fields.push(("reply_id", reply_id.to_string()));
    }
    api.send("post comment", &[], || {
        api.client().post(api.url(&path)).form(&fields)
    })?;
    Ok(())
}

pub fn edit_comment(api: &Api, mod_id: u64, comment_id: u64, content: &str) -> Result<()> {
    let content = checked(content)?;
    let path = format!("{}/comments/{comment_id}", mod_path(mod_id));
    api.send("edit comment", &[], || {
        api.client()
            .put(api.url(&path))
            .form(&[("content", content)])
    })?;
    Ok(())
}

pub fn delete_comment(api: &Api, mod_id: u64, comment_id: u64) -> Result<()> {
    let path = format!("{}/comments/{comment_id}", mod_path(mod_id));
    api.send("delete comment", &[], || {
        api.client().delete(api.url(&path))
    })?;
    Ok(())
}

#[derive(Serialize)]
pub struct ModEvent {
    pub id: u64,
    pub user_id: u64,
    pub date_added: i64,
    pub event_type: String,
}

/// The most recent changes to a mod, newest first.
pub fn events(api: &Api, mod_id: u64) -> Result<Vec<ModEvent>> {
    let path = format!("{}/events", mod_path(mod_id));
    let page = api.send("list activity", &[], || {
        api.client()
            .get(api.url(&path))
            .query(&[("_limit", RECENT_EVENTS), ("_sort", "-id")])
    })?;
    Ok(page["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(ModEvent {
                id: item["id"].as_u64()?,
                user_id: item["user_id"].as_u64().unwrap_or(0),
                date_added: item["date_added"].as_i64().unwrap_or(0),
                event_type: text_of(&item["event_type"]),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{checked, to_comment};

    #[test]
    fn reads_a_comment_as_mod_io_sends_it() {
        let comment = to_comment(&json!({
            "id": 1046632, "reply_id": 0, "karma": 2, "date_added": 1729359790,
            "content": "Hello", "user": { "username": "MasterHyp", "avatar": { "thumb_50x50": "a.png" } },
        }))
        .unwrap();
        assert_eq!(comment.username, "MasterHyp");
        assert_eq!(comment.karma, 2);
        assert_eq!(comment.avatar.as_deref(), Some("a.png"));
    }

    #[test]
    fn a_comment_from_a_deleted_user_still_reads() {
        let comment = to_comment(&json!({ "id": 5, "user": null })).unwrap();
        assert_eq!(comment.username, "");
    }

    #[test]
    fn blank_and_oversized_comments_are_refused() {
        assert!(checked("  ").is_err());
        assert!(checked(&"x".repeat(10_001)).is_err());
        assert_eq!(checked(" hi ").unwrap(), "hi");
    }
}
