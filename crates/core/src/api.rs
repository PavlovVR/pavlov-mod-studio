use std::fs::{self, OpenOptions};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use reqwest::blocking::multipart::Form;
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::{Method, StatusCode};
use serde_json::Value;

use crate::limits::{self, Pass};
use crate::progress::check_cancel;

const MAX_RETRIES: u32 = 12;
const QUICK_RETRIES: u32 = 3;
/// mod.io's ref for a limit on one endpoint. Every other 429 covers them all.
const ENDPOINT_LIMIT: i64 = 11009;
pub const PAGE_SIZE: usize = 100;
/// Far beyond any real mod, so a bad total can't loop forever.
const MAX_ITEMS: usize = 10_000;

pub const GAME_ID: u64 = 3959;
pub const API_BASE: &str = "https://g-3959.modapi.io/v1";

/// Editing a mod must be multipart even with no file, or mod.io refuses it.
pub fn text_form<K: ToString, V: ToString>(fields: &[(K, V)]) -> Form {
    fields.iter().fold(Form::new(), |form, (key, value)| {
        form.text(key.to_string(), value.to_string())
    })
}

pub struct Api {
    base: String,
    token: String,
    client: Client,
    retries: u32,
}

impl Api {
    pub fn new(base: &str, token: &str) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(600))
            .build()?;
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
            client,
            retries: MAX_RETRIES,
        })
    }

    /// For someone waiting at the screen: fails in seconds instead of retrying for minutes.
    pub fn quick(base: &str, token: &str) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
            client,
            retries: QUICK_RETRIES,
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Download to a file, resuming from what's already there after a failure.
    pub fn download(&self, url: &str, path: &Path) -> Result<()> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(None)
            .build()?;
        let mut delay = 2;
        for attempt in 1..=MAX_RETRIES {
            let have = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            let mut request = client.get(url);
            if have > 0 {
                request = request.header("Range", format!("bytes={have}-"));
            }
            let reason = match request.send() {
                Err(e) => e.to_string(),
                // The file was already complete.
                Ok(r) if r.status() == StatusCode::RANGE_NOT_SATISFIABLE => return Ok(()),
                Ok(mut response) if response.status().is_success() => {
                    let resumed = response.status() == StatusCode::PARTIAL_CONTENT;
                    let mut file = OpenOptions::new()
                        .create(true)
                        .write(true)
                        .append(resumed)
                        .truncate(!resumed)
                        .open(path)?;
                    match response.copy_to(&mut file) {
                        Ok(_) => return Ok(()),
                        Err(e) => e.to_string(),
                    }
                }
                Ok(response) if response.status().is_client_error() => {
                    bail!("download failed: HTTP {}", response.status().as_u16())
                }
                Ok(response) => format!("HTTP {}", response.status().as_u16()),
            };
            if attempt == MAX_RETRIES {
                bail!("download failed after {attempt} tries: {reason}");
            }
            crate::say!(
                "  retry {attempt}/{} in {delay}s ({reason})",
                MAX_RETRIES - 1
            );
            sleep(Duration::from_secs(delay));
            delay = (delay * 2).min(120);
        }
        unreachable!("the loop always returns")
    }

    /// Hold a request until it's allowed. Someone at the screen hears about a wait from mod.io
    /// straight away, while a long job sits it out.
    fn admit(&self, write: bool, endpoint: &str) -> Result<()> {
        loop {
            let wait = match limits::ask(write, endpoint) {
                Pass::Go => return Ok(()),
                Pass::Refused(wait) if self.retries == QUICK_RETRIES => {
                    limits::announce(wait);
                    bail!("{}", limits::wait_message(wait));
                }
                Pass::Paced(wait) | Pass::Refused(wait) => wait,
            };
            limits::announce(wait);
            if self.retries != QUICK_RETRIES {
                check_cancel()?;
            }
            sleep(wait.min(Duration::from_secs(1)));
        }
    }

    /// Every item of a list, however many pages it takes. Only for lists that stay short,
    /// because each page is a request: use `page` for files and comments.
    pub fn pages(&self, label: &str, path: &str, query: &[(&str, &str)]) -> Result<Vec<Value>> {
        let limit = PAGE_SIZE.to_string();
        let mut items = Vec::new();
        loop {
            let offset = items.len().to_string();
            let page = self.send(label, &[], || {
                self.client()
                    .get(self.url(path))
                    .query(query)
                    .query(&[("_limit", limit.as_str()), ("_offset", offset.as_str())])
            })?;
            let data = page["data"].as_array().cloned().unwrap_or_default();
            let received = data.len();
            items.extend(data);
            let total = page["result_total"].as_u64().unwrap_or(0) as usize;
            if received == 0 || items.len() >= total || items.len() >= MAX_ITEMS {
                return Ok(items);
            }
        }
    }

    /// One page of a list, newest first. Returns the items, the total on mod.io, and where
    /// the next page starts if there is one.
    pub fn page(
        &self,
        label: &str,
        path: &str,
        offset: u64,
    ) -> Result<(Vec<Value>, u64, Option<u64>)> {
        let limit = PAGE_SIZE.to_string();
        let start = offset.to_string();
        let page = self.send(label, &[], || {
            self.client().get(self.url(path)).query(&[
                ("_sort", "-id"),
                ("_limit", limit.as_str()),
                ("_offset", start.as_str()),
            ])
        })?;
        let items = page["data"].as_array().cloned().unwrap_or_default();
        let reached = offset + items.len() as u64;
        let total = page["result_total"].as_u64().unwrap_or(reached);
        let next = (!items.is_empty() && reached < total).then_some(reached);
        Ok((items, total, next))
    }

    /// Send a request, retrying only failures that can clear on their own.
    /// `accepted` lists mod.io error refs that count as success.
    pub fn send(
        &self,
        label: &str,
        accepted: &[i64],
        build: impl Fn() -> RequestBuilder,
    ) -> Result<Value> {
        let mut delay = 2;
        for attempt in 1..=self.retries {
            let request = build()
                .bearer_auth(&self.token)
                .header("Accept", "application/json")
                .build()?;
            let write = request.method() != Method::GET;
            let endpoint = limits::endpoint(request.method().as_str(), request.url().path());
            self.admit(write, &endpoint)?;
            let mut limited = false;
            let reason = match self.client.execute(request) {
                Err(e) => e.to_string(),
                Ok(response) => {
                    let status = response.status();
                    let retry_after = response
                        .headers()
                        .get("Retry-After")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);
                    let text = response.text().unwrap_or_default();
                    if status.is_success() {
                        return Ok(serde_json::from_str(&text).unwrap_or(Value::Null));
                    }
                    let error_ref = serde_json::from_str::<Value>(&text)
                        .ok()
                        .and_then(|v| v["error"]["error_ref"].as_i64());
                    if let Some(found) = error_ref.filter(|r| accepted.contains(r)) {
                        return Ok(serde_json::json!({ "error_ref": found }));
                    }
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let only_here = error_ref == Some(ENDPOINT_LIMIT);
                        let wait =
                            limits::refused(retry_after, only_here.then_some(endpoint.as_str()));
                        limits::announce(wait);
                        if self.retries == QUICK_RETRIES {
                            bail!("{}", limits::wait_message(wait));
                        }
                        limited = true;
                    } else if status.is_client_error() {
                        bail!("{}", describe(label, status.as_u16(), &text));
                    }
                    format!("HTTP {}", status.as_u16())
                }
            };
            if attempt == self.retries {
                return Err(anyhow!("{label} failed after {attempt} tries: {reason}"));
            }
            // The next pass through `admit` already waits as long as mod.io said to.
            if limited {
                crate::say!("  mod.io asked us to wait ({reason})");
                continue;
            }
            crate::say!(
                "  retry {attempt}/{} in {delay}s ({reason})",
                self.retries - 1
            );
            sleep(Duration::from_secs(delay));
            delay = (delay * 2).min(120);
        }
        unreachable!("the loop always returns")
    }
}

