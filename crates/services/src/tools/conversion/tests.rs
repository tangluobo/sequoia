use super::*;
use anyhow::Result;
use serde_json::json;

#[tokio::test]
async fn recursive_svg_batch_keeps_relative_names_continues_after_failure_and_preserves_old_output()
-> Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    let output = root.path().join("new-output");
    for folder in ["first", "nested/second"] {
        std::fs::create_dir_all(source.join(folder))?;
        std::fs::write(
            source.join(folder).join("same.svg"),
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="17" height="9"><rect width="17" height="9" fill="#c86432"/></svg>"##,
        )?;
    }
    std::fs::create_dir_all(&output)?;
    std::fs::write(source.join("broken.svg"), "<svg invalid")?;
    std::fs::write(output.join("broken.png"), b"previous-owned-output")?;
    let report = execute(
        Request::Images {
            source,
            directory: output.clone(),
            width: 0,
        },
        Control::default(),
    )
    .await?;
    let Report::Images(counts) = &report else {
        panic!("image report")
    };
    assert_eq!((counts.total, counts.success, counts.failed), (3, 2, 1));
    assert!(report.has_errors());
    for folder in ["first", "nested/second"] {
        let image = image::open(output.join(folder).join("same.png"))?.to_rgba8();
        assert_eq!(image.dimensions(), (17, 9));
        assert_eq!(image.get_pixel(8, 4).0, [200, 100, 50, 255]);
    }
    assert_eq!(
        std::fs::read(output.join("broken.png"))?,
        b"previous-owned-output"
    );
    assert!(!output.join("same.png").exists());
    Ok(())
}
#[tokio::test]
async fn single_svg_uses_new_directory_and_cancelled_command_never_publishes() -> Result<()> {
    let root = tempfile::tempdir()?;
    let empty = root.path().join("empty");
    std::fs::create_dir(&empty)?;
    let error = execute(
        Request::Images {
            source: empty,
            directory: root.path().join("empty-output"),
            width: 0,
        },
        Control::default(),
    )
    .await
    .unwrap_err();
    assert!(error.is::<NoImages>());
    let source = root.path().join("中文.svg");
    std::fs::write(
        &source,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4"><rect width="8" height="4" fill="red"/></svg>"##,
    )?;
    let directory = root.path().join("not-created-yet");
    execute(
        Request::Images {
            source: source.clone(),
            directory: directory.clone(),
            width: 40,
        },
        Control::default(),
    )
    .await?;
    assert_eq!(
        image::open(directory.join("中文.png"))?
            .to_rgba8()
            .dimensions(),
        (40, 20)
    );
    let cancelled = Control::default();
    cancelled.cancel();
    assert!(
        execute(
            Request::Images {
                source,
                directory: root.path().join("cancelled"),
                width: 0
            },
            cancelled
        )
        .await
        .is_err()
    );
    assert!(!root.path().join("cancelled").exists());
    Ok(())
}
#[tokio::test]
async fn recursive_voc_preserves_java_metadata_encodings_and_skips_only_bad_inputs() -> Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("voc/sub");
    std::fs::create_dir_all(&source)?;
    let annotation = |name: &str| {
        format!(
            "<annotation><filename>{name}</filename><size><width>100</width><height>80</height></size><object><name>　猫&amp;犬　</name><bndbox><xmin>-2</xmin><ymin>20</ymin><xmax>140</xmax><ymax>60</ymax></bndbox></object><object><name>bad</name><bndbox><xmin>oops</xmin></bndbox></object></annotation>"
        )
    };
    std::fs::write(source.join("utf8.xml"), annotation("中文.jpg"))?;
    let utf16 = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>{}",
        annotation("乌龟.jpg")
    );
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(utf16.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(source.join("utf16.xml"), bytes)?;
    let latin = "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><annotation><filename>café.jpg</filename><size><width>9</width><height>7</height></size></annotation>";
    let (bytes, _, errors) = encoding_rs::WINDOWS_1252.encode(latin);
    assert!(!errors);
    std::fs::write(source.join("latin.xml"), &bytes)?;
    std::fs::write(source.join("bad.xml"), "<annotation>broken")?;
    std::fs::write(
        source.join("missing.xml"),
        "<annotation><filename>missing.jpg</filename></annotation>",
    )?;
    std::fs::write(
        source.join("entity.xml"),
        r#"<!DOCTYPE annotation [<!ENTITY ext SYSTEM "file:///not-an-owned-fixture">]><annotation><filename>&ext;</filename><size><width>1</width><height>1</height></size></annotation>"#,
    )?;
    let output = root.path().join("coco-new");
    let Report::Dataset(counts) = execute(
        Request::Dataset {
            source: root.path().join("voc"),
            directory: output.clone(),
        },
        Control::default(),
    )
    .await?
    else {
        panic!("dataset report")
    };
    assert_eq!(
        (counts.total, counts.success, counts.failed, counts.records),
        (6, 3, 3, 2)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("coco_annotations.json"))?)?;
    assert_eq!(value["info"]["description"], "Converted from PASCAL VOC");
    assert_eq!(value["info"]["contributor"], "Tomato");
    assert_eq!(value["licenses"][0]["name"], "Unknown");
    assert_eq!(
        value["categories"],
        json!([{"id":1,"name":"　猫&犬　","supercategory":"object"}])
    );
    let names: std::collections::BTreeSet<_> = value["images"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["file_name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["中文.jpg", "乌龟.jpg", "café.jpg"].into_iter().collect()
    );
    for image in value["images"].as_array().unwrap() {
        assert_eq!(image["license"], 1);
        assert_eq!(image["flickr_url"], "");
        assert!(image["date_captured"].as_str().unwrap().contains('T'));
    }
    for annotation in value["annotations"].as_array().unwrap() {
        assert_eq!(annotation["bbox"], json!([-2, 20, 142, 40]));
        assert_eq!(annotation["area"], 5680);
    }
    Ok(())
}
#[test]
fn invalid_voc_batch_keeps_previous_json_and_cleans_temporary_publication() -> Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source");
    std::fs::create_dir(&source)?;
    std::fs::write(source.join("bad.xml"), "<wrong>")?;
    let target = root.path().join("coco.json");
    std::fs::write(&target, b"previous-json")?;
    assert!(voc::convert(&source, &target, &Control::default()).is_err());
    assert_eq!(std::fs::read(&target)?, b"previous-json");
    assert!(!std::fs::read_dir(root.path())?.any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tomato-coco-")
    }));
    Ok(())
}
