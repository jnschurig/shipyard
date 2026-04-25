use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeZone, Utc};
use futures_util::StreamExt;
use reqwest::header::{
    ACCEPT, AUTHORIZATION, ETAG, HeaderMap, HeaderValue, IF_NONE_MATCH, USER_AGENT,
};
use reqwest::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;

const API_BASE: &str = "https://api.github.com";
const USER_AGENT_VAL: &str = concat!("shipyard/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Release {
    pub tag_name: String,
    pub name: Option<String>,
    pub published_at: Option<String>,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RateLimitStatus {
    pub remaining: Option<u32>,
    pub limit: Option<u32>,
    pub reset_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("rate limited; resets at {reset_at:?}")]
    RateLimited { reset_at: Option<DateTime<Utc>> },
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("unexpected status {status}: {body}")]
    UnexpectedStatus { status: StatusCode, body: String },
    #[error("io error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cache decode error: {0}")]
    CacheDecode(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    etag: String,
    releases: Vec<Release>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct EtagCache {
    #[serde(flatten)]
    entries: HashMap<String, CacheEntry>,
}

pub struct Client {
    http: reqwest::Client,
    api_base: String,
    cache_path: PathBuf,
    cache: Arc<Mutex<EtagCache>>,
    token: Option<String>,
}

impl Client {
    pub fn new(cache_path: PathBuf) -> Result<Self> {
        Self::with_base(cache_path, API_BASE.to_string())
    }

    pub fn with_base(cache_path: PathBuf, api_base: String) -> Result<Self> {
        let cache = load_cache(&cache_path);
        let token = std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty());
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT_VAL)
            .build()?;
        Ok(Self {
            http,
            api_base,
            cache_path,
            cache: Arc::new(Mutex::new(cache)),
            token,
        })
    }

    fn auth_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VAL));
        h.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        if let Some(t) = &self.token
            && let Ok(v) = HeaderValue::from_str(&format!("Bearer {t}"))
        {
            h.insert(AUTHORIZATION, v);
        }
        h
    }

    pub async fn list_releases(&self, repo_slug: &str) -> Result<(Vec<Release>, RateLimitStatus)> {
        let url = format!("{}/repos/{}/releases", self.api_base, repo_slug);
        let mut headers = self.auth_headers();
        let cached_etag = {
            let cache = self.cache.lock().unwrap();
            cache.entries.get(repo_slug).map(|e| e.etag.clone())
        };
        if let Some(etag) = &cached_etag
            && let Ok(v) = HeaderValue::from_str(etag)
        {
            headers.insert(IF_NONE_MATCH, v);
        }

        let resp = self.http.get(&url).headers(headers).send().await?;
        let rl = parse_rate_limit(resp.headers());
        let status = resp.status();

        if status == StatusCode::NOT_MODIFIED {
            let cache = self.cache.lock().unwrap();
            let releases = cache
                .entries
                .get(repo_slug)
                .map(|e| e.releases.clone())
                .unwrap_or_default();
            return Ok((releases, rl));
        }

        if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
            if let Some(rem) = rl.remaining
                && rem == 0
            {
                return Err(Error::RateLimited {
                    reset_at: rl.reset_at,
                });
            }
            // 403 for other reasons — still surface as rate-limited with no reset,
            // since secondary rate limits also use 403 without remaining=0.
            return Err(Error::RateLimited {
                reset_at: rl.reset_at,
            });
        }

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::UnexpectedStatus { status, body });
        }

        let etag = resp
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let releases: Vec<Release> = resp.json().await?;

        if let Some(etag) = etag {
            let mut cache = self.cache.lock().unwrap();
            cache.entries.insert(
                repo_slug.to_string(),
                CacheEntry {
                    etag,
                    releases: releases.clone(),
                },
            );
            save_cache(&self.cache_path, &cache)?;
        }

        Ok((releases, rl))
    }

    pub async fn download_asset(
        &self,
        asset_url: &str,
        dest: &Path,
        progress_tx: Option<mpsc::Sender<DownloadProgress>>,
    ) -> Result<()> {
        let resp = self
            .http
            .get(asset_url)
            .headers(self.auth_headers())
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::UnexpectedStatus { status, body });
        }
        let total = resp.content_length();

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let tmp = dest.with_extension("shipyard-partial");
        let mut file = fs::File::create(&tmp).map_err(|e| Error::Io {
            path: tmp.clone(),
            source: e,
        })?;

        let mut stream = resp.bytes_stream();
        let mut downloaded: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).map_err(|e| Error::Io {
                path: tmp.clone(),
                source: e,
            })?;
            downloaded += chunk.len() as u64;
            if let Some(tx) = &progress_tx {
                let _ = tx.send(DownloadProgress { downloaded, total }).await;
            }
        }
        file.sync_all().map_err(|e| Error::Io {
            path: tmp.clone(),
            source: e,
        })?;
        drop(file);

        fs::rename(&tmp, dest).map_err(|e| Error::Io {
            path: dest.to_path_buf(),
            source: e,
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

fn load_cache(path: &Path) -> EtagCache {
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => EtagCache::default(),
    }
}

fn save_cache(path: &Path, cache: &EtagCache) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let json = serde_json::to_string_pretty(cache)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| Error::Io {
        path: tmp.clone(),
        source: e,
    })?;
    fs::rename(&tmp, path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(())
}

