use super::*;
use anyhow::Result;
#[tokio::test]
#[ignore = "requires TOMATO_DEV_JDK, TOMATO_DEV_MAVEN and isolated copied TOMATO_DEV_MAVEN_REPO; Maven is forced offline"]
async fn real_maven_compiles_and_runs_java_with_the_selected_profile() -> Result<()> {
    use tomato_core::development::{BuildTool, Runtime};
    let jdk = std::env::var("TOMATO_DEV_JDK")?;
    let maven = std::env::var("TOMATO_DEV_MAVEN")?;
    let repository = PathBuf::from(
        std::env::var_os("TOMATO_DEV_MAVEN_REPO")
            .ok_or_else(|| anyhow::anyhow!("missing owned Maven cache"))?,
    );
    anyhow::ensure!(
        repository.file_name().is_some_and(|n| n
            .to_string_lossy()
            .starts_with("development-maven-fixture-")),
        "must use an owned copied cache"
    );
    let dir = tempfile::tempdir()?;
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("src/main/java/demo"))?;
    std::fs::create_dir_all(root.join("src/main/resources"))?;
    std::fs::write(root.join("src/main/resources/initial.txt"), "before")?;
    std::fs::write(root.join("src/main/resources/removed.txt"), "removed")?;
    std::fs::create_dir_all(root.join(".mvn"))?;
    let settings = root.join("settings.xml");
    let escaped = repository
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;");
    std::fs::write(
        &settings,
        format!(
            "<settings><localRepository>{escaped}</localRepository><offline>true</offline></settings>"
        ),
    )?;
    std::fs::write(
        root.join(".mvn/maven.config"),
        format!("-o\n-gs\n{}\n", settings.display()),
    )?;
    std::fs::write(
        root.join("pom.xml"),
        r#"<project><modelVersion>4.0.0</modelVersion><groupId>invalid.fixture</groupId><artifactId>tomato-owned</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release><project.build.sourceEncoding>UTF-8</project.build.sourceEncoding></properties><build><plugins><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version></plugin><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-resources-plugin</artifactId><version>3.3.0</version></plugin><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-dependency-plugin</artifactId><version>2.8</version></plugin></plugins></build></project>"#,
    )?;
    std::fs::write(
        root.join("src/main/java/demo/App.java"),
        "package demo; public class App { public static void main(String[] args) { System.out.println(\"JAVA READY 中文 \"+System.getProperty(\"spring.profiles.active\")); } }",
    )?;
    let project = Project {
        path: root.to_string_lossy().into_owned(),
        project_type: "MAVEN".into(),
        jdk_id: "jdk".into(),
        maven_id: "maven".into(),
        active_profiles: vec!["fixture".into()],
        ..Default::default()
    };
    let config = Config {
        runtimes: vec![Runtime {
            id: "jdk".into(),
            kind: "JDK".into(),
            path: jdk,
            ..Default::default()
        }],
        build_tools: vec![BuildTool {
            id: "maven".into(),
            kind: "MAVEN".into(),
            home_path: maven,
            settings_path: settings.to_string_lossy().into_owned(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut processes = process::Processes::default();
    processes
        .launch(
            config.clone(),
            project.clone(),
            RunItem {
                kind: "JAVA".into(),
                target: "demo.App".into(),
                source: "src/main/java/demo/App.java".into(),
            },
            false,
        )
        .await?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let log = processes.poll(&root, 0);
        if !log.running {
            anyhow::ensure!(
                log.exit_code == Some(0) && log.text.contains("JAVA READY 中文 fixture"),
                "Maven/Java actual execution failed: {}",
                log.text
            );
            break;
        }
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "owned Maven launch timed out: {}",
            log.text
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(root.join("target/classes/demo/App.class").is_file());
    let unchanged =
        run_java_again(&mut processes, &config, &project, "JAVA READY 中文 fixture").await?;
    assert!(
        unchanged.contains("未发现 Java 类变更，跳过编译"),
        "{unchanged}"
    );
    std::fs::write(
        root.join("src/main/java/demo/App.java"),
        "package demo; public class App { public static void main(String[] args) { System.out.println(\"JAVA CHANGED 中文 \"+System.getProperty(\"spring.profiles.active\")); } }",
    )?;
    std::fs::write(root.join("src/main/resources/initial.txt"), "after-longer")?;
    std::fs::remove_file(root.join("src/main/resources/removed.txt"))?;
    std::fs::write(root.join("src/main/resources/new.txt"), "new")?;
    let changed = run_java_again(
        &mut processes,
        &config,
        &project,
        "JAVA CHANGED 中文 fixture",
    )
    .await?;
    assert!(changed.contains("增量编译 1 个变更文件"), "{changed}");
    assert_eq!(
        std::fs::read_to_string(root.join("target/classes/initial.txt"))?,
        "after-longer"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("target/classes/new.txt"))?,
        "new"
    );
    assert!(!root.join("target/classes/removed.txt").exists());
    #[cfg(windows)]
    {
        // An owned wrapper injects Maven's real cache diagnostics. Its final attempt
        // delegates to the installed Maven with the isolated, offline repository.
        let executable = std::path::Path::new(&config.build_tools[0].home_path).join("bin/mvn.cmd");
        let pom = root.join("pom.xml");
        std::fs::write(&pom, format!("{}\n", std::fs::read_to_string(&pom)?))?;
        std::fs::write(
            root.join("mvnw.cmd"),
            format!(
                "@echo off\r\nif not exist retry-first (\r\n type nul >retry-first\r\n echo cached in the local repository\r\n exit /b 1\r\n)\r\nif not exist retry-second (\r\n type nul >retry-second\r\n echo resolution is not reattempted\r\n exit /b 1\r\n)\r\ncall \"{}\" %*\r\n",
                executable.display()
            ),
        )?;
        processes
            .launch(
                config,
                project,
                RunItem {
                    kind: "JAVA".into(),
                    target: "demo.App".into(),
                    source: "src/main/java/demo/App.java".into(),
                },
                true,
            )
            .await?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        loop {
            let log = processes.poll(&root, 0);
            if !log.running {
                anyhow::ensure!(
                    log.exit_code == Some(0) && log.text.contains("JAVA CHANGED 中文 fixture"),
                    "offline fallback failed: {}",
                    log.text
                );
                assert!(log.text.contains("使用 -U 强制刷新后重试"));
                assert!(log.text.contains("尝试使用本地已有 JAR"));
                assert!(
                    log.text
                        .contains("-Daether.artifactResolver.simpleLrmInterop=true")
                );
                break;
            }
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "owned fallback timed out: {}",
                log.text
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }
    Ok(())
}
async fn run_java_again(
    processes: &mut process::Processes,
    config: &Config,
    project: &Project,
    expected: &str,
) -> Result<String> {
    processes
        .launch(
            config.clone(),
            project.clone(),
            RunItem {
                kind: "JAVA".into(),
                target: "demo.App".into(),
                source: "src/main/java/demo/App.java".into(),
            },
            true,
        )
        .await?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let log = processes.poll(std::path::Path::new(&project.path), 0);
        if !log.running {
            anyhow::ensure!(
                log.exit_code == Some(0) && log.text.contains(expected),
                "owned Java launch failed: {}",
                log.text
            );
            return Ok(log.text);
        }
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "owned Java launch timed out: {}",
            log.text
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}
#[tokio::test]
#[ignore = "requires TOMATO_DEV_NODE pointing at a real Node installation; owned temporary npm project only"]
async fn real_npm_start_restart_stop_cleans_descendants_and_keeps_new_logs() -> Result<()> {
    let node = PathBuf::from(
        std::env::var_os("TOMATO_DEV_NODE")
            .ok_or_else(|| anyhow::anyhow!("missing TOMATO_DEV_NODE"))?,
    );
    let dir = tempfile::tempdir()?;
    let root = dir.path().to_path_buf();
    std::fs::write(
        root.join("package.json"),
        r#"{"scripts":{"dev":"node app.js"}}"#,
    )?;
    std::fs::write(
        root.join("app.js"),
        "const {spawn}=require('child_process');spawn(process.execPath,['child.js'],{stdio:'ignore',windowsHide:true});console.log('READY 中文');setInterval(()=>console.log('tick'),100);",
    )?;
    std::fs::write(
        root.join("child.js"),
        "setTimeout(()=>require('fs').writeFileSync('orphan-marker','orphan'),2500);setInterval(()=>{},1000);",
    )?;
    let project = Project {
        path: root.to_string_lossy().into_owned(),
        name: "owned fixture".into(),
        node_id: "node-fixture".into(),
        project_type: "NODE".into(),
        ..Default::default()
    };
    let config = Config {
        projects: vec![project.clone()],
        runtimes: vec![tomato_core::development::Runtime {
            id: "node-fixture".into(),
            kind: "NODE".into(),
            path: node.to_string_lossy().into_owned(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let item = RunItem {
        kind: "NPM".into(),
        target: "dev".into(),
        source: PathBuf::new(),
    };
    let mut processes = process::Processes::default();
    processes
        .launch(config.clone(), project.clone(), item.clone(), false)
        .await?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !processes.poll(&root, 0).text.contains("READY 中文")
        && std::time::Instant::now() < deadline
    {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let first = processes.poll(&root, 0);
    assert!(first.text.contains("READY 中文"), "{}", first.text);
    assert!(first.running);
    assert!(
        processes
            .launch(config.clone(), project.clone(), item.clone(), false)
            .await
            .is_err()
    );
    processes.launch(config, project, item, true).await?;
    let second = processes.poll(&root, first.offset);
    assert!(second.reset);
    processes.stop(&root).await;
    assert!(!processes.poll(&root, 0).running);
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert!(
        !root.join("orphan-marker").exists(),
        "owned npm descendant escaped cleanup"
    );
    Ok(())
}
#[test]
fn original_config_legacy_arrays_nulls_unknown_fields_backup_and_external_change() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("development.json");
    let original=br#"{"projects":[{"name":"fixture","path":"local","jdkId":null,"futureProject":7}],"runtimes":[{"id":"same","type":"JDK"}],"jdks":[{"id":"same","name":"ignored"},{"id":"jdk","name":"legacy","futureRuntime":true}],"nodes":null,"buildTools":null,"futureRoot":{"x":42}}"#;
    std::fs::write(&path, original)?;
    let (mut config, revision) = repository::load(&path)?;
    assert_eq!(config.projects[0].jdk_id, "");
    assert_eq!(config.runtimes.len(), 2);
    assert_eq!(config.runtimes[1].kind, "JDK");
    assert_eq!(std::fs::read(&path)?, original);
    config.projects[0].name = "renamed".into();
    let next = repository::save(&path, &config, revision.as_deref())?;
    let (loaded, actual) = repository::load(&path)?;
    assert_eq!(actual.as_deref(), Some(next.as_str()));
    assert_eq!(loaded.projects[0].extra["futureProject"], 7);
    assert_eq!(loaded.runtimes[1].extra["futureRuntime"], true);
    assert_eq!(loaded.extra["futureRoot"]["x"], 42);
    assert_eq!(
        std::fs::read(path.with_file_name("development.json.before-rust.bak"))?,
        original
    );
    std::fs::write(&path, "[]")?;
    assert!(repository::save(&path, &config, Some(&next)).is_err());
    assert_eq!(repository::load(&path)?.0.projects.len(), 0);
    Ok(())
}
#[test]
fn project_inspection_file_operations_and_realtime_changes_use_real_files() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("src/main/java/demo"))?;
    std::fs::create_dir_all(root.join("src/main/resources"))?;
    std::fs::write(root.join("pom.xml"), "<project/>")?;
    std::fs::write(
        root.join("package.json"),
        r#"{"scripts":{"dev":"node app.js","build":"node build.js"}}"#,
    )?;
    std::fs::write(
        root.join("src/main/java/demo/App.java"),
        "package demo;\npublic class App { public static void main(String[] args) {} }",
    )?;
    std::fs::write(
        root.join("src/main/resources/application-prod.yml"),
        "spring.profiles.include: [dev, test]\non-profile: cloud | local\n",
    )?;
    let p = Project {
        path: root.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let overview = workspace::inspect(&p)?;
    assert_eq!(overview.project_type, "MAVEN_NODE");
    assert_eq!(overview.runs.len(), 3);
    assert!(overview.runs.iter().any(|r| r.target == "demo.App"));
    assert_eq!(
        overview.profiles,
        vec!["cloud", "dev", "local", "prod", "test"]
    );
    let path = root.join("src/main/resources/application-prod.yml");
    let (text, revision) = workspace::read(&root, &path)?;
    let mut snapshots = HashMap::new();
    assert!(
        version::realtime(&root, &mut snapshots, true)?
            .changes
            .is_empty()
    );
    workspace::write(&root, &path, &format!("{text}\n# edited"), &revision)?;
    assert_eq!(
        version::realtime(&root, &mut snapshots, false)?
            .changes
            .len(),
        1
    );
    assert!(workspace::write(&root, &path, "lost update", &revision).is_err());
    let outside = tempfile::NamedTempFile::new()?;
    assert!(workspace::read(&root, outside.path()).is_err());
    assert!(version::checked(&root, "../escape").is_err());
    std::fs::write(root.join("binary.dat"), [0, 255])?;
    assert!(workspace::read(&root, &root.join("binary.dat")).is_err());
    Ok(())
}
#[tokio::test]
async fn git_changes_keep_spaces_unicode_and_selected_commit_preserves_other_staged_files()
-> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    async fn git(root: &std::path::Path, args: &[&str]) -> Result<String> {
        let mut plan = process::Plan::new("git", root);
        plan.args = args.iter().map(|a| (*a).into()).collect();
        process::capture(plan, std::time::Duration::from_secs(15)).await
    }
    git(root, &["init", "-q"]).await?;
    git(root, &["config", "user.name", "Tomato fixture"]).await?;
    git(root, &["config", "user.email", "fixture@example.invalid"]).await?;
    git(root, &["config", "commit.gpgsign", "false"]).await?;
    std::fs::write(root.join("old.txt"), "baseline")?;
    git(root, &["add", "--", "old.txt"]).await?;
    git(root, &["commit", "-qm", "baseline"]).await?;
    std::fs::write(root.join("old.txt"), "other staged")?;
    git(root, &["add", "--", "old.txt"]).await?;
    std::fs::write(root.join("中文 space.txt"), "selected")?;
    let versions = version::git_changes(root).await?;
    assert!(versions.git);
    assert!(versions.changes.iter().any(|c| c.path == "中文 space.txt"));
    let canonical_path = std::fs::canonicalize(root.join("中文 space.txt"))?;
    let (_, revision) = workspace::read(root, &canonical_path)?;
    let highlights = version::highlights(root, &canonical_path, &revision).await?;
    assert_eq!(
        highlights,
        vec![tomato_core::development::LineChange {
            start: 1,
            end: 1,
            kind: tomato_core::development::ChangeKind::Added
        }]
    );

    version::execute(
        root,
        version::Action::Commit {
            files: vec!["中文 space.txt".into()],
            message: "owned fixture".into(),
            push: None,
        },
    )
    .await?;
    let head = git(root, &["show", "HEAD:old.txt"]).await?;
    assert_eq!(head, "baseline");
    assert!(
        version::git_changes(root)
            .await?
            .changes
            .iter()
            .any(|c| c.path == "old.txt")
    );
    version::execute(
        root,
        version::Action::Ignore {
            path: "中文 space.txt".into(),
            directory: false,
        },
    )
    .await?;
    assert!(std::fs::read_to_string(root.join(".gitignore"))?.contains("/中文 space.txt"));
    Ok(())
}
