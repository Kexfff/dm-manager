//! A client for Wallhaven's API (https://wallhaven.cc/help/api): search, details, cached
//! thumbnails and downloads.
//!
//! Without an API key, only SFW and sketchy wallpapers are available; the API allows about
//! 45 requests a minute.

use std::path::{Path, PathBuf};

use serde::Deserialize;

const API: &str = "https://wallhaven.cc/api/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sorting {
    Toplist,
    Hot,
    Latest,
    Random,
    Views,
    Favorites,
    Relevance,
}

impl Sorting {
    pub const ALL: [Self; 7] =
        [Self::Toplist, Self::Hot, Self::Latest, Self::Random, Self::Views, Self::Favorites, Self::Relevance];

    fn param(self) -> &'static str {
        match self {
            Self::Toplist => "toplist",
            Self::Hot => "hot",
            Self::Latest => "date_added",
            Self::Random => "random",
            Self::Views => "views",
            Self::Favorites => "favorites",
            Self::Relevance => "relevance",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Toplist => "Top",
            Self::Hot => "Hot",
            Self::Latest => "Latest",
            Self::Random => "Random",
            Self::Views => "Most Viewed",
            Self::Favorites => "Most Favorited",
            Self::Relevance => "Relevance",
        }
    }
}

/// How far back the toplist looks: `(param, label)`.
pub const TOP_RANGES: [(&str, &str); 7] = [
    ("1d", "Today"),
    ("3d", "3 Days"),
    ("1w", "Week"),
    ("1M", "Month"),
    ("3M", "3 Months"),
    ("6M", "6 Months"),
    ("1y", "Year"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub text: String,
    /// General, anime, people.
    pub categories: [bool; 3],
    /// SFW, sketchy, NSFW.
    pub purity: [bool; 3],
    pub sorting: Sorting,
    pub top_range: &'static str,
    /// Smallest resolution, e.g. `(2560, 1440)`.
    pub at_least: Option<(u32, u32)>,
    /// Aspect ratios like `16x9`; empty for any.
    pub ratios: Vec<String>,
    /// A color, as six hex digits (`663399`).
    pub color: Option<String>,
    pub page: u32,
    /// Keeps a random sort stable across pages.
    pub seed: Option<String>,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            text: String::new(),
            categories: [true, true, false],
            purity: [true, false, false],
            sorting: Sorting::Toplist,
            top_range: "1M",
            at_least: None,
            ratios: Vec::new(),
            color: None,
            page: 1,
            seed: None,
        }
    }
}

