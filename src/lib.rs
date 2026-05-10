#![cfg_attr(not(test), no_main)]

use std::fmt::Display;

use extism_pdk::{http, *};
use serde::{Deserialize, Serialize};
use wasm_forge_pdk::forge_types;

const HN_API_BASE: &str = "https://hacker-news.firebaseio.com/v0";
const DEFAULT_STORY_LIMIT: u32 = 10;
const MAX_STORY_LIMIT: u32 = 25;
const DEFAULT_SUBMITTED_LIMIT: u32 = 20;
const MAX_SUBMITTED_LIMIT: u32 = 50;
const PREVIEW_LIMIT: usize = 10;
const HTTP_RETRY_ATTEMPTS: usize = 3;

forge_types! {
    pub struct NoArgs {}

    pub struct StoryListArgs {
        /// Maximum number of stories to hydrate and return. Defaults to 10 and is clamped to 25.
        pub limit: Option<u32>,
    }

    pub struct ItemArgs {
        /// Hacker News item ID.
        pub id: u64,
    }

    pub struct UserArgs {
        /// Hacker News user ID.
        pub id: String,
        /// Maximum number of submitted item IDs to include. Defaults to 20 and is clamped to 50.
        pub submitted_limit: Option<u32>,
    }
}

#[derive(Debug, Deserialize)]
struct HnItem {
    id: u64,
    #[serde(rename = "type")]
    item_type: String,
    by: Option<String>,
    time: Option<u64>,
    text: Option<String>,
    deleted: Option<bool>,
    dead: Option<bool>,
    parent: Option<u64>,
    poll: Option<u64>,
    kids: Option<Vec<u64>>,
    url: Option<String>,
    score: Option<i64>,
    title: Option<String>,
    parts: Option<Vec<u64>>,
    descendants: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct HnUser {
    id: String,
    created: u64,
    karma: i64,
    about: Option<String>,
    submitted: Option<Vec<u64>>,
}

#[derive(Debug, Deserialize)]
struct HnUpdates {
    items: Vec<u64>,
    profiles: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StoryListOutput {
    endpoint: String,
    total_ids: usize,
    returned: usize,
    ids: Vec<u64>,
    items: Vec<ListItemSummary>,
}

#[derive(Debug, Serialize)]
struct ListItemSummary {
    id: u64,
    #[serde(rename = "type")]
    item_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    comments: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ItemOutput {
    id: u64,
    #[serde(rename = "type")]
    item_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    descendants: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deleted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dead: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    poll: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kids_count: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    kids_preview: Vec<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parts_count: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    parts_preview: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct UserOutput {
    id: String,
    created: u64,
    karma: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    about: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    submitted_count: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    submitted_preview: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct UpdatesOutput {
    changed_items_count: usize,
    changed_profile_count: usize,
    items: Vec<u64>,
    profiles: Vec<String>,
}

fn build_url(path: &str) -> String {
    format!("{HN_API_BASE}/{path}.json")
}

fn clamp_limit(value: Option<u32>, default: u32, max: u32) -> usize {
    value.unwrap_or(default).clamp(1, max) as usize
}

fn preview<T: Clone>(values: &[T]) -> Vec<T> {
    values.iter().take(PREVIEW_LIMIT).cloned().collect()
}

fn yaml_string<T: Serialize>(value: &T) -> FnResult<String> {
    Ok(serde_yaml::to_string(value)?.trim_end().to_string())
}

fn retry<T, F>(description: &str, mut operation: F) -> Result<T, Error>
where
    F: FnMut() -> Result<T, Error>,
{
    let mut last_error = None;

    for attempt in 1..=HTTP_RETRY_ATTEMPTS {
        match operation() {
            Ok(value) => return Ok(value),
            Err(err) => {
                last_error = Some(err.to_string());

                if attempt == HTTP_RETRY_ATTEMPTS {
                    break;
                }
            }
        }
    }

    let last_error = last_error.unwrap_or_else(|| "unknown error".to_string());
    Err(Error::msg(format!(
        "{description} failed after {HTTP_RETRY_ATTEMPTS} attempts: {last_error}"
    )))
}

fn ensure_success<T>(path: &str, response: &http::HttpResponse) -> Result<T, Error>
where
    T: for<'de> Deserialize<'de>,
{
    if response.status_code() != 200 {
        let body = String::from_utf8_lossy(&response.body()).to_string();
        return Err(Error::msg(format!(
            "Hacker News API request failed for {} with status {}: {}",
            path,
            response.status_code(),
            body
        )));
    }

    response.json::<T>().map_err(|err| {
        Error::msg(format!(
            "Failed to parse Hacker News API response for {path}: {err}"
        ))
    })
}

fn get_json<T>(path: &str) -> Result<T, Error>
where
    T: for<'de> Deserialize<'de>,
{
    retry(&format!("Hacker News API request for {path}"), || {
        let request = HttpRequest::new(build_url(path))
            .with_method("GET")
            .with_header("accept", "application/json");
        let response = http::request::<()>(&request, None)?;
        ensure_success(path, &response)
    })
}

fn format_list_item(item: HnItem) -> ListItemSummary {
    ListItemSummary {
        id: item.id,
        item_type: item.item_type,
        title: item.title,
        by: item.by,
        score: item.score,
        comments: item.descendants,
        url: item.url.filter(|value| !value.is_empty()),
        text: item.text.filter(|value| !value.is_empty()),
        time: item.time,
    }
}

fn format_item(item: HnItem) -> ItemOutput {
    let kids = item.kids.unwrap_or_default();
    let parts = item.parts.unwrap_or_default();

    ItemOutput {
        id: item.id,
        item_type: item.item_type,
        by: item.by,
        time: item.time,
        title: item.title,
        text: item.text.filter(|value| !value.is_empty()),
        url: item.url.filter(|value| !value.is_empty()),
        score: item.score,
        descendants: item.descendants,
        deleted: item.deleted.filter(|value| *value),
        dead: item.dead.filter(|value| *value),
        parent: item.parent,
        poll: item.poll,
        kids_count: (!kids.is_empty()).then_some(kids.len()),
        kids_preview: preview(&kids),
        parts_count: (!parts.is_empty()).then_some(parts.len()),
        parts_preview: preview(&parts),
    }
}

fn format_user(user: HnUser, submitted_limit: usize) -> UserOutput {
    let submitted = user.submitted.unwrap_or_default();

    UserOutput {
        id: user.id,
        created: user.created,
        karma: user.karma,
        about: user.about.filter(|value| !value.is_empty()),
        submitted_count: (!submitted.is_empty()).then_some(submitted.len()),
        submitted_preview: submitted.into_iter().take(submitted_limit).collect(),
    }
}

fn fetch_story_list(endpoint: &str, limit: usize) -> Result<StoryListOutput, Error> {
    let ids: Vec<u64> = get_json(endpoint)?;
    let hydrated_ids: Vec<u64> = ids.iter().copied().take(limit).collect();
    let items = hydrated_ids
        .iter()
        .map(|id| get_json::<HnItem>(&format!("item/{id}")).map(format_list_item))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(StoryListOutput {
        endpoint: endpoint.to_string(),
        total_ids: ids.len(),
        returned: items.len(),
        ids: hydrated_ids,
        items,
    })
}

fn display_scalar<T>(key: &str, value: T) -> FnResult<String>
where
    T: Display,
{
    Ok(format!("{key}: {value}"))
}

/// Returns the current highest Hacker News item ID.
#[plugin_fn]
pub fn get_max_item(Json(_args): Json<NoArgs>) -> FnResult<String> {
    let max_item: u64 = get_json("maxitem")?;
    display_scalar("max_item", max_item)
}

/// Returns compact YAML for the current top Hacker News stories.
#[plugin_fn]
pub fn get_top_stories(Json(args): Json<StoryListArgs>) -> FnResult<String> {
    let output = fetch_story_list(
        "topstories",
        clamp_limit(args.limit, DEFAULT_STORY_LIMIT, MAX_STORY_LIMIT),
    )?;
    yaml_string(&output)
}

/// Returns compact YAML for the newest Hacker News stories.
#[plugin_fn]
pub fn get_new_stories(Json(args): Json<StoryListArgs>) -> FnResult<String> {
    let output = fetch_story_list(
        "newstories",
        clamp_limit(args.limit, DEFAULT_STORY_LIMIT, MAX_STORY_LIMIT),
    )?;
    yaml_string(&output)
}

/// Returns compact YAML for the current best Hacker News stories.
#[plugin_fn]
pub fn get_best_stories(Json(args): Json<StoryListArgs>) -> FnResult<String> {
    let output = fetch_story_list(
        "beststories",
        clamp_limit(args.limit, DEFAULT_STORY_LIMIT, MAX_STORY_LIMIT),
    )?;
    yaml_string(&output)
}

/// Returns compact YAML for the latest Ask HN stories.
#[plugin_fn]
pub fn get_ask_stories(Json(args): Json<StoryListArgs>) -> FnResult<String> {
    let output = fetch_story_list(
        "askstories",
        clamp_limit(args.limit, DEFAULT_STORY_LIMIT, MAX_STORY_LIMIT),
    )?;
    yaml_string(&output)
}

/// Returns compact YAML for the latest Show HN stories.
#[plugin_fn]
pub fn get_show_stories(Json(args): Json<StoryListArgs>) -> FnResult<String> {
    let output = fetch_story_list(
        "showstories",
        clamp_limit(args.limit, DEFAULT_STORY_LIMIT, MAX_STORY_LIMIT),
    )?;
    yaml_string(&output)
}

/// Returns compact YAML for the latest Hacker News job stories.
#[plugin_fn]
pub fn get_job_stories(Json(args): Json<StoryListArgs>) -> FnResult<String> {
    let output = fetch_story_list(
        "jobstories",
        clamp_limit(args.limit, DEFAULT_STORY_LIMIT, MAX_STORY_LIMIT),
    )?;
    yaml_string(&output)
}

/// Returns a compact YAML view of a single Hacker News item.
#[plugin_fn]
pub fn get_item(Json(args): Json<ItemArgs>) -> FnResult<String> {
    let output = format_item(get_json(&format!("item/{}", args.id))?);
    yaml_string(&output)
}

/// Returns a compact YAML view of a single Hacker News user.
#[plugin_fn]
pub fn get_user(Json(args): Json<UserArgs>) -> FnResult<String> {
    let limit = clamp_limit(
        args.submitted_limit,
        DEFAULT_SUBMITTED_LIMIT,
        MAX_SUBMITTED_LIMIT,
    );
    let output = format_user(get_json(&format!("user/{}", args.id))?, limit);
    yaml_string(&output)
}

/// Returns the latest changed Hacker News item and profile IDs.
#[plugin_fn]
pub fn get_updates(Json(_args): Json<NoArgs>) -> FnResult<String> {
    let updates: HnUpdates = get_json("updates")?;
    let output = UpdatesOutput {
        changed_items_count: updates.items.len(),
        changed_profile_count: updates.profiles.len(),
        items: updates.items,
        profiles: updates.profiles,
    };
    yaml_string(&output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_item(item_type: &str) -> HnItem {
        HnItem {
            id: 42,
            item_type: item_type.to_string(),
            by: Some("alice".to_string()),
            time: Some(1_700_000_000),
            text: Some("hello".to_string()),
            deleted: Some(false),
            dead: Some(false),
            parent: Some(10),
            poll: Some(20),
            kids: Some(vec![1, 2, 3]),
            url: Some("https://example.com".to_string()),
            score: Some(99),
            title: Some("Sample".to_string()),
            parts: Some(vec![7, 8]),
            descendants: Some(12),
        }
    }

    #[test]
    fn builds_hn_urls() {
        assert_eq!(
            build_url("item/123"),
            "https://hacker-news.firebaseio.com/v0/item/123.json"
        );
    }

    #[test]
    fn clamps_story_limits() {
        assert_eq!(clamp_limit(None, DEFAULT_STORY_LIMIT, MAX_STORY_LIMIT), 10);
        assert_eq!(
            clamp_limit(Some(0), DEFAULT_STORY_LIMIT, MAX_STORY_LIMIT),
            1
        );
        assert_eq!(
            clamp_limit(Some(99), DEFAULT_STORY_LIMIT, MAX_STORY_LIMIT),
            25
        );
    }

    #[test]
    fn clamps_submitted_limits() {
        assert_eq!(
            clamp_limit(None, DEFAULT_SUBMITTED_LIMIT, MAX_SUBMITTED_LIMIT),
            20
        );
        assert_eq!(
            clamp_limit(Some(500), DEFAULT_SUBMITTED_LIMIT, MAX_SUBMITTED_LIMIT),
            50
        );
    }

    #[test]
    fn retries_until_success() {
        let mut attempts = 0;

        let result: Result<u32, Error> = retry("test request", || {
            attempts += 1;

            if attempts < 3 {
                Err(Error::msg("temporary failure"))
            } else {
                Ok(42)
            }
        });

        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 3);
    }

    #[test]
    fn returns_last_error_after_retry_exhaustion() {
        let mut attempts = 0;

        let err = retry::<(), _>("test request", || {
            attempts += 1;
            Err(Error::msg(format!("failure #{attempts}")))
        })
        .unwrap_err();

        assert_eq!(attempts, HTTP_RETRY_ATTEMPTS);
        assert_eq!(
            err.to_string(),
            "test request failed after 3 attempts: failure #3"
        );
    }

    #[test]
    fn yaml_omits_empty_fields() {
        let output = ItemOutput {
            id: 1,
            item_type: "story".to_string(),
            by: None,
            time: None,
            title: Some("Hello".to_string()),
            text: None,
            url: None,
            score: None,
            descendants: None,
            deleted: None,
            dead: None,
            parent: None,
            poll: None,
            kids_count: None,
            kids_preview: Vec::new(),
            parts_count: None,
            parts_preview: Vec::new(),
        };

        let yaml = yaml_string(&output).unwrap();
        assert!(yaml.contains("id: 1"));
        assert!(yaml.contains("title: Hello"));
        assert!(!yaml.contains("kids_preview"));
        assert!(!yaml.contains("by:"));
    }

    #[test]
    fn formats_story_summary_for_lists() {
        let summary = format_list_item(sample_item("story"));
        let yaml = yaml_string(&summary).unwrap();

        assert!(yaml.contains("type: story"));
        assert!(yaml.contains("comments: 12"));
        assert!(yaml.contains("score: 99"));
    }

    #[test]
    fn formats_item_variants() {
        for item_type in ["story", "comment", "job", "poll", "pollopt"] {
            let output = format_item(sample_item(item_type));
            assert_eq!(output.item_type, item_type);
            assert_eq!(output.kids_count, Some(3));
            assert_eq!(output.parts_count, Some(2));
        }
    }

    #[test]
    fn formats_user_with_preview() {
        let user = HnUser {
            id: "pg".to_string(),
            created: 1173923446,
            karma: 1000,
            about: Some(String::new()),
            submitted: Some(vec![1, 2, 3, 4]),
        };

        let output = format_user(user, 2);
        assert_eq!(output.submitted_count, Some(4));
        assert_eq!(output.submitted_preview, vec![1, 2]);
        assert!(output.about.is_none());
    }
}
