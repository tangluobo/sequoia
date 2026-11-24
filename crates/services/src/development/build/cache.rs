//! Bounded Maven/Javac state. File scans and resource copies run in the build task.
use super::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::SystemTime,
};

type Files = BTreeMap<PathBuf, (SystemTime, u64)>;
type Key = (PathBuf, bool);
#[derive(Default, Clone)]
pub(crate) struct Cache(Arc<Mutex<BTreeMap<Key, State>>>);
pub(super) struct State {
    configuration: String,
    sources: Files,
    main_resources: Files,
    test_resources: Files,
    pub dependencies: String,
}
impl State {
    fn count(&self) -> usize {
        self.sources.len() + self.main_resources.len() + self.test_resources.len()
    }
}
impl Cache {
    pub fn clear(&self) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }
    pub(super) fn take(&self, key: &Key) -> Option<State> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).remove(key)
    }
    pub(super) fn put(&self, key: Key, state: State) {
        let count = state.count();
        if count > 20_000 {
            return;
        }
        let mut states = self.0.lock().unwrap_or_else(|p| p.into_inner());
        while !states.is_empty()
            && (states.len() >= 8
                || states.values().map(State::count).sum::<usize>() + count > 20_000)
        {
            states.pop_first();
        }
        states.insert(key, state);
    }
}
fn snapshot(root: &Path, java: bool) -> Result<Files> {
    let mut files = Files::new();
    if root.is_dir() {
        crate::development::workspace::walk(root, |path, metadata| {
            if java == path.extension().is_some_and(|ext| ext == "java") {
                ensure!(files.len() < 20_000, "增量文件快照超过限制");
                files.insert(
                    path.strip_prefix(root)?.into(),
                    (metadata.modified()?, metadata.len()),
                );
            }
            Ok(())
        })?;
    }
    Ok(files)
}
pub(super) fn state(
    config: &Config,
    project: &Project,
    module: &Path,
    test: bool,
) -> Result<State> {
    let mut sources = Files::new();
    for part in if test {
        &["main", "test"][..]
    } else {
        &["main"][..]
    } {
        for (path, stamp) in snapshot(&module.join(format!("src/{part}/java")), true)? {
            sources.insert(PathBuf::from(format!("src/{part}/java")).join(path), stamp);
        }
    }
    let mut configuration = serde_json::to_vec(&(
        sdk(config, &project.jdk_id, "JDK")?,
        tool(config, project, "MAVEN")?,
        &project.active_profiles,
    ))?;
    crate::development::workspace::walk(Path::new(&project.path), |path, metadata| {
        if path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("pom.xml"))
        {
            configuration.extend_from_slice(path.to_string_lossy().as_bytes());
            configuration.extend_from_slice(
                format!("{:?}:{}", metadata.modified()?, metadata.len()).as_bytes(),
            );
        }
        Ok(())
    })?;
    if let Some(tool) = config.build_tools.iter().find(|t| t.id == project.maven_id) {
        for path in [
            PathBuf::from(&tool.settings_path),
            Path::new(&project.path).join(".mvn/maven.config"),
            Path::new(&project.path).join(".mvn/jvm.config"),
            Path::new(&project.path).join(if cfg!(windows) { "mvnw.cmd" } else { "mvnw" }),
        ] {
            if path.is_file() {
                configuration.extend_from_slice(&crate::development::workspace::read_bytes(
                    &path,
                    2 * 1024 * 1024,
                )?);
            }
        }
    }
    Ok(State {
        configuration: crate::development::repository::digest(&configuration),
        sources,
        main_resources: snapshot(&module.join("src/main/resources"), false)?,
        test_resources: if test {
            snapshot(&module.join("src/test/resources"), false)?
        } else {
            Files::new()
        },
        dependencies: String::new(),
    })
}
fn changed<'a>(previous: &Files, current: &'a Files) -> Vec<&'a PathBuf> {
    current
        .iter()
        .filter(|(p, stamp)| previous.get(*p) != Some(*stamp))
        .map(|(p, _)| p)
        .collect()
}
fn resources(previous: &Files, current: &Files, source: &Path, output: &Path) -> Result<()> {
    // Validate existing ancestors, including junctions, before writing build output.
    let root = std::fs::canonicalize(output)?;
    let target = |relative: &Path| -> Result<PathBuf> {
        let path = root.join(relative);
        let mut ancestor = path.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().context("资源路径无父目录")?;
        }
        ensure!(
            std::fs::canonicalize(ancestor)?.starts_with(&root),
            "资源输出越出构建目录"
        );
        Ok(path)
    };
    for old in previous.keys().filter(|p| !current.contains_key(*p)) {
        let path = target(old)?;
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    for path in changed(previous, current) {
        let target = target(path)?;
        std::fs::create_dir_all(target.parent().context("资源输出缺少父目录")?)?;
        std::fs::copy(source.join(path), target)?;
    }
    Ok(())
}
pub(super) struct Build<'a> {
    pub config: &'a Config,
    pub project: &'a Project,
    pub module: &'a Path,
    pub test: bool,
    pub log: &'a Shared,
    pub cancel: &'a CancellationToken,
}
pub(super) async fn attempt(previous: &State, current: &State, build: Build<'_>) -> Result<bool> {
    let Build {
        config,
        project,
        module,
        test,
        log,
        cancel,
    } = build;
    if previous.configuration != current.configuration
        || previous
            .sources
            .keys()
            .any(|p| !current.sources.contains_key(p))
    {
        return Ok(false);
    }
    let output = module.join("target/classes");
    if !output.is_dir() || test && !module.join("target/test-classes").is_dir() {
        return Ok(false);
    }
    let mut class = false;
    crate::development::workspace::walk(&output, |p, _| {
        class |= p.extension().is_some_and(|e| e == "class");
        Ok(())
    })?;
    if !class {
        return Ok(false);
    }
    let files = changed(&previous.sources, &current.sources);
    if files.len() > 24 {
        process::append(log, "[Javac] 变更文件较多，回退 Maven 完整编译...\n");
        return Ok(false);
    }
    process::append(
        log,
        &if files.is_empty() {
            "[Javac] 未发现 Java 类变更，跳过编译\n".into()
        } else {
            format!("[Javac] 增量编译 {} 个变更文件...\n", files.len())
        },
    );
    for source_set in ["main", "test"].into_iter().take(if test { 2 } else { 1 }) {
        let prefix = format!("src/{source_set}/java");
        let selected: Vec<_> = files.iter().filter(|p| p.starts_with(&prefix)).collect();
        if selected.is_empty() {
            continue;
        }
        let jdk = sdk(config, &project.jdk_id, "JDK")?;
        let mut plan = Plan::new(binary(Path::new(&jdk.path), "javac"), module);
        java_env(&mut plan, jdk)?;
        let mut outputs = vec![];
        if source_set == "test" {
            outputs.push(module.join("target/test-classes"));
        }
        outputs.push(output.clone());
        let mut cp = std::env::join_paths(outputs)?
            .to_string_lossy()
            .into_owned();
        if !previous.dependencies.is_empty() {
            cp.push(if cfg!(windows) { ';' } else { ':' });
            cp.push_str(&previous.dependencies);
        }
        let mut roots = vec![module.join("src/main/java")];
        if test {
            roots.push(module.join("src/test/java"));
        }
        plan.args = vec![
            "-encoding".into(),
            "UTF-8".into(),
            "-parameters".into(),
            "-proc:full".into(),
            "-classpath".into(),
            cp,
            "-d".into(),
            module
                .join(if source_set == "test" {
                    "target/test-classes"
                } else {
                    "target/classes"
                })
                .to_string_lossy()
                .into_owned(),
            "-sourcepath".into(),
            std::env::join_paths(roots)?.to_string_lossy().into_owned(),
        ];
        plan.args.extend(
            selected
                .iter()
                .map(|p| module.join(p).to_string_lossy().into_owned()),
        );
        if process::run(plan, log, cancel, Some(Duration::from_secs(180)))
            .await?
            .0
            != 0
        {
            return Ok(false);
        }
    }
    resources(
        &previous.main_resources,
        &current.main_resources,
        &module.join("src/main/resources"),
        &output,
    )?;
    if test {
        resources(
            &previous.test_resources,
            &current.test_resources,
            &module.join("src/test/resources"),
            &module.join("target/test-classes"),
        )?;
    }
    Ok(true)
}
