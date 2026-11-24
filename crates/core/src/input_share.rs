//! Horizontal desktop geometry and platform-independent input messages.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
pub const MAX_NODES: usize = 16;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub width: i32,
    pub height: i32,
}
pub fn validate(nodes: &[Node]) -> Result<()> {
    ensure!(
        !nodes.is_empty() && nodes.len() <= MAX_NODES,
        "节点数量应为 1–16"
    );
    for (i, n) in nodes.iter().enumerate() {
        ensure!(
            !n.id.is_empty() && n.id.len() <= 64 && n.name.len() <= 128,
            "无效节点名称"
        );
        ensure!(
            (2..=65536).contains(&n.width) && (2..=65536).contains(&n.height),
            "无效屏幕尺寸"
        );
        ensure!(!nodes[..i].iter().any(|p| p.id == n.id), "节点 ID 重复");
    }
    Ok(())
}
/// Preserve the relative vertical position when crossing adjacent screen edges.
pub fn cross(nodes: &[Node], index: usize, x: i32, y: i32) -> (usize, i32, i32) {
    let current = &nodes[index];
    let next = if x < 0 && index > 0 {
        index - 1
    } else if x >= current.width && index + 1 < nodes.len() {
        index + 1
    } else {
        return (
            index,
            x.clamp(0, current.width - 1),
            y.clamp(0, current.height - 1),
        );
    };
    let target = &nodes[next];
    (
        next,
        if next < index { target.width - 2 } else { 1 },
        ((i64::from(y.clamp(0, current.height - 1)) * i64::from(target.height - 1))
            / i64::from(current.height - 1)) as i32,
    )
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum KeyCode {
    Scan { code: u16, extended: bool },
    Virtual(u16),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Input {
    Display { width: i32, height: i32 },
    Claim,
    Move { x: i32, y: i32 },
    Button { button: u8, down: bool },
    Wheel { delta: i32, horizontal: bool },
    Key { key: KeyCode, down: bool },
    Release,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Routed {
    pub target: String,
    pub input: Input,
}
#[cfg(test)]
mod tests {
    use super::*;
    fn nodes() -> Vec<Node> {
        vec![
            Node {
                id: "a".into(),
                name: "A".into(),
                width: 1920,
                height: 1080,
            },
            Node {
                id: "b".into(),
                name: "B".into(),
                width: 2560,
                height: 1440,
            },
        ]
    }
    #[test]
    fn different_resolutions_and_outer_edges() {
        let n = nodes();
        assert_eq!(cross(&n, 0, 1920, 1079), (1, 1, 1439));
        assert_eq!(cross(&n, 1, -1, 1439), (0, 1918, 1079));
        assert_eq!(cross(&n, 0, -50, -10), (0, 0, 0));
        assert_eq!(cross(&n, 1, 2600, 2000), (1, 2559, 1439));
    }
    #[test]
    fn rejects_duplicate_and_invalid_geometry() {
        let mut n = nodes();
        n[1].id = n[0].id.clone();
        assert!(validate(&n).is_err());
        n.pop();
        n[0].height = 0;
        assert!(validate(&n).is_err());
    }
}
