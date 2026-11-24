use super::{
    RunItem,
    process::{self, Plan, Shared},
};
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tomato_core::development::{BuildTool, Config, Project, Runtime};
mod cache;
mod maven;
pub(super) use cache::Cache;
fn binary(home: &Path, name: &str) -> PathBuf {
    home.join("bin").join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    })
}
fn sdk<'a>(config: &'a Config, id: &str, kind: &str) -> Result<&'a Runtime> {
    config
        .runtimes
        .iter()
        .find(|r| r.id == id && r.kind == kind)
        .with_context(|| format!("请先在项目配置中选择 {kind}"))
}
fn tool<'a>(config: &'a Config, project: &Project, kind: &str) -> Result<&'a BuildTool> {
    config
        .build_tools
        .iter()
        .find(|r| r.id == project.maven_id && r.kind == kind)
        .with_context(|| format!("请先在项目配置中选择 {kind} 构建工具"))
}
fn java_env(plan: &mut Plan, jdk: &Runtime) -> Result<()> {
    let home = Path::new(&jdk.path);
    let bin = home.join("bin");
    ensure!(binary(home, "java").is_file(), "所选 JDK 中未找到 Java");
    plan.env.insert("JAVA_HOME".into(), jdk.path.clone());
    prepend(plan, &bin)?;
    Ok(())
}
fn prepend(plan: &mut Plan, path: &Path) -> Result<()> {
    let mut dirs = vec![path.to_owned()];
    dirs.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    plan.env.insert(
        "PATH".into(),
        std::env::join_paths(dirs)?.to_string_lossy().into_owned(),
    );
    Ok(())
}
fn builder(
    config: &Config,
    p: &Project,
    work: &Path,
    gradle: bool,
    args: Vec<String>,
) -> Result<Plan> {
    let jdk = sdk(config, &p.jdk_id, "JDK")?;
    let tool = tool(config, p, if gradle { "GRADLE" } else { "MAVEN" })?;
    let name = if gradle {
        if cfg!(windows) {
            "gradlew.bat"
        } else {
            "gradlew"
        }
    } else if cfg!(windows) {
        "mvnw.cmd"
    } else {
        "mvnw"
    };
    let wrapper = Path::new(&p.path).join(name);
    let command = if wrapper.is_file() {
        wrapper
    } else {
        Path::new(&tool.home_path).join("bin").join(if gradle {
            if cfg!(windows) {
                "gradle.bat"
            } else {
                "gradle"
            }
        } else if cfg!(windows) {
            "mvn.cmd"
        } else {
            "mvn"
        })
    };
    ensure!(command.is_file(), "构建工具不存在：{}", command.display());
    let mut plan = Plan::new(command, work);
    java_env(&mut plan, jdk)?;
    if !gradle && !tool.settings_path.trim().is_empty() {
        let configured = PathBuf::from(&tool.settings_path);
        let fallback = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".m2/settings.xml");
        let settings = if configured.is_file() {
            configured
        } else {
            fallback
        };
        ensure!(settings.is_file(), "配置的 settings.xml 不存在");
        plan.args
            .extend(["-s".into(), settings.to_string_lossy().into_owned()]);
    }
    if !gradle {
        plan.args.extend([
            "-Djansi.passthrough=true".into(),
            "-Dstyle.color=always".into(),
        ]);
    }
    plan.args.extend(args);
    Ok(plan)
}
fn module(p: &Project, item: &RunItem) -> Result<PathBuf> {
    let root = std::fs::canonicalize(&p.path)?;
    let source = super::workspace::confined(&root, &root.join(&item.source))?;
    let mut current = source.parent();
    while let Some(path) = current {
        if !path.starts_with(&root) {
            break;
        }
        if ["pom.xml", "build.gradle", "build.gradle.kts"]
            .iter()
            .any(|name| path.join(name).is_file())
        {
            return Ok(path.into());
        }
        current = path.parent();
    }
    Ok(root)
}
pub async fn execute(
    config: &Config,
    p: &Project,
    item: Option<RunItem>,
    log: &Shared,
    cancel: &CancellationToken,
    cache: &Cache,
) -> Result<i32> {
    let gradle = p.project_type.starts_with("GRADLE")
        || super::workspace::detect(Path::new(&p.path)).starts_with("GRADLE");
    let Some(item) = item else {
        let args = if gradle {
            vec!["--console=plain", "dependencies"]
        } else {
            vec!["dependency:tree"]
        };
        let plan = builder(
            config,
            p,
            Path::new(&p.path),
            gradle,
            args.into_iter().map(Into::into).collect(),
        )?;
        return Ok(
            process::run(plan, log, cancel, Some(Duration::from_secs(300)))
                .await?
                .0,
        );
    };
    if item.kind == "NPM" {
        let node = sdk(config, &p.node_id, "NODE")?;
        let path = Path::new(&node.path);
        let executable = if path.is_dir() {
            path.join(if cfg!(windows) { "node.exe" } else { "node" })
        } else {
            path.into()
        };
        ensure!(executable.is_file(), "Node 可执行文件不存在");
        let home = executable.parent().context("Node 路径缺少父目录")?;
        let cli = home.join("node_modules/npm/bin/npm-cli.js");
        let mut plan = if cli.is_file() {
            let mut plan = Plan::new(&executable, &p.path);
            plan.args.push(cli.to_string_lossy().into_owned());
            plan
        } else {
            Plan::new(
                home.join(if cfg!(windows) { "npm.cmd" } else { "npm" }),
                &p.path,
            )
        };
        let value: serde_json::Value = serde_json::from_slice(&super::workspace::read_bytes(
            &Path::new(&p.path).join("package.json"),
            1024 * 1024,
        )?)?;
        ensure!(
            value
                .get("scripts")
                .and_then(|v| v.get(&item.target))
                .is_some(),
            "npm script 已不存在"
        );
        prepend(&mut plan, home)?;
        plan.args.extend(["run".into(), item.target]);
        return Ok(process::run(plan, log, cancel, None).await?.0);
    }
    ensure!(item.kind == "JAVA", "未知运行项");
    let work = module(p, &item)?;
    let test = item
        .source
        .to_string_lossy()
        .replace('\\', "/")
        .contains("/test/java/");
    let jdk = sdk(config, &p.jdk_id, "JDK")?;
    let classpath = if gradle {
        let script = tempfile::Builder::new()
            .prefix("tomato-gradle-runtime-")
            .suffix(".gradle")
            .tempfile()?;
        std::fs::write(
            script.path(),
            "allprojects { p -> p.plugins.withId('java') { if (p.tasks.findByName('tomatoRuntimeClasspath') == null) { p.tasks.register('tomatoRuntimeClasspath') { doLast { def n = System.getProperty('tomato.sourceSet', 'main'); println 'TOMATO_CLASSPATH=' + p.sourceSets.getByName(n).runtimeClasspath.asPath } } } } }\n",
        )?;
        let args = vec![
            "--console=plain".into(),
            "-I".into(),
            script.path().to_string_lossy().into_owned(),
            format!("-Dtomato.sourceSet={}", if test { "test" } else { "main" }),
            if test {
                "testClasses".into()
            } else {
                "classes".into()
            },
            "tomatoRuntimeClasspath".into(),
        ];
        let (code, text) = process::run(
            builder(config, p, &work, true, args)?,
            log,
            cancel,
            Some(Duration::from_secs(300)),
        )
        .await?;
        ensure!(code == 0, "Gradle 编译或依赖解析失败");
        text.lines()
            .find_map(|line| line.strip_prefix("TOMATO_CLASSPATH="))
            .filter(|s| !s.trim().is_empty())
            .context("Gradle 未返回 runtimeClasspath")?
            .trim()
            .to_owned()
    } else {
        let dependency = maven::prepare(config, p, &work, test, log, cancel, cache).await?;
        let mut paths = Vec::new();
        if test {
            paths.push(work.join("target/test-classes"))
        }
        paths.push(work.join("target/classes"));
        let mut cp = std::env::join_paths(paths)?.to_string_lossy().into_owned();
        if !dependency.trim().is_empty() {
            cp.push(if cfg!(windows) { ';' } else { ':' });
            cp.push_str(dependency.trim());
        }
        cp
    };
    let mut plan = Plan::new(binary(Path::new(&jdk.path), "java"), work);
    java_env(&mut plan, jdk)?;
    plan.args = vec![
        "-XX:TieredStopAtLevel=1".into(),
        "-Dspring.output.ansi.enabled=always".into(),
        "-Dfile.encoding=UTF-8".into(),
        "-Dsun.stdout.encoding=UTF-8".into(),
        "-Dsun.stderr.encoding=UTF-8".into(),
    ];
    if !p.active_profiles.is_empty() {
        plan.args.push(format!(
            "-Dspring.profiles.active={}",
            p.active_profiles.join(",")
        ));
    }
    plan.args
        .extend(["-classpath".into(), classpath, item.target]);
    Ok(process::run(plan, log, cancel, None).await?.0)
}
pub fn version_command(kind: &str, path: &Path) -> Result<Plan> {
    let executable = if path.is_file() {
        path.into()
    } else {
        match kind {
            "JDK" => binary(path, "java"),
            "NODE" => path.join(if cfg!(windows) { "node.exe" } else { "node" }),
            "FLUTTER" => path.join("bin").join(if cfg!(windows) {
                "flutter.bat"
            } else {
                "flutter"
            }),
            "MAVEN" => path
                .join("bin")
                .join(if cfg!(windows) { "mvn.cmd" } else { "mvn" }),
            "GRADLE" => path.join("bin").join(if cfg!(windows) {
                "gradle.bat"
            } else {
                "gradle"
            }),
            _ => anyhow::bail!("未知 SDK 类型"),
        }
    };
    ensure!(
        executable.is_file(),
        "未找到可执行文件：{}",
        executable.display()
    );
    let dir = executable.parent().context("可执行路径无目录")?;
    let mut plan = Plan::new(&executable, dir);
    plan.args.push(
        if kind == "JDK" {
            "-version"
        } else {
            "--version"
        }
        .into(),
    );
    Ok(plan)
}
