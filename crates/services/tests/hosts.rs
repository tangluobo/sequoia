use anyhow::Result;
use tomato_services::hosts::{self, Action, Config, Group};

#[test]
fn legacy_hosts_groups_apply_rename_delete_and_conflicts() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("hosts_groups.json");
    let system = dir.path().join("hosts");
    std::fs::write(
        &config,
        r#"{"groups":[{"name":"开发","enabled":false,"entries":[{"ip":"127.0.0.2","domain":"dev.test alias","future":2}],"color":"blue"}],"future":{"x":1}}"#,
    )?;
    std::fs::write(
        &system,
        "# user\n127.0.0.1 localhost\n# development\nkeep\n# End development\n",
    )?;
    let old = hosts::execute(&config, &system, Action::Load)?;
    let mut next = old.config.clone();
    next.groups[0].enabled = true;
    let enabled = hosts::execute(
        &config,
        &system,
        Action::Save {
            previous: Box::new(old.clone()),
            config: next,
            system: None,
            apply: true,
        },
    )?;
    assert!(
        enabled
            .system
            .contains("# 开发\n127.0.0.2 dev.test alias\n# End 开发")
    );
    assert!(
        enabled
            .system
            .contains("# development\nkeep\n# End development")
    );
    assert_eq!(enabled.config.extra, old.config.extra);
    let encoded = std::fs::read_to_string(&config)?;
    assert!(encoded.contains("\"groups\":[") && encoded.contains("\"entries\":["));
    assert!(encoded.contains("\"name\":\"开发\"") && encoded.contains("\"ip\":\"127.0.0.2\""));
    assert_eq!(enabled.config.groups[0].entries[0].extra["future"], 2);
    let mut next = enabled.config.clone();
    next.groups[0].name = "测试".into();
    let renamed = hosts::execute(
        &config,
        &system,
        Action::Save {
            previous: Box::new(enabled),
            config: next,
            system: None,
            apply: true,
        },
    )?;
    assert!(!renamed.system.contains("# 开发"));
    assert!(renamed.system.contains("# 测试"));
    let mut next = renamed.config.clone();
    next.groups.clear();
    let removed = hosts::execute(
        &config,
        &system,
        Action::Save {
            previous: Box::new(renamed),
            config: next,
            system: None,
            apply: true,
        },
    )?;
    assert!(!removed.system.contains("# 测试"));
    assert!(config.with_extension("json.bak").exists());
    std::fs::write(&system, "external")?;
    assert!(
        hosts::execute(
            &config,
            &system,
            Action::Save {
                previous: Box::new(removed.clone()),
                config: removed.config.clone(),
                system: Some("overwrite".into()),
                apply: false
            }
        )
        .is_err()
    );
    assert_eq!(std::fs::read_to_string(&system)?, "external");
    std::fs::write(&config, "external broken JSON")?;
    assert!(hosts::execute(&config, &system, Action::Load).is_err());
    assert_eq!(std::fs::read_to_string(&config)?, "external broken JSON");
    Ok(())
}

#[test]
fn unmatched_and_prefix_markers_are_preserved() {
    let config = Config {
        groups: vec![Group {
            name: "dev".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let input = "# dev\nunclosed\n# development\nkeep\n# End development";
    assert_eq!(hosts::compose(input, &config, &Config::default()), input);
    let entries =
        hosts::parse_entries("# ignore\n 127.0.0.1\t中文.test alias # comment\ninvalid\n");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].domain, "中文.test alias # comment");
}
