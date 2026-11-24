//! Kafka wire API; topic leaders, controller and group coordinators are routed separately.
use super::*;
use anyhow::{Context, bail};
use bytes::{Bytes, BytesMut};
use kafka_protocol::{
    messages::*,
    protocol::{Decodable, Encodable, HeaderVersion, Request, StrBytes},
    records::{self, RecordBatchDecoder, RecordBatchEncoder},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
fn string(s: &str) -> StrBytes {
    StrBytes::from_string(s.into())
}
fn topic(s: &str) -> TopicName {
    TopicName(string(s))
}
fn group(s: &str) -> GroupId {
    GroupId(string(s))
}
#[derive(Debug)]
struct ProtocolError(i16);
impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Kafka 错误 {}", self.0)
    }
}
impl std::error::Error for ProtocolError {}
fn check(code: i16) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(ProtocolError(code).into())
    }
}

struct Client {
    transport: transport::Transport,
    bootstrap: Vec<String>,
    correlation: i32,
}
impl Client {
    fn new(c: &Connection) -> Self {
        Self {
            transport: transport::Transport::new(c),
            bootstrap: transport::bootstrap(c, ','),
            correlation: 0,
        }
    }
    async fn call<R: Request>(
        &mut self,
        address: &str,
        version: i16,
        request: R,
    ) -> Result<R::Response> {
        let mut socket = self.transport.dial(address).await?;
        self.correlation += 1;
        let id = self.correlation;
        let mut bytes = BytesMut::new();
        RequestHeader::default()
            .with_request_api_key(R::KEY)
            .with_request_api_version(version)
            .with_correlation_id(id)
            .with_client_id(Some(string("tomato")))
            .encode(&mut bytes, R::header_version(version))?;
        request.encode(&mut bytes, version)?;
        ensure!(bytes.len() <= 16 * 1024 * 1024, "Kafka 请求过大");
        tokio::time::timeout(Duration::from_secs(20), async {
            socket.write_i32(bytes.len() as i32).await?;
            socket.write_all(&bytes).await?;
            let len = socket.read_i32().await?;
            ensure!((4..=16 * 1024 * 1024).contains(&len), "Kafka 响应长度无效");
            let mut bytes = vec![0; len as usize];
            socket.read_exact(&mut bytes).await?;
            let mut bytes = Bytes::from(bytes);
            let header = ResponseHeader::decode(&mut bytes, R::Response::header_version(version))?;
            ensure!(header.correlation_id == id, "Kafka 响应序号不匹配");
            R::Response::decode(&mut bytes, version)
        })
        .await
        .context("Kafka 请求超时")?
    }
    async fn metadata(&mut self, name: Option<&str>) -> Result<MetadataResponse> {
        let request = MetadataRequest::default()
            .with_topics(name.map(|name| {
                vec![metadata_request::MetadataRequestTopic::default().with_name(Some(topic(name)))]
            }))
            .with_allow_auto_topic_creation(false);
        let mut last = None;
        for addr in self.bootstrap.clone() {
            match self.call(&addr, 9, request.clone()).await {
                Ok(r) => {
                    for t in &r.topics {
                        check(t.error_code)?;
                    }
                    return Ok(r);
                }
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("Kafka bootstrap 为空")))
    }
    fn address(metadata: &MetadataResponse, id: BrokerId) -> Result<String> {
        let b = metadata
            .brokers
            .iter()
            .find(|b| b.node_id == id)
            .ok_or(ProtocolError(5))?;
        Ok(format!(
            "{}:{}",
            if b.host.contains(':') {
                format!("[{}]", b.host)
            } else {
                b.host.to_string()
            },
            b.port
        ))
    }
    async fn coordinator(&mut self, metadata: &MetadataResponse, name: &str) -> Result<String> {
        let address = Self::address(
            metadata,
            metadata
                .brokers
                .first()
                .context("Kafka 集群没有 Broker")?
                .node_id,
        )?;
        let response = self
            .call(
                &address,
                3,
                FindCoordinatorRequest::default().with_key(string(name)),
            )
            .await?;
        check(response.error_code)?;
        Ok(format!("{}:{}", response.host, response.port))
    }
    async fn offset(
        &mut self,
        m: &MetadataResponse,
        name: &str,
        p: i32,
        timestamp: i64,
    ) -> Result<i64> {
        let t = m
            .topics
            .iter()
            .find(|t| t.name.as_ref().is_some_and(|x| x.as_str() == name))
            .ok_or(ProtocolError(3))?;
        let part = t
            .partitions
            .iter()
            .find(|x| x.partition_index == p)
            .context("分区不存在")?;
        check(part.error_code)?;
        let a = Self::address(m, part.leader_id)?;
        let response = self
            .call(
                &a,
                5,
                ListOffsetsRequest::default()
                    .with_replica_id(BrokerId(-1))
                    .with_topics(vec![
                        list_offsets_request::ListOffsetsTopic::default()
                            .with_name(topic(name))
                            .with_partitions(vec![
                                list_offsets_request::ListOffsetsPartition::default()
                                    .with_partition_index(p)
                                    .with_timestamp(timestamp),
                            ]),
                    ]),
            )
            .await?;
        let p = response
            .topics
            .first()
            .and_then(|t| t.partitions.first())
            .context("Kafka 未返回 Offset")?;
        check(p.error_code)?;
        Ok(p.offset)
    }
    async fn offsets(&mut self, m: &MetadataResponse, name: &str) -> Result<Vec<Value>> {
        let t = m
            .topics
            .iter()
            .find(|t| t.name.as_ref().is_some_and(|x| x.as_str() == name))
            .ok_or(ProtocolError(3))?;
        let mut out = Vec::new();
        for p in &t.partitions {
            let min = self.offset(m, name, p.partition_index, -2).await?;
            let max = self.offset(m, name, p.partition_index, -1).await?;
            out.push(json!({"partition":p.partition_index,"minOffset":min,"maxOffset":max,"messageCount":max-min,"leader":p.leader_id.0,"replicas":p.replica_nodes.iter().map(|n|n.0).collect::<Vec<_>>(),"isr":p.isr_nodes.iter().map(|n|n.0).collect::<Vec<_>>() }));
        }
        Ok(out)
    }
    async fn groups(&mut self, m: &MetadataResponse) -> Result<Vec<String>> {
        let mut groups = std::collections::BTreeSet::new();
        for b in &m.brokers {
            let a = Self::address(m, b.node_id)?;
            let r = self.call(&a, 3, ListGroupsRequest::default()).await?;
            check(r.error_code)?;
            for g in r.groups {
                groups.insert(g.group_id.to_string());
                ensure!(groups.len() <= 1000, "消费组超过 1000 个，请缩小查询范围");
            }
        }
        Ok(groups.into_iter().collect())
    }
    async fn commits(&mut self, m: &MetadataResponse, name: &str) -> Result<Vec<Value>> {
        let a = self.coordinator(m, name).await?;
        let r = self
            .call(
                &a,
                7,
                OffsetFetchRequest::default()
                    .with_group_id(group(name))
                    .with_topics(None),
            )
            .await?;
        check(r.error_code)?;
        let mut out = Vec::new();
        for t in r.topics {
            for p in t.partitions {
                check(p.error_code)?;
                let max = self.offset(m, &t.name, p.partition_index, -1).await?;
                out.push(json!({"group":name,"topic":t.name.to_string(),"partition":p.partition_index,"consumerOffset":p.committed_offset,"brokerOffset":max,"diff":(max-p.committed_offset.max(0)).max(0)}));
            }
        }
        Ok(out)
    }
    async fn fetch(
        &mut self,
        m: &MetadataResponse,
        name: &str,
        partition: i32,
        offset: i64,
        limit: usize,
        end: Option<i64>,
    ) -> Result<Vec<Value>> {
        let limit = limit.min(1000);
        let t = m
            .topics
            .iter()
            .find(|t| t.name.as_ref().is_some_and(|x| x.as_str() == name))
            .ok_or(ProtocolError(3))?;
        let p = t
            .partitions
            .iter()
            .find(|p| p.partition_index == partition)
            .context("分区不存在")?;
        let a = Self::address(m, p.leader_id)?;
        let mut out = Vec::new();
        let mut next = offset;
        let mut bytes = 0;
        for _ in 0..32 {
            if out.len() >= limit {
                break;
            }
            let r = self
                .call(
                    &a,
                    12,
                    FetchRequest::default()
                        .with_replica_id(BrokerId(-1))
                        .with_max_wait_ms(300)
                        .with_min_bytes(1)
                        .with_max_bytes(2 * 1024 * 1024)
                        .with_session_id(0)
                        .with_session_epoch(-1)
                        .with_topics(vec![
                            fetch_request::FetchTopic::default()
                                .with_topic(topic(name))
                                .with_partitions(vec![
                                    fetch_request::FetchPartition::default()
                                        .with_partition(partition)
                                        .with_fetch_offset(next)
                                        .with_partition_max_bytes(2 * 1024 * 1024),
                                ]),
                        ]),
                )
                .await?;
            check(r.error_code)?;
            let mut advanced = false;
            for t in r.responses {
                for p in t.partitions {
                    check(p.error_code)?;
                    let Some(mut records) = p.records else {
                        continue;
                    };
                    if records.is_empty() {
                        continue;
                    }
                    let decoded = RecordBatchDecoder::decode_with_custom_compression(
                        &mut records,
                        Some(decompress),
                    )?;
                    for record in decoded.records {
                        if record.offset < next {
                            continue;
                        }
                        next = record.offset + 1;
                        advanced = true;
                        if record.control {
                            continue;
                        }
                        if end.is_some_and(|end| record.timestamp > end) {
                            return Ok(out);
                        }
                        let value = record
                            .value
                            .as_deref()
                            .map(String::from_utf8_lossy)
                            .map(|v| v.into_owned());
                        bytes += value.as_ref().map_or(0, String::len);
                        ensure!(
                            bytes <= 8 * 1024 * 1024,
                            "消息正文超过 8 MiB，请缩小查询范围"
                        );
                        out.push(json!({"topic":name,"partition":partition,"offset":record.offset,"timestamp":record.timestamp,"timestampType":if record.timestamp_type==records::TimestampType::Creation{"CREATE_TIME"}else{"LOG_APPEND_TIME"},"key":record.key.as_deref().map(String::from_utf8_lossy),"body":value,"headers":record.headers.into_iter().map(|(k,v)|json!({"key":k.as_str(),"value":v.as_deref().map(String::from_utf8_lossy)})).collect::<Vec<_>>()}));
                        if out.len() >= limit {
                            break;
                        }
                    }
                }
            }
            if !advanced {
                break;
            }
        }
        Ok(out)
    }
}
fn decompress(bytes: &mut Bytes, kind: records::Compression) -> Result<Bytes> {
    use std::io::Read;
    let reader: Box<dyn Read> = match kind {
        records::Compression::None => return Ok(bytes.clone()),
        records::Compression::Gzip => Box::new(flate2::read::GzDecoder::new(bytes.as_ref())),
        records::Compression::Lz4 => Box::new(lz4::Decoder::new(bytes.as_ref())?),
        records::Compression::Zstd => Box::new(zstd::stream::read::Decoder::new(bytes.as_ref())?),
        records::Compression::Snappy => {
            let mut input = bytes.as_ref();
            let mut out = Vec::new();
            if input.starts_with(b"\x82SNAPPY\0") {
                ensure!(input.len() >= 16, "Snappy 数据截断");
                input = &input[16..];
                while !input.is_empty() {
                    ensure!(input.len() >= 4, "Snappy 块截断");
                    let n = u32::from_be_bytes(input[..4].try_into()?) as usize;
                    input = &input[4..];
                    ensure!(n <= input.len(), "Snappy 块长度无效");
                    let chunk = &input[..n];
                    ensure!(
                        out.len() + snap::raw::decompress_len(chunk)? <= 16 * 1024 * 1024,
                        "Kafka 解压超出 16 MiB"
                    );
                    out.extend(snap::raw::Decoder::new().decompress_vec(chunk)?);
                    input = &input[n..];
                }
            } else {
                ensure!(
                    snap::raw::decompress_len(input)? <= 16 * 1024 * 1024,
                    "Kafka 解压超出 16 MiB"
                );
                out = snap::raw::Decoder::new().decompress_vec(input)?;
            }
            return Ok(out.into());
        }
    };
    let mut out = Vec::new();
    reader.take(16 * 1024 * 1024 + 1).read_to_end(&mut out)?;
    ensure!(out.len() <= 16 * 1024 * 1024, "Kafka 解压超出 16 MiB");
    Ok(out.into())
}
pub async fn execute(c: &Connection, action: Action) -> Result<Output> {
    for attempt in 0..5 {
        let result = execute_once(c, action.clone()).await;
        let retry = result.as_ref().is_err_and(|e| {
            e.downcast_ref::<ProtocolError>()
                .is_some_and(|e| matches!(e.0, 3 | 5 | 6 | 14 | 15 | 16 | 41))
        });
        if !retry || attempt == 4 {
            return result;
        }
        // These broker rejections explicitly mean the request was not applied.
        // An I/O timeout after producing is never replayed: its delivery is uncertain.
        tokio::time::sleep(Duration::from_millis(100 * (1 << attempt))).await;
    }
    unreachable!()
}
async fn execute_once(c: &Connection, action: Action) -> Result<Output> {
    let mut client = Client::new(c);
    let m = client.metadata(None).await?;
    ensure!(
        m.brokers.len() <= 100 && m.topics.len() <= 10000,
        "Kafka 目录超过内存上限"
    );
    match action{
        Action::Topics=>Ok(Output::rows(m.topics.iter().map(|t|json!({"topic":t.name.as_ref().map(|n|n.as_str()),"partitions":t.partitions.len(),"replicationFactor":t.partitions.first().map(|p|p.replica_nodes.len()),"internal":t.is_internal})).collect())),
        Action::Stats{topic}|Action::Route{topic}=>Ok(Output::rows(client.offsets(&m,&topic).await?)),
        Action::Cluster=>Ok(Output::rows(m.brokers.iter().map(|b|json!({"nodeId":b.node_id.0,"host":b.host.as_str(),"port":b.port,"rack":b.rack.as_ref().map(|s|s.as_str()),"controller":b.node_id==m.controller_id})).collect())),
        Action::CreateTopic{topic:name,partitions,replicas}=>{
            ensure!(!name.is_empty()&&partitions>0&&partitions<=10000&&replicas>0,"主题参数无效");let a=Client::address(&m,m.controller_id)?;
            let r=client.call(&a,5,CreateTopicsRequest::default().with_timeout_ms(15000).with_topics(vec![create_topics_request::CreatableTopic::default().with_name(topic(&name)).with_num_partitions(partitions).with_replication_factor(replicas)])).await?;
            for t in r.topics{check(t.error_code)?;}Ok(Output::changed("创建主题成功"))
        }
        Action::DeleteTopic{topic:name}=>{let a=Client::address(&m,m.controller_id)?;let r=client.call(&a,4,DeleteTopicsRequest::default().with_timeout_ms(15000).with_topic_names(vec![topic(&name)])).await?;for t in r.responses{check(t.error_code)?;}Ok(Output::changed("删除主题成功"))}
        Action::Groups=>{let mut rows=Vec::new();for name in client.groups(&m).await?{let commits=client.commits(&m,&name).await?;let lag:i64=commits.iter().filter_map(|v|v["diff"].as_i64()).sum();rows.push(json!({"group":name,"diff":lag}));}Ok(Output::rows(rows))}
        Action::Group{group:name}=>{let a=client.coordinator(&m,&name).await?;let r=client.call(&a,5,DescribeGroupsRequest::default().with_groups(vec![group(&name)])).await?;let g=r.groups.first().context("消费组不存在")?;check(g.error_code)?;
            let offsets=client.commits(&m,&name).await?;let mut out=Output::rows(offsets);out.detail=json!({"group":name,"state":g.group_state.as_str(),"protocol":g.protocol_data.as_str(),"members":g.members.iter().map(|member|json!({"memberId":member.member_id.as_str(),"clientId":member.client_id.as_str(),"host":member.client_host.as_str()})).collect::<Vec<_>>()});Ok(out)}
        Action::DeleteGroups { groups } => batch::run(groups, &mut GroupRemover { client: &mut client, metadata: &m }).await,
        Action::DeleteGroup{group:name}=>{let a=client.coordinator(&m,&name).await?;let r=client.call(&a,2,DeleteGroupsRequest::default().with_groups_names(vec![group(&name)])).await?;for g in r.results{check(g.error_code)?;}Ok(Output::changed("删除消费组成功"))}
        Action::ConsumeStatus{topic:name}=>{let mut rows=Vec::new();for group in client.groups(&m).await?{append_rows(client.commits(&m,&group).await?.into_iter().filter(|v|v["topic"]==name),&mut rows)?;}Ok(Output::rows(rows))}
        Action::Messages{topic:name,query}=>{
            let mut rows=Vec::new();match query{
                Query::Offset{partition,offset,limit}=>rows=client.fetch(&m,&name,partition,offset,limit,None).await?,
                Query::Time{begin,end}=>{ensure!(begin<=end,"开始时间不能晚于结束时间");let t=m.topics.iter().find(|t|t.name.as_ref().is_some_and(|x|x.as_str()==name)).ok_or(ProtocolError(3))?;
                    for p in &t.partitions{let offset=client.offset(&m,&name,p.partition_index,begin).await?;if offset>=0{append_rows(client.fetch(&m,&name,p.partition_index,offset,1000-rows.len(),Some(end)).await?,&mut rows)?;}
if rows.len()>=1000{break}}rows.sort_by_key(|v|v["timestamp"].as_i64().unwrap_or(0));}
                Query::Unconsumed{group,limit}=>{for p in client.commits(&m,&group).await?.into_iter().filter(|v|v["topic"]==name){append_rows(client.fetch(&m,&name,p["partition"].as_i64().context("缺少分区")?as i32,p["consumerOffset"].as_i64().unwrap_or(0).max(0),limit.min(1000).saturating_sub(rows.len()),None).await?,&mut rows)?;if rows.len()>=limit.min(1000){break}}}
                _=>bail!("Kafka 使用时间或 Offset 查询消息"),
            }Ok(Output::rows(rows))
        }
        Action::Send{topic:name,partition,key,body,headers,..}=>{
            let t=m.topics.iter().find(|t|t.name.as_ref().is_some_and(|x|x.as_str()==name)).ok_or(ProtocolError(3))?;let partition=match partition {Some(p)=>p,None=>default_partition(t,&key)?};
            let p=t.partitions.iter().find(|p|p.partition_index==partition).context("分区不存在")?;let a=Client::address(&m,p.leader_id)?;
            let record=records::Record{transactional:false,control:false,partition_leader_epoch:-1,producer_id:-1,producer_epoch:-1,timestamp_type:records::TimestampType::Creation,offset:0,sequence:-1,timestamp:chrono::Utc::now().timestamp_millis(),key:(!key.is_empty()).then(||Bytes::from(key)),value:Some(Bytes::from(body)),headers:headers.into_iter().map(|(k,v)|(string(&k),Some(Bytes::from(v)))).collect()};
            let mut bytes=BytesMut::new();RecordBatchEncoder::encode(&mut bytes,[&record],&records::RecordEncodeOptions{version:2,compression:records::Compression::None})?;
            let r=client.call(&a,8,ProduceRequest::default().with_acks(-1).with_timeout_ms(15000).with_topic_data(vec![produce_request::TopicProduceData::default().with_name(topic(&name)).with_partition_data(vec![produce_request::PartitionProduceData::default().with_index(partition).with_records(Some(bytes.freeze()))])])).await?;
            let result=r.responses.first().and_then(|t|t.partition_responses.first()).context("Kafka 未确认发送")?;check(result.error_code)?;let mut out=Output::changed("发送成功");out.detail=json!({"partition":partition,"offset":result.base_offset});Ok(out)
        }
        _=>bail!("此操作仅用于 RocketMQ"),
    }
}

