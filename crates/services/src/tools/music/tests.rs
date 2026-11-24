use super::*;
use axum::{
    Json, Router,
    extract::{Path as RoutePath, Query},
    routing::get,
};
use serde_json::{Value, json};
use std::collections::HashMap;

fn flac() -> Vec<u8> {
    let mut bytes = b"fLaC\x80\x00\x00\x22".to_vec();
    bytes.resize(44, 0);
    bytes
}

#[test]
fn public_tracks_use_file_artists_and_filter_the_query() -> Result<()> {
    let fixture = || {
        serde_json::from_value::<Item>(json!({
            "metadata": {"title": "精选录音"},
            "files": [
                {"name": "song.flac", "size": "44", "title": "Mojito", "artist": "周杰伦"},
                {"name": "other.flac", "size": "44", "title": "Other Song", "artist": "Other Artist"},
                {"name": "hidden.flac", "size": "44", "title": "Mojito", "artist": "周杰伦", "private": true},
                {"name": "song.mp3", "size": "44", "title": "Mojito", "artist": "周杰伦"}
            ]
        }))
    };
    let artist_tracks: Vec<_> = fixture()?
        .tracks("collection", "周杰伦", SearchBy::Any, SearchScope::Public)
        .collect();
    assert_eq!(artist_tracks.len(), 1);
    assert_eq!(artist_tracks[0].artist, "周杰伦");
    assert_eq!(artist_tracks[0].title, "Mojito");
    let song_tracks: Vec<_> = fixture()?
        .tracks("collection", "mojito", SearchBy::Song, SearchScope::Public)
        .collect();
    assert_eq!(song_tracks.len(), 1);
    assert_eq!(song_tracks[0].file, "song.flac");
    Ok(())
}

async fn index(Query(query): Query<HashMap<String, String>>) -> Json<Value> {
    assert_eq!(query["page"], "1");
    assert!(query["q"].contains("collection:etree AND format:FLAC"));
    assert!(
        query["q"].contains(r#"creator:"test artist""#)
            || query["q"].contains(r#"description:"test song""#)
    );
    Json(json!({"response": {"numFound": 13, "docs": [
        {"identifier": "concert"}, {"identifier": "restricted"}
    ]}}))
}

async fn metadata(RoutePath(item): RoutePath<String>) -> Json<Value> {
    let sha1 = format!("{:x}", Sha1::digest(flac()));
    Json(json!({
        "metadata": {
            "title": "Test recording", "creator": ["Test Artist"],
            "access-restricted-item": item == "restricted"
        },
        "files": [
            {"name": "disc 1/a.flac", "size": "44", "title": "Test Song", "sha1": sha1},
            {"name": "b.flac", "size": "44", "title": "Other Song"},
            {"name": "private.flac", "size": "44", "title": "Test Song", "private": "true"},
            {"name": "preview.mp3", "size": "44", "title": "Test Song"}
        ]
    }))
}

async fn download(RoutePath(file): RoutePath<String>) -> Vec<u8> {
    if file == "disc 1/a.flac" {
        flac()
    } else {
        vec![b'x'; 44]
    }
}

#[tokio::test]
async fn loopback_artist_and_song_search_and_partial_batch() -> Result<()> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let address = listener.local_addr()?;
    let app = Router::new()
        .route("/advancedsearch.php", get(index))
        .route("/metadata/{item}", get(metadata))
        .route("/download/concert/{*file}", get(download));
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let archive = Archive {
        client: Client::new(),
        base: Url::parse(&format!("http://{address}/"))?,
    };
    let Output::Search {
        tracks,
        more,
        unavailable,
        ..
    } = archive
        .search(SearchBy::Song, "Test Song", 0, SearchScope::Live)
        .await?
    else {
        anyhow::bail!("Expected song search results");
    };
    assert!(more);
    assert_eq!(unavailable, 0);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].artist, "Test Artist");
    assert_eq!(tracks[0].file, "disc 1/a.flac");
    let Output::Search { tracks, .. } = archive
        .search(SearchBy::Artist, "Test Artist", 0, SearchScope::Live)
        .await?
    else {
        anyhow::bail!("Expected artist search results");
    };
    assert_eq!(tracks.len(), 2);
    let root = tempfile::tempdir()?;
    let existing = root.path().join("Test Artist - Test Song.flac");
    std::fs::write(&existing, b"existing")?;
    let control = Control::default();
    let Output::Download {
        outcomes,
        cancelled,
    } = archive
        .run(
            Request::Download {
                tracks,
                directory: root.path().into(),
            },
            &control,
        )
        .await?
    else {
        anyhow::bail!("Expected batch download results");
    };
    assert!(!cancelled);
    let [Download::Saved(path), Download::Failed(reason)] = outcomes.as_slice() else {
        anyhow::bail!("Expected one saved track and one rejected file: {outcomes:?}");
    };
    assert_eq!(std::fs::read(path)?, flac());
    assert_eq!(std::fs::read(existing)?, b"existing");
    assert!(reason.contains("FLAC"));
    assert_eq!(std::fs::read_dir(root.path())?.count(), 2);
    assert_eq!(control.read(|p| (p.objects, p.total)), Some((2, 2)));
    server.abort();
    Ok(())
}
