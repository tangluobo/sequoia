//! Test-only consumer actors connect exclusively to the explicitly owned loopback brokers.
use anyhow::{Context, Result, ensure};
use bytes::{Bytes, BytesMut};
use kafka_protocol::{
    messages::*,
    protocol::{Decodable, Encodable, HeaderVersion, Request, StrBytes},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
fn check_loopback(address: &str) -> Result<()> {
    ensure!(
        address.parse::<std::net::SocketAddr>()?.ip().is_loopback(),
        "Fixture address must be loopback"
    );
    Ok(())
}
async fn kafka<R: Request>(addr: &str, version: i16, r: R) -> Result<R::Response> {
    check_loopback(addr)?;
    let mut s = TcpStream::connect(addr).await?;
    let mut b = BytesMut::new();
    RequestHeader::default()
        .with_request_api_key(R::KEY)
        .with_request_api_version(version)
        .with_correlation_id(1)
        .with_client_id(Some(StrBytes::from_static_str("tomato-seed")))
        .encode(&mut b, R::header_version(version))?;
    r.encode(&mut b, version)?;
    s.write_i32(b.len() as i32).await?;
    s.write_all(&b).await?;
    let n = s.read_i32().await?;
    ensure!((4..=1024 * 1024).contains(&n), "Invalid seed reply");
    let mut b = vec![0; n as usize];
    s.read_exact(&mut b).await?;
    let mut b = Bytes::from(b);
    ResponseHeader::decode(&mut b, R::Response::header_version(version))?;
    R::Response::decode(&mut b, version)
}
pub async fn kafka_group(
    addr: &str,
    topic: &str,
    group: &str,
    partition: i32,
    offset: i64,
) -> Result<()> {
    let group = GroupId(StrBytes::from_string(group.into()));
    let request = OffsetCommitRequest::default()
        .with_group_id(group.clone())
        .with_generation_id_or_member_epoch(-1)
        .with_topics(vec![
            offset_commit_request::OffsetCommitRequestTopic::default()
                .with_name(TopicName(StrBytes::from_string(topic.into())))
                .with_partitions(vec![
                    offset_commit_request::OffsetCommitRequestPartition::default()
                        .with_partition_index(partition)
                        .with_committed_offset(offset),
                ]),
        ]);
    // FindCoordinator can return a leader before its offsets partition has
    // loaded. Re-discover and retry only the identical idempotent test offset.
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let coordinator = kafka(
                addr,
                3,
                FindCoordinatorRequest::default().with_key(group.0.clone()),
            )
            .await?;
            ensure!(
                matches!(coordinator.error_code, 0 | 14..=16),
                "Seed coordinator failed: {}",
                coordinator.error_code
            );
            if coordinator.error_code == 0 {
                let address = format!("{}:{}", coordinator.host, coordinator.port);
                let reply = kafka(&address, 7, request.clone()).await?;
                ensure!(
                    reply.topics.len() == 1 && reply.topics[0].partitions.len() == 1,
                    "Invalid seed offset reply"
                );
                let code = reply.topics[0].partitions[0].error_code;
                if code == 0 {
                    return Ok::<_, anyhow::Error>(());
                }
                ensure!(matches!(code, 14..=16), "Seed offset commit failed: {code}");
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .context("Owned group coordinator/offset partition did not become ready")?
}
async fn write(s: &mut TcpStream, h: Value, body: &[u8]) -> Result<()> {
    let h = serde_json::to_vec(&h)?;
    s.write_u32((4 + h.len() + body.len()) as u32).await?;
    s.write_u32(h.len() as u32).await?;
    s.write_all(&h).await?;
    s.write_all(body).await?;
    Ok(())
}
async fn read(s: &mut TcpStream) -> Result<(Value, Vec<u8>)> {
    let n = s.read_u32().await? as usize;
    ensure!(
        (4..=4 * 1024 * 1024).contains(&n),
        "Invalid owned broker frame"
    );
    let mut b = vec![0; n];
    s.read_exact(&mut b).await?;
    let k = (u32::from_be_bytes(b[..4].try_into()?) & 0xffffff) as usize;
    ensure!(k <= n - 4, "Invalid owned broker header");
    Ok((serde_json::from_slice(&b[4..4 + k])?, b[4 + k..].to_vec()))
}
fn header(code: i32, fields: Value) -> Value {
    json!({"code":code,"language":"OTHER","version":433,"opaque":1,"flag":0,"extFields":fields})
}
pub async fn rocket_call(addr: &str, code: i32, fields: Value, body: Value) -> Result<Value> {
    check_loopback(addr)?;
    let mut s = TcpStream::connect(addr).await?;
    let b = if body.is_null() {
        Vec::new()
    } else {
        serde_json::to_vec(&body)?
    };
    write(&mut s, header(code, fields), &b).await?;
    let (h, b) = read(&mut s).await?;
    ensure!(h["code"] == 0, "Seed RocketMQ request failed {h}");
    if b.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(serde_json::from_slice(&b)?)
    }
}
pub struct RocketConsumer {
    task: tokio::task::JoinHandle<()>,
    pub deliveries: Arc<AtomicUsize>,
}
impl Drop for RocketConsumer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub async fn rocket_consumer(addr: &str, topic: &str, group: &str) -> Result<RocketConsumer> {
    check_loopback(addr)?;
    rocket_call(addr,200,json!({}),json!({"groupName":group,"consumeEnable":true,"consumeFromMinEnable":true,"consumeBroadcastEnable":true,"retryQueueNums":1,"retryMaxTimes":16,"brokerId":0,"whichBrokerWhenConsumeSlowly":1,"notifyConsumerIdsChangedEnable":true})).await?;
    rocket_offset(addr, topic, group, 0).await?;
    let heartbeat = serde_json::to_vec(
        &json!({"clientID":format!("127.0.0.1@owned-{group}"),"producerDataSet":[],"consumerDataSet":[{"groupName":group,"consumeType":"CONSUME_PASSIVELY","messageModel":"CLUSTERING","consumeFromWhere":"CONSUME_FROM_LAST_OFFSET","subscriptionDataSet":[{"topic":topic,"subString":"*","tagsSet":[],"codeSet":[],"subVersion":chrono::Utc::now().timestamp_millis(),"expressionType":"TAG","classFilterMode":false}],"unitMode":false}]}),
    )?;
    let mut socket = TcpStream::connect(addr).await?;
    write(&mut socket, header(34, json!({})), &heartbeat).await?;
    loop {
        let (h, _) = read(&mut socket).await?;
        if h["flag"].as_i64().unwrap_or(0) & 1 != 0 {
            ensure!(h["code"] == 0, "Heartbeat rejected");
            break;
        }
    }
    let deliveries = Arc::new(AtomicUsize::new(0));
    let counter = deliveries.clone();
    let task = tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                            result=read(&mut socket)=>{let Ok((request,_body))=result else{break};if request["flag"].as_i64().unwrap_or(0)&1!=0{continue}
            if request["code"]==309{counter.fetch_add(1,Ordering::AcqRel);let body=serde_json::to_vec(&json!({"order":false,"autoCommit":true,"consumeResult":"CR_SUCCESS","remark":"owned consumer acknowledged","spentTimeMills":1})).unwrap();let response=json!({"code":0,"language":"OTHER","version":433,"opaque":request["opaque"],"flag":1});if write(&mut socket,response,&body).await.is_err(){break}}},
                            _=timer.tick()=>{if write(&mut socket,header(34,json!({})),&heartbeat).await.is_err(){break}},
                        }
        }
    });
    Ok(RocketConsumer { task, deliveries })
}
pub async fn rocket_offset(addr: &str, topic: &str, group: &str, offset: i64) -> Result<()> {
    rocket_call(addr,15,json!({"consumerGroup":group,"topic":topic,"queueId":"0","commitOffset":offset.to_string()}),Value::Null).await?;
    Ok(())
}