// Kafka's default producer partitioner uses Java Utils.murmur2 for keyed records.
fn murmur2(data: &[u8]) -> u32 {
    const M: u32 = 0x5bd1e995;
    let mut h = 0x9747b28c_u32 ^ data.len() as u32;
    let (chunks, tail) = data.as_chunks::<4>();
    for bytes in chunks {
        let mut k = u32::from_le_bytes(*bytes);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M) ^ k;
    }
    for (index, byte) in tail.iter().enumerate() {
        h ^= u32::from(*byte) << (index * 8);
    }
    if !tail.is_empty() {
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ (h >> 15)
}
fn default_partition(topic: &metadata_response::MetadataResponseTopic, key: &str) -> Result<i32> {
    ensure!(!topic.partitions.is_empty(), "主题没有分区");
    if !key.is_empty() {
        return Ok(
            ((murmur2(key.as_bytes()) & 0x7fff_ffff) as usize % topic.partitions.len()) as i32,
        );
    }
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let mut available: Vec<_> = topic
        .partitions
        .iter()
        .filter(|p| p.leader_id.0 >= 0 && p.error_code == 0)
        .map(|p| p.partition_index)
        .collect();
    if available.is_empty() {
        return Err(ProtocolError(5).into());
    }
    available.sort_unstable();
    let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % available.len();
    Ok(available[index])
}
struct GroupRemover<'a> {
    client: &'a mut Client,
    metadata: &'a MetadataResponse,
}
impl batch::Remove for GroupRemover<'_> {
    async fn remove(&mut self, name: &str) -> Result<()> {
        let address = self.client.coordinator(self.metadata, name).await?;
        let response = self
            .client
            .call(
                &address,
                2,
                DeleteGroupsRequest::default().with_groups_names(vec![group(name)]),
            )
            .await?;
        ensure!(!response.results.is_empty(), "Kafka 未返回消费者组删除结果");
        for result in response.results {
            check(result.error_code)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod partition_tests {
    #[test]
    fn key_hashes_match_original_java_kafka_utils() {
        let rows: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/kafka-partitioner.json"))
                .unwrap();
        for row in rows.as_array().unwrap() {
            assert_eq!(
                super::murmur2(row["key"].as_str().unwrap().as_bytes()) as i32,
                row["hash"].as_i64().unwrap() as i32
            );
        }
    }
}
