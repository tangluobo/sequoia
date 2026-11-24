//! SDK/build-tool variation shared by the project dialog and its background use cases.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryRole {
    Runtime,
    BuildTool,
}
impl EntryRole {
    pub fn is_build(self) -> bool {
        self == Self::BuildTool
    }
}
pub fn build_kind(project_type: &str) -> Option<&'static str> {
    match project_type {
        "MAVEN" | "MAVEN_NODE" => Some("MAVEN"),
        "GRADLE" | "GRADLE_NODE" => Some("GRADLE"),
        _ => None,
    }
}
pub fn display_kind(kind: &str) -> &'static str {
    match kind {
        "NODE" => "Node",
        "FLUTTER" => "Flutter",
        "MAVEN" => "Maven",
        "GRADLE" => "Gradle",
        _ => "JDK",
    }
}
pub fn known_version(version: &str) -> &str {
    if version.trim().is_empty() {
        "未知版本"
    } else {
        version
    }
}
pub fn release_version(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.strip_prefix("JAVA_VERSION=")
            .map(|value| value.replace('"', "").trim().chars().take(512).collect())
    })
}
fn after<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.get(..prefix.len())
        .filter(|s| s.eq_ignore_ascii_case(prefix))
        .map(|_| line[prefix.len()..].trim())
}
/// Ignore JVM warnings and Gradle separators instead of saving them as a version.
pub fn command_version(kind: &str, output: &str) -> String {
    let output = super::AnsiText::default().push(output);
    let lines = || output.lines().map(str::trim).filter(|s| !s.is_empty());
    let numeric = |s: &&str| s.as_bytes().first().is_some_and(u8::is_ascii_digit);
    let value = match kind {
        "MAVEN" => lines()
            .find_map(|s| after(s, "Apache Maven"))
            .or_else(|| lines().find(numeric)),
        "GRADLE" => lines()
            .find_map(|s| after(s, "Gradle"))
            .or_else(|| lines().find(numeric)),
        "NODE" => lines()
            .find_map(|s| after(s, "v").filter(|v| numeric(v)))
            .or_else(|| lines().find(numeric)),
        "FLUTTER" => lines().find_map(|s| after(s, "Flutter")),
        "JDK" => lines()
            .find(|s| {
                after(s, "openjdk").is_some() || after(s, "java version").is_some() || numeric(s)
            })
            .map(|s| {
                if let Some(start) = s.find('"')
                    && let Some(end) = s[start + 1..].find('"')
                {
                    &s[start + 1..start + 1 + end]
                } else if numeric(&s) {
                    s
                } else {
                    s.split_whitespace().nth(1).unwrap_or("")
                }
            }),
        _ => None,
    };
    value.unwrap_or("").chars().take(512).collect()
}