impl Query {
    fn params(&self, api_key: Option<&str>) -> Vec<(&'static str, String)> {
        let bits = |b: [bool; 3]| b.iter().map(|on| if *on { '1' } else { '0' }).collect::<String>();
        let mut params = vec![
            ("categories", bits(self.categories)),
            ("purity", bits(self.purity)),
            ("sorting", self.sorting.param().into()),
            ("page", self.page.to_string()),
        ];
        if !self.text.trim().is_empty() {
            params.push(("q", self.text.trim().into()));
        }
        if self.sorting == Sorting::Toplist {
            params.push(("topRange", self.top_range.into()));
        }
        if let Some((w, h)) = self.at_least {
            params.push(("atleast", format!("{w}x{h}")));
        }
        if !self.ratios.is_empty() {
            params.push(("ratios", self.ratios.join(",")));
        }
        if let Some(color) = &self.color {
            params.push(("colors", color.trim_start_matches('#').into()));
        }
        if let Some(seed) = &self.seed {
            params.push(("seed", seed.clone()));
        }
        if let Some(key) = api_key {
            params.push(("apikey", key.into()));
        }
        params
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Thumbs {
    pub large: String,
    pub original: String,
    pub small: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Wallpaper {
    pub id: String,
    /// The wallpaper's page on wallhaven.cc.
    pub url: String,
    /// The full image.
    pub path: String,
    pub resolution: String,
    pub dimension_x: u32,
    pub dimension_y: u32,
    pub file_size: u64,
    pub file_type: String,
    pub category: String,
    pub purity: String,
    pub views: u64,
    pub favorites: u64,
    pub colors: Vec<String>,
    pub thumbs: Thumbs,
}

impl Wallpaper {
    /// `wallhaven-<id>.<ext>`, as Wallhaven names its downloads.
    pub fn file_name(&self) -> String {
        self.path.rsplit('/').next().map(String::from).unwrap_or_else(|| format!("wallhaven-{}.jpg", self.id))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub wallpapers: Vec<Wallpaper>,
    pub page: u32,
    pub last_page: u32,
    pub total: u64,
    pub seed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Details {
    pub tags: Vec<Tag>,
    pub uploader: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Tag {
    pub id: u64,
    pub name: String,
}

#[derive(Deserialize)]
struct SearchResponse {
    data: Vec<Wallpaper>,
    meta: Meta,
}

#[derive(Deserialize)]
struct Meta {
    current_page: u32,
    last_page: u32,
    total: u64,
    seed: Option<String>,
}

#[derive(Deserialize)]
struct DetailsResponse {
    data: DetailsData,
}

#[derive(Deserialize)]
struct DetailsData {
    #[serde(default)]
    tags: Vec<Tag>,
    uploader: Option<Uploader>,
    source: Option<String>,
}

#[derive(Deserialize)]
struct Uploader {
    username: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbSize {
    Small,
    Large,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    api_key: Option<String>,
}

impl Client {
    pub fn new(api_key: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("dm-manager/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { http, api_key: api_key.filter(|k| !k.trim().is_empty()) }
    }

    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    pub async fn search(&self, query: &Query) -> Result<Page, String> {
        let response = self
            .http
            .get(format!("{API}/search"))
            .query(&query.params(self.api_key.as_deref()))
            .send()
            .await
            .map_err(|e| network_error(&e))?;
        let body: SearchResponse = checked(response).await?.json().await.map_err(|e| e.to_string())?;
        Ok(Page {
            wallpapers: body.data,
            page: body.meta.current_page,
            last_page: body.meta.last_page,
            total: body.meta.total,
            seed: body.meta.seed,
        })
    }

    pub async fn details(&self, id: &str) -> Result<Details, String> {
        let mut request = self.http.get(format!("{API}/w/{id}"));
        if let Some(key) = &self.api_key {
            request = request.query(&[("apikey", key)]);
        }
        let response = request.send().await.map_err(|e| network_error(&e))?;
        let body: DetailsResponse = checked(response).await?.json().await.map_err(|e| e.to_string())?;
        Ok(Details {
            tags: body.data.tags,
            uploader: body.data.uploader.map(|u| u.username),
            source: body.data.source.filter(|s| !s.is_empty()),
        })
    }

    /// A thumbnail, from the cache or downloaded into it.
    pub async fn thumbnail(&self, wallpaper: &Wallpaper, size: ThumbSize) -> Result<PathBuf, String> {
        let (url, suffix) = match size {
            ThumbSize::Small => (&wallpaper.thumbs.small, "small"),
            ThumbSize::Large => (&wallpaper.thumbs.large, "large"),
        };
        let path = cache_dir().join(format!("{}-{suffix}.jpg", wallpaper.id));
        if path.exists() {
            return Ok(path);
        }
        let response = self.http.get(url).send().await.map_err(|e| network_error(&e))?;
        let bytes = checked(response).await?.bytes().await.map_err(|e| e.to_string())?;
        write_atomically(&path, &bytes)?;
        Ok(path)
    }

    /// Downloads the full image into `dir` (unless it's already there), reporting
    /// `(bytes so far, total)` as it goes.
    pub async fn download(
        &self,
        wallpaper: &Wallpaper,
        dir: &Path,
        progress: impl Fn(u64, u64),
    ) -> Result<PathBuf, String> {
        let path = dir.join(wallpaper.file_name());
        if std::fs::metadata(&path).is_ok_and(|m| m.len() == wallpaper.file_size || wallpaper.file_size == 0) {
            return Ok(path);
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
        let mut response = checked(self.http.get(&wallpaper.path).send().await.map_err(|e| network_error(&e))?).await?;
        let total = response.content_length().unwrap_or(wallpaper.file_size);
        let mut data = Vec::with_capacity(total as usize);
        while let Some(chunk) = response.chunk().await.map_err(|e| network_error(&e))? {
            data.extend_from_slice(&chunk);
            progress(data.len() as u64, total);
        }
        write_atomically(&path, &data)?;
        Ok(path)
    }
}

async fn checked(response: reqwest::Response) -> Result<reqwest::Response, String> {
    match response.status().as_u16() {
        200..=299 => Ok(response),
        401 => Err("Wallhaven didn't accept the API key".into()),
        429 => Err("Wallhaven is limiting requests; try again in a minute".into()),
        404 => Err("Not found on Wallhaven".into()),
        code => Err(format!("Wallhaven answered with error {code}")),
    }
}

fn network_error(e: &reqwest::Error) -> String {
    if e.is_connect() || e.is_timeout() { "Can't reach wallhaven.cc — are you online?".into() } else { e.to_string() }
}

fn write_atomically(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, data).map_err(|e| format!("Can't write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Can't save {}: {e}", path.display()))
}

/// `$XDG_CACHE_HOME/dm-manager/wallhaven`
pub fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| crate::home().unwrap_or_default().join(".cache"))
        .join("dm-manager")
        .join("wallhaven")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parameters() {
        let query = Query { text: " cats ".into(), at_least: Some((2560, 1440)), ..Default::default() };
        let params = query.params(Some("key"));
        let get = |k: &str| params.iter().find(|(p, _)| *p == k).map(|(_, v)| v.as_str());
        assert_eq!(get("categories"), Some("110"));
        assert_eq!(get("purity"), Some("100"));
        assert_eq!(get("sorting"), Some("toplist"));
        assert_eq!(get("topRange"), Some("1M"));
        assert_eq!(get("q"), Some("cats"));
        assert_eq!(get("atleast"), Some("2560x1440"));
        assert_eq!(get("apikey"), Some("key"));
        let latest = Query { sorting: Sorting::Latest, ..Default::default() }.params(None);
        assert!(!latest.iter().any(|(p, _)| *p == "topRange" || *p == "apikey"));
    }

    #[test]
    fn parses_a_search_response() {
        let json = r##"{"data":[{"id":"jel1jq","url":"https://wallhaven.cc/w/jel1jq","short_url":"x","views":5,"favorites":3,
            "source":"","purity":"sfw","category":"general","dimension_x":3840,"dimension_y":2160,"resolution":"3840x2160",
            "ratio":"1.78","file_size":4753754,"file_type":"image/jpeg","created_at":"2026-09-22 07:53:43","colors":["#000000"],
            "path":"https://w.wallhaven.cc/full/je/wallhaven-jel1jq.jpg",
            "thumbs":{"large":"l","original":"o","small":"s"}}],
            "meta":{"current_page":1,"last_page":88,"per_page":24,"total":2107,"query":null,"seed":null}}"##;
        let r: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.data[0].file_name(), "wallhaven-jel1jq.jpg");
        assert_eq!(r.meta.last_page, 88);
    }
}