const EXPIRED: &str = "Your mod.io token has expired. Make a new one in Settings.";

/// Turn a refused request into something a person can act on.
fn describe(label: &str, status: u16, body: &str) -> String {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let error = &parsed["error"];
    let known = match error["error_ref"].as_i64() {
        Some(11003) => {
            Some("Your mod.io token can't make changes. Make a new one with Read and Write ticked, using the steps in Settings.")
        }
        Some(11139) => Some("Your mod.io token doesn't cover this. Make a new one with All my mods and User actions ticked, using the steps in Settings."),
        Some(11005) => Some(EXPIRED),
        Some(11006) => Some("Your mod.io account is not active."),
        Some(15007 | 15008) => {
            Some("Only a team leader or manager of this mod can change its files.")
        }
        Some(15009) => Some("mod.io can't delete a live file. Make another file live first."),
        Some(15013) => Some("You are not on this mod's team, so you can't change it."),
        Some(15014) => Some("Only a team leader or manager can change the mature content flags."),
        Some(15015) => Some("Only a team leader or manager can change the mod's status."),
        Some(15016) => Some("This mod needs a file before it can go live."),
        Some(15019) => Some("Only a team leader or manager can delete this mod."),
        _ => None,
    };
    if let Some(message) = known {
        return message.to_string();
    }
    if status == 401 {
        return EXPIRED.to_string();
    }
    let mut parts = Vec::new();
    if let Some(message) = error["message"].as_str() {
        parts.push(message.to_string());
    }
    if let Some(fields) = error["errors"].as_object() {
        parts.extend(
            fields
                .values()
                .filter_map(|v| v.as_str().map(str::to_string)),
        );
    }
    if parts.is_empty() {
        let detail: String = body.chars().take(400).collect();
        return format!("{label}: HTTP {status} {detail}");
    }
    format!("{label}: {}", parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::describe;

    #[test]
    fn known_refusals_say_what_to_do() {
        let body = r#"{"error":{"code":403,"error_ref":15009,"message":"nope"}}"#;
        assert!(describe("delete file", 403, body).contains("Make another file live"));
    }

    #[test]
    fn an_expired_token_is_clear_without_a_ref() {
        assert!(describe("list files", 401, "").contains("Make a new one"));
    }

    #[test]
    fn field_errors_are_included() {
        let body = r#"{"error":{"code":422,"error_ref":13009,"message":"Invalid.","errors":{"name":"Too short."}}}"#;
        assert_eq!(
            describe("edit mod", 422, body),
            "edit mod: Invalid. Too short."
        );
    }

    #[test]
    fn an_unreadable_body_still_reports_the_status() {
        assert_eq!(describe("read mod", 404, "gone"), "read mod: HTTP 404 gone");
    }
}
