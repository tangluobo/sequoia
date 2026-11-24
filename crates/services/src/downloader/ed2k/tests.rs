use super::*;
mod wire_fixture;

const USER_LINK: &str = "ed2k://|file|cn_windows_10_business_editions_version_1909_updated_jan_2020_x64_dvd_b3e1f3a6.iso|5311711232|3527D2A9845FF4105F485CC364655B66|/";

#[test]
fn legacy_windows_11_installer_is_routed_by_its_ed2k_uri_without_http_domain_parsing() -> Result<()>
{
    let url = "ed2k://|file|zh-cn_windows_11_business_editions_version_25h2_updated_aug_2026_arm64_dvd_516cd019.iso|8568778752|EFFDAF4F12E25BBF709642531A06D1CD|/";
    let source::Source::Ed2k(link) = source::Source::parse(url)? else {
        anyhow::bail!("Windows image was treated as an HTTP address")
    };
    assert_eq!(link.file_size, 8_568_778_752);
    assert_eq!(link.file_hash, "effdaf4f12e25bbf709642531a06d1cd");
    Ok(())
}

#[test]
fn user_iso_routes_to_ed2k_with_64_bit_metadata() -> Result<()> {
    validate_source(USER_LINK)?;
    let source = source::Source::parse(USER_LINK)?;
    let source::Source::Ed2k(link) = &source else {
        anyhow::bail!("wrong engine");
    };
    let link = link.clone();
    assert_eq!(link.file_size, 5_311_711_232);
    assert_eq!(link.file_hash, "3527d2a9845ff4105f485cc364655b66");
    let record = source.record(USER_LINK.into(), PathBuf::from("downloads"), 8);
    assert_eq!(record.protocol, "ED2K");
    assert_eq!(record.total_bytes, 5_311_711_232);
    assert_eq!(
        record.file_name,
        "cn_windows_10_business_editions_version_1909_updated_jan_2020_x64_dvd_b3e1f3a6.iso"
    );
    let request = tomato_ed2k::protocol::build_get_sources(&link.file_hash_bytes, link.file_size);
    assert_eq!(request.payload.len(), 28);
    assert_eq!(
        u64::from_le_bytes(request.payload[20..28].try_into()?),
        5_311_711_232
    );
    Ok(())
}

#[test]
fn malformed_links_are_rejected_without_rejecting_unicode_or_extensions() -> Result<()> {
    for link in [
        "ed2k://|server|127.0.0.1|4661|/",
        "ed2k://|file||12|3527D2A9845FF4105F485CC364655B66|/",
        "ed2k://|file|f|0|3527D2A9845FF4105F485CC364655B66|/",
        "ed2k://|file|f|12|hello|/",
        "ed2k://|file|f|12|3527D2A9845FF4105F485CC364655B66",
        "ed2k://|file|f|18446744073709551615|3527D2A9845FF4105F485CC364655B66|/",
    ] {
        assert!(validate_source(link).is_err(), "{link}");
    }
    let source::Source::Ed2k(link) = source::Source::parse(
        "ED2K://|file|%E6%B5%8B%E8%AF%95.iso|5311711232|3527D2A9845FF4105F485CC364655B66|h=H52BRVWPBBTAED5NXQDH2RJDDAKRUWST|/|sources,127.0.0.1:4662|/",
    )?
    else {
        panic!()
    };
    assert_eq!(link.file_name, "测试.iso");
    assert_eq!(link.sources[0].port, 4662);
    assert!(validate_source("https://example.com/a.zip").is_ok());
    assert!(validate_source("magnet:?xt=urn:btih:fixture").is_err());
    Ok(())
}

#[test]
fn ed2k_destination_is_owned_and_never_overwrites_an_existing_file() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let source = source::Source::parse(USER_LINK)?;
    let source::Source::Ed2k(link) = source::Source::parse(USER_LINK)? else {
        panic!()
    };
    let mut record = source.record(USER_LINK.into(), dir.path().to_owned(), 4);
    let existing = dir.path().join(&record.file_name);
    std::fs::write(&existing, b"user file")?;
    let target = reserve(&record, &link)?;
    assert_ne!(target.path, existing);
    record.file_name = target.path.file_name().unwrap().to_string_lossy().into();
    record.target_file = Some(target.path.display().to_string());
    assert!(reserve(&record, &link).is_err());
    let resume_path = target.path.clone();
    drop(target);
    let target = reserve(&record, &link)?;
    assert_eq!(target.path, resume_path);
    assert_eq!(std::fs::read(existing)?, b"user file");
    Ok(())
}

#[test]
fn legacy_download_history_without_rust_id_resumes_the_same_locked_and_hash_matched_partial()
-> Result<()> {
    let directory = tempfile::tempdir()?;
    let source::Source::Ed2k(link) = source::Source::parse(USER_LINK)? else {
        unreachable!()
    };
    let source = source::Source::parse(USER_LINK)?;
    let mut original = source.record(USER_LINK.into(), directory.path().into(), 8);
    let target = reserve(&original, &link)?;
    original.target_file = Some(target.path.display().to_string());
    let path = target.path.clone();
    let owner_path = target.owner_path.clone();
    let mut legacy = serde_json::to_value(&original)?;
    legacy.as_object_mut().unwrap().remove("rustId");
    let restored: Record = serde_json::from_value(legacy)?;
    assert_ne!(
        restored.id, original.id,
        "Java histories do not retain Rust's transient task id"
    );
    assert!(
        reserve(&restored, &link).is_err(),
        "an active file lease must still block another actor"
    );
    drop(target);
    let owner_before = std::fs::read(&owner_path)?;
    let resumed = reserve(&restored, &link)?;
    assert_eq!(resumed.path, path);
    drop(resumed);
    assert_eq!(
        std::fs::read(&owner_path)?,
        owner_before,
        "recovering ownership must not rewrite user sidecars"
    );
    let mut wrong_hash = link.clone();
    wrong_hash.file_hash = "00".repeat(16);
    wrong_hash.file_hash_bytes = [0; 16];
    assert!(
        reserve(&restored, &wrong_hash).is_err(),
        "a legacy id is not permission to adopt a different file hash"
    );
    let mut wrong_size = link.clone();
    wrong_size.file_size += 1;
    assert!(
        reserve(&restored, &wrong_size).is_err(),
        "a different file length must remain protected"
    );
    let mut unrelated = restored;
    unrelated.target_file = None;
    assert_ne!(
        reserve(&unrelated, &link)?.path,
        path,
        "another task without this explicit resume path needs a new destination"
    );
    Ok(())
}
