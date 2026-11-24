//! Original DevelopmentConfigManager schema; unknown fields survive edits.
pub mod log;
pub mod runtime;

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
type Extra = BTreeMap<String, serde_json::Value>;
fn text<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}
fn list<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Project {
    #[serde(deserialize_with = "text")]
    pub name: String,
    #[serde(deserialize_with = "text")]
    pub path: String,
    #[serde(deserialize_with = "text")]
    pub project_type: String,
    #[serde(deserialize_with = "text")]
    pub jdk_id: String,
    #[serde(deserialize_with = "text")]
    pub node_id: String,
    #[serde(deserialize_with = "text")]
    pub flutter_id: String,
    #[serde(deserialize_with = "text")]
    pub maven_id: String,
    #[serde(deserialize_with = "text")]
    pub main_class: String,
    #[serde(deserialize_with = "text")]
    pub main_source_path: String,
    #[serde(deserialize_with = "list")]
    pub active_profiles: Vec<String>,
    #[serde(deserialize_with = "text")]
    pub run_type: String,
    #[serde(deserialize_with = "text")]
    pub run_target: String,
    #[serde(flatten)]
    pub extra: Extra,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Runtime {
    #[serde(deserialize_with = "text")]
    pub id: String,
    #[serde(deserialize_with = "text")]
    pub name: String,
    #[serde(deserialize_with = "text")]
    pub path: String,
    #[serde(deserialize_with = "text")]
    pub version: String,
    #[serde(rename = "type", deserialize_with = "text")]
    pub kind: String,
    #[serde(flatten)]
    pub extra: Extra,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct BuildTool {
    #[serde(deserialize_with = "text")]
    pub id: String,
    #[serde(rename = "type", deserialize_with = "text")]
    pub kind: String,
    #[serde(deserialize_with = "text")]
    pub name: String,
    #[serde(deserialize_with = "text")]
    pub home_path: String,
    #[serde(deserialize_with = "text")]
    pub version: String,
    #[serde(deserialize_with = "text")]
    pub settings_path: String,
    #[serde(flatten)]
    pub extra: Extra,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    #[serde(deserialize_with = "list")]
    pub projects: Vec<Project>,
    #[serde(deserialize_with = "list")]
    pub runtimes: Vec<Runtime>,
    #[serde(deserialize_with = "list")]
    pub build_tools: Vec<BuildTool>,
    #[serde(flatten)]
    pub extra: Extra,
}
impl Config {
    pub fn parse(bytes: &[u8]) -> anyhow::Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        let mut config: Self = if value.is_array() {
            Self {
                projects: serde_json::from_value(value)?,
                ..Default::default()
            }
        } else {
            serde_json::from_value(value)?
        };
        for (key, kind) in [("jdks", "JDK"), ("nodes", "NODE")] {
            if let Some(value) = config.extra.remove(key).filter(|v| !v.is_null()) {
                for mut runtime in serde_json::from_value::<Vec<Runtime>>(value)? {
                    if !config.runtimes.iter().any(|r| r.id == runtime.id) {
                        runtime.kind = kind.into();
                        config.runtimes.push(runtime);
                    }
                }
            }
        }
        Ok(config)
    }
}
pub fn project_type_label(kind: &str) -> &str {
    match kind {
        "MAVEN" => "Maven",
        "NODE" => "Node",
        "MAVEN_NODE" => "Maven + Node",
        "GRADLE" => "Gradle",
        "GRADLE_NODE" => "Gradle + Node",
        "FLUTTER" => "Flutter",
        _ => "普通目录",
    }
}

pub fn java_main_class(source: &str, filename: &str) -> Option<String> {
    use std::sync::OnceLock;
    static MAIN: OnceLock<regex::Regex> = OnceLock::new();
    static PACKAGE: OnceLock<regex::Regex> = OnceLock::new();
    if !MAIN
        .get_or_init(|| {
            regex::Regex::new(r"\bpublic\s+static\s+void\s+main\s*\(").expect("static main regex")
        })
        .is_match(source)
    {
        return None;
    }
    let class = filename.strip_suffix(".java")?;
    let package = PACKAGE
        .get_or_init(|| {
            regex::Regex::new(r"(?m)^\s*package\s+([A-Za-z0-9_.]+)\s*;")
                .expect("static package regex")
        })
        .captures(source);
    Some(package.map_or_else(|| class.into(), |c| format!("{}.{class}", &c[1])))
}

#[derive(Default)]
pub struct AnsiText {
    state: u8,
}
impl AnsiText {
    pub fn push(&mut self, text: &str) -> String {
        let mut result = String::new();
        for c in text.chars() {
            match self.state {
                0 => {
                    if c == '\u{1b}' {
                        self.state = 1
                    } else {
                        result.push(c)
                    }
                }
                1 => {
                    self.state = match c {
                        '[' => 2,
                        ']' => 3,
                        _ => 0,
                    }
                }
                2 => {
                    if ('@'..='~').contains(&c) {
                        self.state = 0
                    }
                }
                3 => {
                    if c == '\u{7}' {
                        self.state = 0
                    } else if c == '\u{1b}' {
                        self.state = 4
                    }
                }
                _ => self.state = if c == '\\' { 0 } else { 3 },
            }
        }
        result
    }
}
pub fn dependency_lines(text: &str) -> Vec<(usize, String)> {
    let mut strip = AnsiText::default();
    let text = strip.push(text);
    let mut result = Vec::new();
    for line in text.lines() {
        let maven = line.starts_with("[INFO] ");
        let line = line.strip_prefix("[INFO] ").unwrap_or(line);
        let marker = if maven {
            line.find("+-").or_else(|| line.find("\\-"))
        } else {
            line.find("+---").or_else(|| line.find("\\---"))
        };
        if let Some(at) = marker {
            if !line[..at].chars().all(|c| matches!(c, ' ' | '|')) {
                continue;
            }
            let skip = if maven { 2 } else { 4 };
            let label = line[at + skip..].trim();
            if !label.is_empty() {
                result.push((at / if maven { 3 } else { 5 } + 1, label.into()));
            }
        }
    }
    result
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineChange {
    pub start: usize,
    pub end: usize,
    pub kind: ChangeKind,
}
pub fn diff_highlights(text: &str) -> Vec<LineChange> {
    static HUNK: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = HUNK.get_or_init(|| {
        regex::Regex::new(r"^@@ -\d+(?:,(\d+))? \+(\d+)(?:,(\d+))? @@.*$")
            .expect("static diff regex")
    });
    let mut result = Vec::new();
    for line in text.split([
        '\r', '\n', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}',
    ]) {
        let Some(c) = pattern.captures(line) else {
            continue;
        };
        let old = c
            .get(1)
            .map_or(Some(1), |m| m.as_str().parse::<usize>().ok());
        let new = c
            .get(3)
            .map_or(Some(1), |m| m.as_str().parse::<usize>().ok());
        let start = c[2].parse::<usize>().ok();
        let (Some(old), Some(new), Some(start)) = (old, new, start) else {
            continue;
        };
        if result.len() >= 100_000 {
            break;
        }
        if new == 0 {
            result.push(LineChange {
                start: start.max(1),
                end: start.max(1),
                kind: ChangeKind::Deleted,
            });
            continue;
        }
        let common = if old == 0 { new } else { old.min(new) };
        if common > 0 {
            result.push(LineChange {
                start,
                end: start.saturating_add(common - 1),
                kind: if old == 0 {
                    ChangeKind::Added
                } else {
                    ChangeKind::Modified
                },
            });
        }
        if common < new {
            result.push(LineChange {
                start: start.saturating_add(common),
                end: start.saturating_add(new - 1),
                kind: ChangeKind::Added,
            });
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_java_git_diff_highlights_match_compact_ranges() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/development/diff-cases.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let mut actual = Vec::new();
            for change in diff_highlights(case["diff"].as_str().unwrap()) {
                for line in change.start..=change.end {
                    actual.push(serde_json::json!({"line":line,"kind":match change.kind{ChangeKind::Added=>"ADDED",ChangeKind::Modified=>"MODIFIED",ChangeKind::Deleted=>"DELETED"}}));
                }
            }
            assert_eq!(serde_json::json!(actual), case["changes"]);
        }
    }
    #[test]
    fn actual_java_config_migration_goldens_match() {
        for (input, expected) in [
            (
                include_str!("../tests/fixtures/development/config-0.input.json"),
                include_str!("../tests/fixtures/development/config-0.expected.json"),
            ),
            (
                include_str!("../tests/fixtures/development/config-1.input.json"),
                include_str!("../tests/fixtures/development/config-1.expected.json"),
            ),
            (
                include_str!("../tests/fixtures/development/config-2.input.json"),
                include_str!("../tests/fixtures/development/config-2.expected.json"),
            ),
        ] {
            assert_eq!(
                Config::parse(input.as_bytes()).unwrap(),
                Config::parse(expected.as_bytes()).unwrap()
            );
        }
    }
    #[test]
    fn original_main_pattern_profiles_and_ansi_dependency_output() {
        assert_eq!(
            java_main_class("package a.b;\npublic\tstatic\nvoid main (", "App.java").as_deref(),
            Some("a.b.App")
        );
        assert!(java_main_class("public static final void main(", "App.java").is_none());
        let mut ansi = AnsiText::default();
        assert_eq!(ansi.push("a\u{1b}[3"), "a");
        assert_eq!(ansi.push("2mb\u{1b}[0m"), "b");
        assert_eq!(
            dependency_lines(
                "\u{1b}[32m[INFO] +- a:b:1\u{1b}[0m\n[INFO] |  \\- c:d:2\n+--- e:f:3\n|    \\--- g:h:4"
            ),
            vec![
                (1, "a:b:1".into()),
                (2, "c:d:2".into()),
                (1, "e:f:3".into()),
                (2, "g:h:4".into())
            ]
        );
    }
}