fn parse_rate_limit(headers: &reqwest::header::HeaderMap) -> RateLimitStatus {
    let get = |k: &str| headers.get(k).and_then(|v| v.to_str().ok());
    let remaining = get("x-ratelimit-remaining").and_then(|s| s.parse().ok());
    let limit = get("x-ratelimit-limit").and_then(|s| s.parse().ok());
    let reset_at = get("x-ratelimit-reset")
        .and_then(|s| s.parse::<i64>().ok())
        .and_then(|ts| Utc.timestamp_opt(ts, 0).single());
    RateLimitStatus {
        remaining,
        limit,
        reset_at,
    }
}

fn _response_type_dummy(_: &Response) {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_release_json() -> serde_json::Value {
        serde_json::json!([{
            "tag_name": "9.2.3",
            "name": "Ackbar Delta 9.2.3",
            "published_at": "2026-04-14T13:07:27Z",
            "assets": [{
                "name": "SoH-Ackbar-Delta-Mac.zip",
                "browser_download_url": "https://example.invalid/mac.zip",
                "size": 46385648u64
            }]
        }])
    }

    #[tokio::test]
    async fn list_releases_success_caches_etag() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/releases"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "\"abc123\"")
                    .insert_header("x-ratelimit-remaining", "59")
                    .insert_header("x-ratelimit-limit", "60")
                    .insert_header("x-ratelimit-reset", "1713700000")
                    .set_body_json(sample_release_json()),
            )
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("etags.json");
        let client = Client::with_base(cache_path.clone(), server.uri()).unwrap();

        let (releases, rl) = client.list_releases("foo/bar").await.unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].tag_name, "9.2.3");
        assert_eq!(rl.remaining, Some(59));
        assert_eq!(rl.limit, Some(60));
        assert!(rl.reset_at.is_some());
        assert!(cache_path.exists());
    }

    #[tokio::test]
    async fn list_releases_304_uses_cache_across_client_restart() {
        let server = MockServer::start().await;
        // Mount the more-specific (if-none-match) mock first so wiremock matches it
        // in preference to the generic 200 on the conditional request.
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/releases"))
            .and(header("if-none-match", "\"abc123\""))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/releases"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "\"abc123\"")
                    .set_body_json(sample_release_json()),
            )
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("etags.json");

        // First call populates the cache.
        {
            let c1 = Client::with_base(cache_path.clone(), server.uri()).unwrap();
            let (r, _) = c1.list_releases("foo/bar").await.unwrap();
            assert_eq!(r.len(), 1);
        }

        // New client, fresh memory — must rehydrate ETag from disk and get 304.
        let c2 = Client::with_base(cache_path.clone(), server.uri()).unwrap();
        let (r, _) = c2.list_releases("foo/bar").await.unwrap();
        assert_eq!(r.len(), 1, "304 path should return cached releases");
    }

    #[tokio::test]
    async fn list_releases_rate_limit_is_typed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/foo/bar/releases"))
            .respond_with(
                ResponseTemplate::new(403)
                    .insert_header("x-ratelimit-remaining", "0")
                    .insert_header("x-ratelimit-limit", "60")
                    .insert_header("x-ratelimit-reset", "1713700000")
                    .set_body_string("API rate limit exceeded"),
            )
            .mount(&server)
            .await;

        let dir = tempdir().unwrap();
        let client = Client::with_base(dir.path().join("etags.json"), server.uri()).unwrap();

        match client.list_releases("foo/bar").await {
            Err(Error::RateLimited { reset_at }) => {
                assert!(reset_at.is_some());
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn download_asset_streams_to_dest() {
        let server = MockServer::start().await;
        let body = b"hello shipyard".to_vec();
        Mock::given(method("GET"))
            .and(path("/dl/thing.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempdir().unwrap();
        let dest = dir.path().join("thing.zip");
        let client = Client::with_base(dir.path().join("etags.json"), server.uri()).unwrap();

        let (tx, mut rx) = mpsc::channel::<DownloadProgress>(16);
        let url = format!("{}/dl/thing.zip", server.uri());

        let download = client.download_asset(&url, &dest, Some(tx));
        let (res, mut got_progress) = tokio::join!(download, async {
            let mut any = false;
            while rx.recv().await.is_some() {
                any = true;
            }
            any
        });
        res.unwrap();
        // drain any leftover (after sender drops rx returns None) — already done.
        let _ = &mut got_progress;

        assert!(dest.exists());
        let got = fs::read(&dest).unwrap();
        assert_eq!(got, body);
        assert!(
            !dir.path().join("thing.shipyard-partial").exists(),
            "partial file should have been renamed"
        );
    }
}
