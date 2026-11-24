//! Genuine Gradle builds use only an owned project, copied distribution and offline wrapper.
#![cfg(windows)]
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tomato_core::development::{BuildTool, Config, Project, Runtime};
use tomato_services::development::{Action, Manager, Output, RunItem};

async fn complete(
    manager: &mut Manager,
    config: &Path,
    root: &Path,
    dependencies: bool,
) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let Output::Log(log) = manager
            .execute(
                config.into(),
                Action::Poll {
                    root: root.into(),
                    after: 0,
                    dependencies,
                },
            )
            .await?
        else {
            anyhow::bail!("missing real process log")
        };
        if !log.running {
            ensure!(
                log.exit_code == Some(0),
                "actual Gradle process failed: {}",
                log.text
            );
            return Ok(log.text);
        }
        ensure!(
            Instant::now() < deadline,
            "owned Gradle timed out: {}",
            log.text
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires owned TOMATO_DEV_GRADLE distribution and TOMATO_DEV_JDK; actual offline main/test classes, runtimeClasspath and dependency actions"]
async fn actual_gradle_main_test_and_dependency_actions_use_the_configured_jdk() -> Result<()> {
    let gradle =
        PathBuf::from(std::env::var_os("TOMATO_DEV_GRADLE").context("missing owned Gradle")?);
    ensure!(
        gradle.ancestors().any(|p| p.file_name().is_some_and(|n| n
            .to_string_lossy()
            .starts_with("development-gradle-fixture-"))),
        "use an owned copied distribution"
    );
    let jdk = std::env::var("TOMATO_DEV_JDK")?;
    let temp = tempfile::Builder::new()
        .prefix("tomato-gradle-owned-")
        .tempdir()?;
    let root = temp.path().join("project & 原生");
    std::fs::create_dir_all(root.join("src/main/java/demo"))?;
    std::fs::create_dir_all(root.join("src/test/java/demo"))?;
    let home = temp.path().join("gradle-home");
    std::fs::create_dir_all(&home)?;
    std::fs::write(
        root.join("settings.gradle"),
        "rootProject.name = 'owned-fixture'\n",
    )?;
    std::fs::write(
        root.join("build.gradle"),
        "plugins { id 'java' }\ntasks.withType(JavaCompile).configureEach { options.encoding = 'UTF-8' }\n",
    )?;
    std::fs::write(
        root.join("gradle.properties"),
        "org.gradle.daemon=false\norg.gradle.vfs.watch=false\norg.gradle.workers.max=2\norg.gradle.jvmargs=-Xmx256m\norg.gradle.java.installations.auto-download=false\n",
    )?;
    std::fs::write(
        root.join("gradlew.bat"),
        format!(
            "@echo off\r\ncall \"{}\" --offline --no-daemon -g \"{}\" %*\r\n",
            gradle.join("bin/gradle.bat").display(),
            home.display()
        ),
    )?;
    std::fs::write(
        root.join("src/main/java/demo/Main.java"),
        "package demo; public class Main { public static String value() { return \"GRADLE 中文\"; } public static void main(String[] a) { System.out.println(value()+\" \"+System.getProperty(\"spring.profiles.active\")); } }",
    )?;
    std::fs::write(
        root.join("src/test/java/demo/TestMain.java"),
        "package demo; public class TestMain { public static void main(String[] a) { System.out.println(Main.value()+\" TEST \"+System.getProperty(\"spring.profiles.active\")); } }",
    )?;
    let project = Project {
        path: root.to_string_lossy().into_owned(),
        project_type: "GRADLE".into(),
        jdk_id: "jdk".into(),
        maven_id: "gradle".into(),
        active_profiles: vec!["fixture".into()],
        ..Default::default()
    };
    let config = Config {
        projects: vec![project.clone()],
        runtimes: vec![Runtime {
            id: "jdk".into(),
            kind: "JDK".into(),
            path: jdk,
            ..Default::default()
        }],
        build_tools: vec![BuildTool {
            id: "gradle".into(),
            kind: "GRADLE".into(),
            home_path: gradle.to_string_lossy().into_owned(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let path = temp.path().join("development.json");
    let mut manager = Manager::default();
    for (source, target, expected) in [
        (
            "src/main/java/demo/Main.java",
            "demo.Main",
            "GRADLE 中文 fixture",
        ),
        (
            "src/test/java/demo/TestMain.java",
            "demo.TestMain",
            "GRADLE 中文 TEST fixture",
        ),
    ] {
        manager
            .execute(
                path.clone(),
                Action::Launch {
                    config: Box::new(config.clone()),
                    project: project.clone(),
                    item: RunItem {
                        kind: "JAVA".into(),
                        target: target.into(),
                        source: source.into(),
                    },
                    restart: true,
                },
            )
            .await?;
        let log = complete(&mut manager, &path, &root, false).await?;
        ensure!(
            log.contains(expected),
            "Gradle did not launch the selected source set: {log}"
        );
    }
    assert!(
        root.join("build/classes/java/main/demo/Main.class")
            .is_file()
    );
    assert!(
        root.join("build/classes/java/test/demo/TestMain.class")
            .is_file()
    );
    manager
        .execute(
            path.clone(),
            Action::Dependencies {
                config: Box::new(config),
                project,
            },
        )
        .await?;
    let dependencies = complete(&mut manager, &path, &root, true).await?;
    ensure!(
        dependencies.contains("compileClasspath") && dependencies.contains("No dependencies"),
        "Gradle dependency action did not return the actual graph: {dependencies}"
    );
    manager.execute(path, Action::Shutdown).await?;
    Ok(())
}
