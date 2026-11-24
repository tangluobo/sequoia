//! Verify that incremental runs really avoid Maven, rather than silently falling back.
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tomato_core::development::{BuildTool, Config, Project, Runtime};
use tomato_services::development::{Action, Manager, Output, RunItem};

async fn launch(
    manager: &mut Manager,
    path: &Path,
    config: &Config,
    project: &Project,
    expected: &str,
) -> Result<String> {
    manager
        .execute(
            path.into(),
            Action::Launch {
                config: Box::new(config.clone()),
                project: project.clone(),
                item: RunItem {
                    kind: "JAVA".into(),
                    target: "demo.Main".into(),
                    source: "src/main/java/demo/Main.java".into(),
                },
                restart: true,
            },
        )
        .await?;
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let Output::Log(log) = manager
            .execute(
                path.into(),
                Action::Poll {
                    root: PathBuf::from(&project.path),
                    after: 0,
                    dependencies: false,
                },
            )
            .await?
        else {
            anyhow::bail!("missing process log")
        };
        if !log.running {
            ensure!(
                log.exit_code == Some(0) && log.text.contains(expected),
                "real Java launch failed: {}",
                log.text
            );
            return Ok(log.text);
        }
        ensure!(
            Instant::now() < deadline,
            "owned Maven timed out: {}",
            log.text
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
#[ignore = "requires real TOMATO_DEV_JDK/TOMATO_DEV_MAVEN and copied offline TOMATO_DEV_MAVEN_REPO; proves that javac and resource sync run without a hidden Maven fallback"]
async fn unchanged_and_changed_launches_really_skip_maven_and_sync_resources() -> Result<()> {
    let repository = PathBuf::from(
        std::env::var_os("TOMATO_DEV_MAVEN_REPO").context("missing owned Maven cache")?,
    );
    ensure!(
        repository.file_name().is_some_and(|n| n
            .to_string_lossy()
            .starts_with("development-maven-fixture-")),
        "use an owned copied Maven cache"
    );
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    std::fs::create_dir_all(root.join("src/main/java/demo"))?;
    std::fs::create_dir_all(root.join("src/main/resources"))?;
    std::fs::create_dir(root.join(".mvn"))?;
    let settings = root.join("settings.xml");
    std::fs::write(
        &settings,
        format!(
            "<settings><localRepository>{}</localRepository><offline>true</offline></settings>",
            repository
                .to_string_lossy()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
        ),
    )?;
    std::fs::write(
        root.join(".mvn/maven.config"),
        format!("-o\n-gs\n{}\n", settings.display()),
    )?;
    std::fs::write(
        root.join("pom.xml"),
        r#"<project><modelVersion>4.0.0</modelVersion><groupId>invalid.fixture</groupId><artifactId>incremental</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release><project.build.sourceEncoding>UTF-8</project.build.sourceEncoding></properties><build><plugins><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version></plugin><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-resources-plugin</artifactId><version>3.3.0</version></plugin><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-dependency-plugin</artifactId><version>2.8</version></plugin></plugins></build></project>"#,
    )?;
    let source = root.join("src/main/java/demo/Main.java");
    std::fs::write(
        &source,
        "package demo; public class Main { public static void main(String[] a) { System.out.println(\"INITIAL 中文\"); } }",
    )?;
    std::fs::write(root.join("src/main/resources/change.txt"), "old")?;
    std::fs::write(root.join("src/main/resources/remove.txt"), "removed")?;
    let project = Project {
        path: root.to_string_lossy().into_owned(),
        project_type: "MAVEN".into(),
        jdk_id: "jdk".into(),
        maven_id: "maven".into(),
        ..Default::default()
    };
    let config = Config {
        projects: vec![project.clone()],
        runtimes: vec![Runtime {
            id: "jdk".into(),
            kind: "JDK".into(),
            path: std::env::var("TOMATO_DEV_JDK")?,
            ..Default::default()
        }],
        build_tools: vec![BuildTool {
            id: "maven".into(),
            kind: "MAVEN".into(),
            home_path: std::env::var("TOMATO_DEV_MAVEN")?,
            settings_path: settings.to_string_lossy().into_owned(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let path = root.join("development.json");
    let mut manager = Manager::default();
    let initial = launch(&mut manager, &path, &config, &project, "INITIAL 中文").await?;
    ensure!(
        initial.contains("dependency:build-classpath"),
        "initial run must resolve its real classpath"
    );
    let unchanged = launch(&mut manager, &path, &config, &project, "INITIAL 中文").await?;
    ensure!(
        unchanged.contains("未发现 Java 类变更，跳过编译")
            && !unchanged.contains("dependency:build-classpath")
            && !unchanged.contains("正在编译项目"),
        "unchanged launch did not skip Maven: {unchanged}"
    );
    std::fs::write(
        &source,
        "package demo; public class Main { public static void main(String[] a) { System.out.println(\"CHANGED NATIVE 中文\"); } }",
    )?;
    std::fs::write(root.join("src/main/resources/change.txt"), "changed native")?;
    std::fs::write(root.join("src/main/resources/new.txt"), "new")?;
    std::fs::remove_file(root.join("src/main/resources/remove.txt"))?;
    let changed = launch(
        &mut manager,
        &path,
        &config,
        &project,
        "CHANGED NATIVE 中文",
    )
    .await?;
    ensure!(
        changed.contains("增量编译 1 个变更文件")
            && changed.contains("javac")
            && !changed.contains("dependency:build-classpath")
            && !changed.contains("正在编译项目"),
        "changed launch silently fell back to Maven: {changed}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("target/classes/change.txt"))?,
        "changed native"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("target/classes/new.txt"))?,
        "new"
    );
    assert!(!root.join("target/classes/remove.txt").exists());
    manager.execute(path, Action::Shutdown).await?;
    Ok(())
}