/// Producer fixtures use the upstream codec to exercise compressed records received by the native client.
pub async fn kafka_compressed(
    addr: &str,
    topic: &str,
    compression: kafka_protocol::records::Compression,
    body: &str,
) -> Result<()> {
    use kafka_protocol::records::{Record, RecordBatchEncoder, RecordEncodeOptions, TimestampType};
    let record = Record {
        transactional: false,
        control: false,
        partition_leader_epoch: -1,
        producer_id: -1,
        producer_epoch: -1,
        timestamp_type: TimestampType::Creation,
        offset: 0,
        sequence: -1,
        timestamp: chrono::Utc::now().timestamp_millis(),
        key: None,
        value: Some(Bytes::copy_from_slice(body.as_bytes())),
        headers: Default::default(),
    };
    let mut b = BytesMut::new();
    RecordBatchEncoder::encode(
        &mut b,
        [&record],
        &RecordEncodeOptions {
            version: 2,
            compression,
        },
    )?;
    let request = ProduceRequest::default()
        .with_acks(-1)
        .with_timeout_ms(15000)
        .with_topic_data(vec![
            produce_request::TopicProduceData::default()
                .with_name(TopicName(StrBytes::from_string(topic.into())))
                .with_partition_data(vec![
                    produce_request::PartitionProduceData::default()
                        .with_index(1)
                        .with_records(Some(b.freeze())),
                ]),
        ]);
    let response = kafka(addr, 8, request).await?;
    ensure!(
        response.responses[0].partition_responses[0].error_code == 0,
        "Owned compressed produce rejected"
    );
    Ok(())
}
pub async fn rocket_compressed(addr: &str, topic: &str, codec: u32, body: &str) -> Result<()> {
    use std::io::Write;
    check_loopback(addr)?;
    let compressed = match codec {
        0 => {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(body.as_bytes())?;
            encoder.finish()?
        }
        1 => {
            let mut encoder = lz4::EncoderBuilder::new().build(Vec::new())?;
            encoder.write_all(body.as_bytes())?;
            let (bytes, result) = encoder.finish();
            result?;
            bytes
        }
        2 => zstd::stream::encode_all(body.as_bytes(), 3)?,
        _ => anyhow::bail!("Invalid owned compression"),
    };
    let mut stream = TcpStream::connect(addr).await?;
    let fields = json!({"producerGroup":"tomato-compressed-seed","topic":topic,"defaultTopic":"TBW102","defaultTopicQueueNums":"2","queueId":"0","sysFlag":(1|(codec<<8)).to_string(),"bornTimestamp":chrono::Utc::now().timestamp_millis().to_string(),"flag":"0","properties":"","reconsumeTimes":"0","unitMode":"false","batch":"false"});
    write(&mut stream, header(10, fields), &compressed).await?;
    let (response, _) = read(&mut stream).await?;
    ensure!(
        response["code"] == 0,
        "Owned compressed RocketMQ send rejected: {response}"
    );
    Ok(())
}
