//! Docker inspect compatibility and the original partial resource-update semantics.
use super::{ManagementAction, Result, ensure};
use anyhow::Context;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    pub memory: u64,
    pub swap: i64,
}
impl Limits {
    pub fn read(value: &Value) -> Self {
        Self {
            memory: value
                .pointer("/HostConfig/Memory")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            swap: value
                .pointer("/HostConfig/MemorySwap")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        }
    }
}
pub fn inspection(text: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(text).context("无法解析容器属性")?;
    let value = match value {
        Value::Array(rows) => rows.into_iter().next().context("远端未返回容器属性")?,
        value => value,
    };
    ensure!(value.is_object(), "远端未返回有效容器属性");
    Ok(value)
}
pub fn memory_bytes(text: &str) -> Result<u64> {
    let text = text.trim();
    ensure!(
        !text.is_empty() && text.len() <= 32,
        "内存格式无效（示例: 512m / 2g / 104857600）"
    );
    let (digits, shift) = match text.as_bytes().last().unwrap().to_ascii_lowercase() {
        b'b' => (&text[..text.len() - 1], 0),
        b'k' => (&text[..text.len() - 1], 10),
        b'm' => (&text[..text.len() - 1], 20),
        b'g' => (&text[..text.len() - 1], 30),
        _ => (text, 0),
    };
    let digits = digits.trim();
    ensure!(
        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()),
        "内存格式无效（示例: 512m / 2g / 104857600）"
    );
    let bytes = digits
        .parse::<u64>()
        .ok()
        .and_then(|value| value.checked_mul(1_u64 << shift));
    bytes
        .filter(|n| *n <= i64::MAX as u64)
        .context("内存限制超出支持范围")
}
pub fn memory_text(bytes: u64) -> String {
    if bytes == 0 {
        return String::new();
    }
    for (unit, shift) in [("g", 30), ("m", 20), ("k", 10)] {
        if bytes.is_multiple_of(1_u64 << shift) {
            return format!("{}{unit}", bytes >> shift);
        }
    }
    bytes.to_string()
}
/// Empty fields leave existing settings unchanged; an explicit zero removes a limit.
pub fn resource_update(
    id: &str,
    memory: &str,
    cpus: &str,
    shares: &str,
    old: Limits,
) -> Result<Option<ManagementAction>> {
    let memory = if memory.trim().is_empty() {
        None
    } else {
        Some(memory_bytes(memory)?)
    };
    let cpus = if cpus.trim().is_empty() {
        None
    } else {
        let number = cpus.trim().parse::<f64>().context("CPU 数量需为非负数字")?;
        ensure!(number.is_finite() && number >= 0., "CPU 数量需为非负数字");
        Some(number)
    };
    let shares = if shares.trim().is_empty() {
        None
    } else {
        Some(
            shares
                .trim()
                .parse::<u32>()
                .context("CPU 份额需为非负整数")?,
        )
    };
    if memory.is_none() && cpus.is_none() && shares.is_none() {
        return Ok(None);
    }
    let memory_swap = memory
        .map(|memory| -> Result<i64> {
            if memory == 0 || old.swap == -1 {
                return Ok(-1);
            }
            let extra = if old.swap > 0 {
                (old.swap as u64).saturating_sub(old.memory)
            } else {
                0
            };
            let total = memory
                .checked_add(extra)
                .filter(|n| *n <= i64::MAX as u64)
                .context("内存与交换空间限制超出支持范围")?;
            Ok(total as i64)
        })
        .transpose()?;
    let action = ManagementAction::DockerUpdate {
        id: id.into(),
        restart: None,
        memory,
        memory_swap,
        cpus,
        shares,
    };
    super::command(&action)?;
    Ok(Some(action))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_changes_preserve_swap_allow_blank_fields_and_explicit_zero() {
        let old = Limits {
            memory: 512 << 20,
            swap: 1024 << 20,
        };
        let command =
            super::super::command(&resource_update("web", "2g", "", "", old).unwrap().unwrap())
                .unwrap();
        assert!(command.contains("--memory=2147483648 --memory-swap=2684354560"));
        assert!(!command.contains("--cpus=") && !command.contains("--cpu-shares="));
        for old in [Limits::default(), Limits { swap: -1, ..old }, old] {
            let command = super::super::command(
                &resource_update("web", "0", "0", "0", old).unwrap().unwrap(),
            )
            .unwrap();
            assert!(command.contains("--memory=0 --memory-swap=-1 --cpus=0 --cpu-shares=0"));
        }
        assert!(resource_update("web", " ", "", "", old).unwrap().is_none());
        let command = super::super::command(
            &resource_update("web", "", "1.5", "1024", old)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert!(!command.contains("--memory"));
        assert!(command.contains("--cpus=1.5 --cpu-shares=1024"));
        assert!(resource_update("web", "9223372036854775807", "", "", old).is_err());
    }
    #[test]
    fn memory_and_inspect_inputs_are_validated_without_cloning_array_contents() {
        for (input, bytes) in [
            ("512m", 512 << 20),
            ("2G", 2 << 30),
            (" 1024b ", 1024),
            ("0", 0),
        ] {
            assert_eq!(memory_bytes(input).unwrap(), bytes);
        }
        for input in [
            "-1",
            "1.5m",
            "NaN",
            "1m;id",
            "18446744073709551615g",
            "",
            "1t",
        ] {
            assert!(memory_bytes(input).is_err(), "{input}");
        }
        assert_eq!(memory_text(512 << 20), "512m");
        assert_eq!(memory_text(0), "");
        for input in ["NaN", "inf", "-1", "no"] {
            assert!(resource_update("web", "", input, "", Limits::default()).is_err());
        }
        assert!(resource_update("-bad", "1m", "", "", Limits::default()).is_err());
        for input in ["[]", "[null]", "null", "42", "not json"] {
            assert!(inspection(input).is_err());
        }
        assert_eq!(
            inspection(r#"[{"Id":"web"}]"#).unwrap(),
            inspection(r#"{"Id":"web"}"#).unwrap()
        );
    }
}
