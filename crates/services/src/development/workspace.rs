use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tomato_core::development::Project;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunItem {
    pub kind: String,
    pub target: String,
    pub source: PathBuf,
}
impl RunItem {
    pub fn label(&self) -> String {
        if self.kind == "JAVA" {
            self.target
                .rsplit(['.', '$'])
                .next()
                .unwrap_or(&self.target)
                .into()
        } else {
            format!("npm: {}", self.target)
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
}
#[derive(Debug, Serialize)]
pub struct Overview {
    pub root: PathBuf,
    pub project_type: String,
    pub runs: Vec<RunItem>,
    pub profiles: Vec<String>,
}
pub fn confined(root: &Path, path: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(root)?;
    let path = fs::canonicalize(path)?;
    ensure!(path.starts_with(root), "文件不在所选项目内");
    Ok(path)
}
pub fn read_bytes(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "文件超出大小限制");
    Ok(bytes)
}
pub fn read(root: &Path, path: &Path) -> Result<(String, String)> {
    let path = confined(root, path)?;
    let bytes = read_bytes(&path, 8 * 1024 * 1024)?;
    ensure!(
        !bytes.contains(&0),
        "此文件是二进制文件，无法在代码编辑器中打开"
    );
    let revision = super::repository::digest(&bytes);
    Ok((String::from_utf8(bytes)?, revision))
}
pub fn write(root: &Path, path: &Path, text: &str, revision: &str) -> Result<String> {
    ensure!(text.len() <= 8 * 1024 * 1024, "代码文件超过 8 MiB");
    let path = confined(root, path)?;
    let bytes = read_bytes(&path, 8 * 1024 * 1024)?;
    ensure!(
        super::repository::digest(&bytes) == revision,
        "文件已被其他程序修改，编辑内容已保留"
    );
    let permissions = fs::metadata(&path)?.permissions();
    tomato_core::config::atomic_write(&path, text.as_bytes())?;
    fs::set_permissions(&path, permissions)?;
    Ok(super::repository::digest(text.as_bytes()))
}
pub fn list(root: &Path, path: &Path) -> Result<Vec<Entry>> {
    let actual = confined(root, path)?;
    let mut result = Vec::new();
    for entry in fs::read_dir(actual)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        ensure!(result.len() < 10000, "目录超过 10000 项");
        result.push(Entry {
            path: path.join(entry.file_name()),
            name: entry.file_name().to_string_lossy().into_owned(),
            directory: kind.is_dir(),
        });
    }
    result.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(result)
}
pub fn detect(root: &Path) -> String {
    let any = |names: &[&str]| names.iter().any(|name| root.join(name).is_file());
    let maven = any(&["pom.xml", "mvnw", "mvnw.cmd"]);
    let gradle = any(&["build.gradle", "build.gradle.kts", "gradlew", "gradlew.bat"]);
    let node = root.join("package.json").is_file();
    if root.join("pubspec.yaml").is_file() {
        "FLUTTER"
    } else if maven && node {
        "MAVEN_NODE"
    } else if maven {
        "MAVEN"
    } else if gradle && node {
        "GRADLE_NODE"
    } else if gradle {
        "GRADLE"
    } else if node {
        "NODE"
    } else {
        "GENERAL"
    }
    .into()
}
pub fn walk(root: &Path, mut visit: impl FnMut(&Path, &fs::Metadata) -> Result<()>) -> Result<()> {
    let mut stack = vec![root.to_owned()];
    let mut count = 0;
    while let Some(path) = stack.pop() {
        for item in fs::read_dir(path)? {
            let item = item?;
            let kind = item.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            count += 1;
            ensure!(count <= 100_000, "项目扫描超过 100000 项");
            if kind.is_dir() {
                stack.push(item.path())
            } else if kind.is_file() {
                visit(&item.path(), &item.metadata()?)?
            }
        }
    }
    Ok(())
}
pub fn inspect(project: &Project) -> Result<Overview> {
    let root = PathBuf::from(&project.path);
    ensure!(root.is_dir(), "开发目录不存在");
    let mut runs = Vec::new();
    let mut profiles = BTreeSet::new();
    let mut profile_files = 0;
    if root.join("package.json").is_file() {
        let value: serde_json::Value =
            serde_json::from_slice(&read_bytes(&root.join("package.json"), 1024 * 1024)?)?;
        if let Some(scripts) = value.get("scripts").and_then(|s| s.as_object()) {
            for (name, _) in scripts {
                runs.push(RunItem {
                    kind: "NPM".into(),
                    target: name.clone(),
                    source: PathBuf::new(),
                });
            }
        }
    }
    let detected = detect(&root);
    let java_project = detected.starts_with("MAVEN") || detected.starts_with("GRADLE");
    if java_project {
        walk(&root, |path, _| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let normalized = path.to_string_lossy().replace('\\', "/");
            if name.ends_with(".java")
                && runs.iter().filter(|r| r.kind == "JAVA").count() < 100
                && (normalized.contains("/src/main/java/")
                    || normalized.contains("/src/test/java/"))
                && let Ok(source) =
                    read_bytes(path, 1024 * 1024).and_then(|bytes| Ok(String::from_utf8(bytes)?))
                && let Some(target) = tomato_core::development::java_main_class(&source, &name)
            {
                runs.push(RunItem {
                    kind: "JAVA".into(),
                    target,
                    source: path.strip_prefix(&root)?.into(),
                });
            }
            if profile_files < 200
                && (normalized.contains("/src/main/resources/")
                    || normalized.contains("/src/test/resources/"))
                && name.to_lowercase().starts_with("application")
                && ["yml", "yaml", "properties"].contains(
                    &path
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_ref(),
                )
            {
                profile_files += 1;
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                if let Some(profile) = stem.strip_prefix("application-") {
                    add_profiles(profile, &mut profiles)
                }
                if let Ok(text) = String::from_utf8(read_bytes(path, 1024 * 1024)?) {
                    for line in text.lines() {
                        let line = line.trim();
                        for key in [
                            "spring.profiles.active",
                            "spring.profiles.include",
                            "spring.config.activate.on-profile",
                            "on-profile",
                        ] {
                            if let Some(rest) = line
                                .strip_prefix(key)
                                .and_then(|s| s.trim_start().strip_prefix([':', '=']))
                            {
                                add_profiles(rest.split('#').next().unwrap_or(""), &mut profiles)
                            }
                        }
                    }
                }
            }
            Ok(())
        })?;
    }
    runs.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.target.cmp(&b.target)));
    Ok(Overview {
        project_type: detect(&root),
        root,
        runs,
        profiles: profiles.into_iter().collect(),
    })
}
fn add_profiles(raw: &str, profiles: &mut BTreeSet<String>) {
    if raw.contains("${") {
        return;
    }
    for item in raw.split([',', '|']) {
        let value = item.trim().trim_matches(['[', ']', '\'', '"']).trim();
        if !value.is_empty() {
            profiles.insert(value.into());
        }
    }
}
