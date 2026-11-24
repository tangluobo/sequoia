//! Bounded TBinaryProtocol for the Apache Hive TCLIService wire contract.
use anyhow::{Context, Result, bail, ensure};
use futures_util::future::BoxFuture;
use tokio::io::{AsyncRead, AsyncReadExt};
const LIMIT: usize = 8 * 1024 * 1024;

// Do not derive Debug: session handles and OpenSession may contain credentials.
#[derive(Clone)]
pub(super) enum Value {
    Bool(bool),
    Byte(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    Double(f64),
    Bytes(Vec<u8>),
    Struct(Vec<(i16, Value)>),
    List(u8, Vec<Value>),
    Map(u8, u8, Vec<(Value, Value)>),
}
impl Value {
    pub fn object(fields: impl IntoIterator<Item = (i16, Value)>) -> Self {
        Self::Struct(fields.into_iter().collect())
    }
    pub fn string(value: impl AsRef<str>) -> Self {
        Self::Bytes(value.as_ref().as_bytes().to_owned())
    }
    pub fn field(&self, id: i16) -> Option<&Value> {
        match self {
            Self::Struct(fields) => fields
                .iter()
                .find(|(key, _)| *key == id)
                .map(|(_, value)| value),
            _ => None,
        }
    }
    pub fn required(&self, id: i16) -> Result<&Value> {
        self.field(id)
            .with_context(|| format!("Hive 响应缺少字段 {id}"))
    }
    pub fn into_field(self, id: i16) -> Result<Value> {
        if let Self::Struct(fields) = self {
            return fields
                .into_iter()
                .find(|(key, _)| *key == id)
                .map(|(_, value)| value)
                .with_context(|| format!("Hive 响应缺少字段 {id}"));
        }
        bail!("Hive 响应字段不是结构")
    }
    pub fn integer(&self) -> Result<i64> {
        match self {
            Self::Byte(n) => Ok(i64::from(*n)),
            Self::I16(n) => Ok(i64::from(*n)),
            Self::I32(n) => Ok(i64::from(*n)),
            Self::I64(n) => Ok(*n),
            _ => bail!("Hive 响应字段不是整数"),
        }
    }
    pub fn boolean(&self) -> Result<bool> {
        match self {
            Self::Bool(n) => Ok(*n),
            _ => bail!("Hive 响应字段不是布尔值"),
        }
    }
    pub fn bytes(&self) -> Result<&[u8]> {
        match self {
            Self::Bytes(n) => Ok(n),
            _ => bail!("Hive 响应字段不是字符串"),
        }
    }
    pub fn text(&self) -> Result<&str> {
        std::str::from_utf8(self.bytes()?).context("Hive 响应字符串不是 UTF-8")
    }
    pub fn list(&self) -> Result<&[Value]> {
        match self {
            Self::List(_, values) => Ok(values),
            _ => bail!("Hive 响应字段不是列表"),
        }
    }
    fn kind(&self) -> u8 {
        match self {
            Self::Bool(_) => 2,
            Self::Byte(_) => 3,
            Self::Double(_) => 4,
            Self::I16(_) => 6,
            Self::I32(_) => 8,
            Self::I64(_) => 10,
            Self::Bytes(_) => 11,
            Self::Struct(_) => 12,
            Self::Map(..) => 13,
            Self::List(..) => 15,
        }
    }
    fn write(&self, output: &mut Vec<u8>, depth: usize) -> Result<()> {
        ensure!(depth <= 64, "Hive 请求嵌套超过 64 层");
        match self {
            Self::Bool(v) => output.push(u8::from(*v)),
            Self::Byte(v) => output.push(*v as u8),
            Self::I16(v) => output.extend_from_slice(&v.to_be_bytes()),
            Self::I32(v) => output.extend_from_slice(&v.to_be_bytes()),
            Self::I64(v) => output.extend_from_slice(&v.to_be_bytes()),
            Self::Double(v) => output.extend_from_slice(&v.to_be_bytes()),
            Self::Bytes(v) => {
                output.extend_from_slice(&i32::try_from(v.len())?.to_be_bytes());
                output.extend_from_slice(v);
            }
            Self::Struct(fields) => {
                for (id, value) in fields {
                    output.push(value.kind());
                    output.extend_from_slice(&id.to_be_bytes());
                    value.write(output, depth + 1)?;
                }
                output.push(0);
            }
            Self::List(kind, values) => {
                output.push(*kind);
                output.extend_from_slice(&i32::try_from(values.len())?.to_be_bytes());
                for value in values {
                    ensure!(value.kind() == *kind, "Hive 请求列表类型不一致");
                    value.write(output, depth + 1)?;
                }
            }
            Self::Map(key, value, entries) => {
                output.extend_from_slice(&[*key, *value]);
                output.extend_from_slice(&i32::try_from(entries.len())?.to_be_bytes());
                for (k, v) in entries {
                    ensure!(
                        k.kind() == *key && v.kind() == *value,
                        "Hive 请求映射类型不一致"
                    );
                    k.write(output, depth + 1)?;
                    v.write(output, depth + 1)?;
                }
            }
        }
        ensure!(output.len() <= LIMIT, "Hive 请求超过 8 MiB");
        Ok(())
    }
}
fn message(method: &str, sequence: i32, kind: u32, body: &Value) -> Result<Vec<u8>> {
    let mut bytes = (0x8001_0000 | kind).to_be_bytes().to_vec();
    Value::string(method).write(&mut bytes, 0)?;
    bytes.extend_from_slice(&sequence.to_be_bytes());
    body.write(&mut bytes, 0)?;
    Ok(bytes)
}
pub(super) fn request(method: &str, sequence: i32, request: Value) -> Result<Vec<u8>> {
    message(method, sequence, 1, &Value::object([(1, request)]))
}
pub(super) async fn response<R: AsyncRead + Unpin + Send>(
    reader: &mut R,
    method: &str,
    sequence: i32,
) -> Result<Value> {
    let version = reader.read_u32().await?;
    ensure!(
        version & 0xffff_0000 == 0x8001_0000,
        "Hive 返回了无效的 Thrift 协议版本"
    );
    ensure!(
        matches!(version & 0xffff, 2 | 3),
        "Hive 返回了无效的 Thrift 消息类型"
    );
    let mut budget = Budget(0);
    let name = value(reader, 11, 0, &mut budget).await?;
    ensure!(
        name.text()? == method && reader.read_i32().await? == sequence,
        "Hive 返回了不匹配的请求标识"
    );
    let result = value(reader, 12, 0, &mut budget).await?;
    if version & 0xffff == 3 {
        bail!(
            "Hive Thrift 服务错误：{}",
            result
                .field(1)
                .and_then(|v| v.text().ok())
                .unwrap_or("未知错误")
        );
    }
    result.into_field(0)
}
struct Budget(usize);
impl Budget {
    fn reserve(&mut self, bytes: usize) -> Result<()> {
        self.0 = self.0.saturating_add(bytes);
        ensure!(self.0 <= LIMIT, "Hive 响应超过 8 MiB");
        Ok(())
    }
}
fn value<'a, R: AsyncRead + Unpin + Send>(
    reader: &'a mut R,
    kind: u8,
    depth: usize,
    budget: &'a mut Budget,
) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move {
        ensure!(depth <= 64, "Hive 响应嵌套超过 64 层");
        budget.reserve(std::mem::size_of::<Value>())?;
        Ok(match kind {
            2 => {
                let v = reader.read_u8().await?;
                ensure!(v <= 1, "Hive 布尔字段无效");
                Value::Bool(v == 1)
            }
            3 => Value::Byte(reader.read_i8().await?),
            4 => Value::Double(reader.read_f64().await?),
            6 => Value::I16(reader.read_i16().await?),
            8 => Value::I32(reader.read_i32().await?),
            10 => Value::I64(reader.read_i64().await?),
            11 => {
                let length =
                    usize::try_from(reader.read_i32().await?).context("Hive 字符串长度为负数")?;
                budget.reserve(length)?;
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).await?;
                Value::Bytes(bytes)
            }
            12 => {
                let mut fields = Vec::new();
                let mut names = std::collections::BTreeSet::new();
                loop {
                    let kind = reader.read_u8().await?;
                    if kind == 0 {
                        break;
                    }
                    ensure!(fields.len() < 4096, "Hive 结构字段超过 4096 项");
                    let id = reader.read_i16().await?;
                    ensure!(names.insert(id), "Hive 响应包含重复字段");
                    fields.push((id, value(reader, kind, depth + 1, budget).await?));
                }
                Value::Struct(fields)
            }
            14 | 15 => {
                let kind = reader.read_u8().await?;
                let count =
                    usize::try_from(reader.read_i32().await?).context("Hive 列表长度为负数")?;
                ensure!(count <= 100_000, "Hive 列表超过 100000 项");
                let mut values = Vec::new();
                for _ in 0..count {
                    values.push(value(reader, kind, depth + 1, budget).await?);
                }
                Value::List(kind, values)
            }
            13 => {
                let key = reader.read_u8().await?;
                let val = reader.read_u8().await?;
                let count =
                    usize::try_from(reader.read_i32().await?).context("Hive 映射长度为负数")?;
                ensure!(count <= 100_000, "Hive 映射超过 100000 项");
                let mut entries = Vec::new();
                for _ in 0..count {
                    entries.push((
                        value(reader, key, depth + 1, budget).await?,
                        value(reader, val, depth + 1, budget).await?,
                    ));
                }
                Value::Map(key, val, entries)
            }
            _ => bail!("Hive 返回了不支持的 Thrift 字段类型 {kind}"),
        })
    })
}

#[cfg(test)]
pub(super) async fn read_request<R: AsyncRead + Unpin + Send>(
    reader: &mut R,
) -> Result<(String, i32, Value)> {
    ensure!(
        reader.read_u32().await? == 0x8001_0001,
        "fixture expected strict Thrift CALL"
    );
    let mut budget = Budget(0);
    let method = value(reader, 11, 0, &mut budget).await?.text()?.to_owned();
    let sequence = reader.read_i32().await?;
    let request = value(reader, 12, 0, &mut budget).await?.into_field(1)?;
    Ok((method, sequence, request))
}
#[cfg(test)]
pub(super) fn reply(method: &str, sequence: i32, response: Value) -> Result<Vec<u8>> {
    message(method, sequence, 2, &Value::object([(0, response)]))
}
