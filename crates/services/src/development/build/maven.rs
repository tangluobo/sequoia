//! Maven preparation follows DevelopmentManagerPane's cached-resolution recovery.
use super::*;

fn cached_resolution_failure(output: &str) -> bool {
    [
        "cached in the local repository",
        "resolution is not reattempted",
        "present, but unavailable",
    ]
    .iter()
    .any(|message| output.contains(message))
}

async fn command(
    config: &Config,
    project: &Project,
    work: &Path,
    args: Vec<String>,
    seconds: u64,
    log: &Shared,
    cancel: &CancellationToken,
) -> Result<(i32, String)> {
    process::run(
        builder(config, project, work, false, args)?,
        log,
        cancel,
        Some(Duration::from_secs(seconds)),
    )
    .await
}

pub(super) async fn prepare(
    config: &Config,
    project: &Project,
    module: &Path,
    test: bool,
    log: &Shared,
    cancel: &CancellationToken,
    cache: &Cache,
) -> Result<String> {
    let key = (module.to_owned(), test);
    let mut snapshot = super::cache::state(config, project, module, test).ok();
    if let Some(previous) = cache.take(&key)
        && let Some(current) = &mut snapshot
    {
        let build = super::cache::Build {
            config,
            project,
            module,
            test,
            log,
            cancel,
        };
        match super::cache::attempt(&previous, current, build).await {
            Ok(true) => {
                current.dependencies = previous.dependencies;
                let result = current.dependencies.clone();
                cache.put(key, snapshot.take().expect("incremental snapshot exists"));
                return Ok(result);
            }
            Ok(false) => {}
            Err(error) => process::append(
                log,
                &format!("[Javac] 增量编译不可用，回退 Maven 完整编译：{error}\n"),
            ),
        }
    }
    let goal = if test { "test-compile" } else { "compile" };
    process::append(log, "[Maven] 正在编译项目...\n");
    let mut compile = command(
        config,
        project,
        Path::new(&project.path),
        vec![goal.into()],
        300,
        log,
        cancel,
    )
    .await?;
    if compile.0 != 0 && cached_resolution_failure(&compile.1) {
        process::append(
            log,
            "[Maven] 检测到依赖失败缓存，使用 -U 强制刷新后重试...\n",
        );
        compile = command(
            config,
            project,
            Path::new(&project.path),
            vec!["-U".into(), goal.into()],
            300,
            log,
            cancel,
        )
        .await?;
    }
    let mut local = false;
    if compile.0 != 0 && cached_resolution_failure(&compile.1) {
        process::append(log, "[Maven] 远程仓库仍不可用，尝试使用本地已有 JAR...\n");
        compile = command(
            config,
            project,
            Path::new(&project.path),
            vec![
                "-o".into(),
                "-Daether.artifactResolver.simpleLrmInterop=true".into(),
                goal.into(),
            ],
            300,
            log,
            cancel,
        )
        .await?;
        local = compile.0 == 0;
    }
    ensure!(compile.0 == 0, "Maven 编译失败");
    // Release bounded command capture before starting the dependency resolver.
    drop(compile);
    let prefix = if local {
        vec![
            "-o".into(),
            "-Daether.artifactResolver.simpleLrmInterop=true".into(),
        ]
    } else {
        Vec::<String>::new()
    };
    let mut repository = prefix.clone();
    repository.extend(
        [
            "help:evaluate",
            "-Dexpression=settings.localRepository",
            "-q",
            "-DforceStdout",
        ]
        .map(String::from),
    );
    let (code, output) = command(config, project, module, repository, 120, log, cancel).await?;
    if code == 0 {
        let mut ansi = tomato_core::development::AnsiText::default();
        if let Some(repository) = ansi
            .push(&output)
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
        {
            process::append(log, &format!("[Maven] 本地仓库：{repository}\n"));
        }
    }
    let dependencies = tempfile::Builder::new()
        .prefix("tomato-maven-classpath-")
        .suffix(".txt")
        .tempfile()?;
    let mut args = prefix;
    args.extend([
        "dependency:build-classpath".into(),
        format!("-Dmdep.outputFile={}", dependencies.path().display()),
        format!(
            "-Dmdep.includeScope={}",
            if test { "test" } else { "runtime" }
        ),
    ]);
    let (code, _) = command(config, project, module, args, 180, log, cancel).await?;
    ensure!(code == 0, "Maven 无法生成依赖类路径");
    let dependencies = String::from_utf8(crate::development::workspace::read_bytes(
        dependencies.path(),
        2 * 1024 * 1024,
    )?)?
    .trim()
    .to_owned();
    if let Some(mut state) = snapshot {
        state.dependencies = dependencies.clone();
        cache.put(key, state);
    }
    Ok(dependencies)
}
