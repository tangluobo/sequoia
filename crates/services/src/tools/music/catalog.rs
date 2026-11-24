//! Read-only song discovery through Apple's documented iTunes Search API.
use anyhow::{Context, Result, ensure};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const LIMIT: usize = 25;

#[derive(Debug, Serialize)]
pub struct CatalogTrack {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub page: String,
}

#[derive(Deserialize)]
struct SearchResponse {
    results: Vec<SearchTrack>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchTrack {
    kind: Option<String>,
    track_id: Option<u64>,
    track_name: Option<String>,
    artist_name: Option<String>,
    collection_name: Option<String>,
    track_view_url: Option<String>,
}

pub(super) async fn search(client: &Client, query: &str) -> Result<Vec<CatalogTrack>> {
    let response: SearchResponse = super::get_json(client, search_url(query)?)
        .await
        .context("华语目录搜索失败")?;
    Ok(response.tracks())
}

fn search_url(query: &str) -> Result<Url> {
    let query = query.trim();
    ensure!(
        !query.is_empty() && query.chars().count() <= 100,
        "请输入 1–100 个字符的搜索词"
    );
    let mut url = Url::parse("https://itunes.apple.com/search")?;
    url.query_pairs_mut()
        .append_pair("term", query)
        .append_pair("country", "TW")
        .append_pair("media", "music")
        .append_pair("entity", "song")
        .append_pair("limit", &LIMIT.to_string());
    Ok(url)
}

impl SearchResponse {
    fn tracks(self) -> Vec<CatalogTrack> {
        let mut ids = HashSet::new();
        self.results
            .into_iter()
            .take(LIMIT)
            .filter_map(|track| {
                let page = Url::parse(&track.track_view_url?).ok()?;
                if track.kind.as_deref() != Some("song")
                    || page.scheme() != "https"
                    || page.host_str() != Some("music.apple.com")
                    || !page.username().is_empty()
                    || page.password().is_some()
                    || page.port().is_some()
                {
                    return None;
                }
                let title: String = track.track_name?.chars().take(200).collect();
                let artist: String = track.artist_name?.chars().take(200).collect();
                if title.trim().is_empty()
                    || artist.trim().is_empty()
                    || !ids.insert(track.track_id?)
                {
                    return None;
                }
                Some(CatalogTrack {
                    title,
                    artist,
                    album: track
                        .collection_name
                        .unwrap_or_default()
                        .chars()
                        .take(200)
                        .collect(),
                    page: page.into(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplified_search_preserves_the_query_and_accepts_the_original_catalog_track() -> Result<()>
    {
        let target = search_url("告白气球")?;
        let params: std::collections::HashMap<_, _> = target.query_pairs().collect();
        assert_eq!(params["term"], "告白气球");
        assert_eq!(params["country"], "TW");
        let response: SearchResponse = serde_json::from_str(
            r#"{"results":[
                {"kind":"song","trackId":1118757877,"trackName":"告白氣球","artistName":"周杰倫",
                 "collectionName":"周杰倫的床邊故事","trackViewUrl":"https://music.apple.com/tw/album/1118757859?i=1118757877"},
                {"kind":"song","trackId":11,"trackName":"無效","artistName":"無效",
                 "trackViewUrl":"https://music.apple.com.example.com/tw/album/11"}
            ]}"#,
        )?;
        let tracks = response.tracks();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "告白氣球");
        assert_eq!(tracks[0].artist, "周杰倫");
        Ok(())
    }
}
