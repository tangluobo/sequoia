#[path = "support/mq_seed.rs"]
mod seed;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use tomato_services::mq::{self, Action, Query};
fn connection(kind: &str) -> Result<tomato_core::Connection> {
    let endpoint = std::env::var(if kind == "KAFKA" {
        "TOMATO_KAFKA_FIXTURE"
    } else {
        "TOMATO_ROCKET_FIXTURE"
    })
    .context("Start the owned loopback MQ fixtures first")?;
    let address: std::net::SocketAddr = endpoint.parse()?;
    ensure!(address.ip().is_loopback(), "MQ fixture must be loopback");
    let mut c = tomato_core::Connection::new(kind);
    c.set("host", address.ip().to_string());
    c.set("port", address.port());
    Ok(c)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned loopback Kafka broker"]
async fn actual_kafka_topics_produce_fetch_offsets_time_cluster_and_delete() -> Result<()> {
    let c = connection("KAFKA")?;
    let name = format!("tomato_qa_{}", uuid::Uuid::new_v4().simple());
    let create = mq::execute(
        &c,
        Action::CreateTopic {
            topic: name.clone(),
            partitions: 2,
            replicas: 1,
        },
    )
    .await?;
    assert!(create.message.contains("成功"));
    let work = async {
        let topics = mq::execute(&c, Action::Topics).await?;
        assert!(topics.rows.iter().any(|v| v["topic"] == name));
        let begin = chrono::Utc::now().timestamp_millis() - 1000;
        let send = mq::execute(
            &c,
            Action::Send {
                topic: name.clone(),
                partition: Some(1),
                key: "fixture-key".into(),
                body: "{\"消息\":\"真实 Kafka 往返\"}".into(),
                headers: BTreeMap::from([("fixture-header".into(), "header-value".into())]),
                tags: String::new(),
                delay: 0,
            },
        )
        .await?;
        assert_eq!(send.detail["partition"], 1);
        let messages = mq::execute(
            &c,
            Action::Messages {
                topic: name.clone(),
                query: Query::Offset {
                    partition: 1,
                    offset: 0,
                    limit: 10,
                },
            },
        )
        .await?;
        assert_eq!(messages.rows.len(), 1);
        assert_eq!(messages.rows[0]["key"], "fixture-key");
        assert_eq!(messages.rows[0]["body"], "{\"消息\":\"真实 Kafka 往返\"}");
        assert_eq!(messages.rows[0]["headers"][0]["value"], "header-value");
        let by_time = mq::execute(
            &c,
            Action::Messages {
                topic: name.clone(),
                query: Query::Time {
                    begin,
                    end: chrono::Utc::now().timestamp_millis() + 1000,
                },
            },
        )
        .await?;
        assert_eq!(by_time.rows.len(), 1);
        let stats = mq::execute(
            &c,
            Action::Stats {
                topic: name.clone(),
            },
        )
        .await?;
        assert_eq!(stats.rows.len(), 2);
        assert!(
            stats
                .rows
                .iter()
                .any(|v| v["partition"] == 1 && v["maxOffset"] == 1)
        );
        assert!(!mq::execute(&c, Action::Cluster).await?.rows.is_empty());
        let group = format!("owned_group_{}", uuid::Uuid::new_v4().simple());
        let endpoint = std::env::var("TOMATO_KAFKA_FIXTURE")?;
        seed::kafka_group(&endpoint, &name, &group, 1, 0).await?;
        let groups = mq::execute(&c, Action::Groups).await?;
        assert!(
            groups
                .rows
                .iter()
                .any(|v| v["group"] == group && v["diff"] == 1)
        );
        assert_eq!(
            mq::execute(
                &c,
                Action::Group {
                    group: group.clone()
                }
            )
            .await?
            .rows[0]["consumerOffset"],
            0
        );
        assert!(
            mq::execute(
                &c,
                Action::ConsumeStatus {
                    topic: name.clone()
                }
            )
            .await?
            .rows
            .iter()
            .any(|v| v["group"] == group)
        );
        assert_eq!(
            mq::execute(
                &c,
                Action::Messages {
                    topic: name.clone(),
                    query: Query::Unconsumed {
                        group: group.clone(),
                        limit: 10
                    }
                }
            )
            .await?
            .rows
            .len(),
            1
        );
        mq::execute(&c, Action::DeleteGroup { group }).await?;
        actual_group_batch(&c, &name).await?;

        mq::execute(&c, Action::Groups).await?;
        for (codec, marker) in [
            (kafka_protocol::records::Compression::Gzip, "gzip"),
            (kafka_protocol::records::Compression::Snappy, "snappy"),
            (kafka_protocol::records::Compression::Lz4, "lz4"),
            (kafka_protocol::records::Compression::Zstd, "zstd"),
        ] {
            seed::kafka_compressed(
                &endpoint,
                &name,
                codec,
                &format!("{marker}:{}", "压缩消息".repeat(4096)),
            )
            .await?;
        }
        let compressed = mq::execute(
            &c,
            Action::Messages {
                topic: name.clone(),
                query: Query::Offset {
                    partition: 1,
                    offset: 1,
                    limit: 10,
                },
            },
        )
        .await?;
        assert_eq!(compressed.rows.len(), 4);
        for (message, marker) in compressed
            .rows
            .iter()
            .zip(["gzip", "snappy", "lz4", "zstd"])
        {
            assert_eq!(
                message["body"],
                format!("{marker}:{}", "压缩消息".repeat(4096))
            );
        }
        let keyed = mq::execute(
            &c,
            Action::Send {
                topic: name.clone(),
                partition: None,
                key: "key".into(),
                body: "默认分区".into(),
                headers: BTreeMap::new(),
                tags: String::new(),
                delay: 0,
            },
        )
        .await?;
        assert_eq!(keyed.detail["partition"], 1);
        for expected in 0..2 {
            let unkeyed = mq::execute(
                &c,
                Action::Send {
                    topic: name.clone(),
                    partition: None,
                    key: String::new(),
                    body: format!("轮询分区 {expected}"),
                    headers: BTreeMap::new(),
                    tags: String::new(),
                    delay: 0,
                },
            )
            .await?;
            assert_eq!(unkeyed.detail["partition"], expected);
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    let remove = mq::execute(
        &c,
        Action::DeleteTopic {
            topic: name.clone(),
        },
    )
    .await;
    work?;
    remove?;
    assert!(
        !mq::execute(&c, Action::Topics)
            .await?
            .rows
            .iter()
            .any(|v| v["topic"] == name)
    );
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned loopback RocketMQ NameServer and broker"]
async fn actual_rocketmq_create_route_send_pull_key_id_stats_cluster_and_delete() -> Result<()> {
    let c = connection("ROCKETMQ")?;
    let name = format!("tomato_qa_{}", uuid::Uuid::new_v4().simple());
    mq::execute(
        &c,
        Action::CreateTopic {
            topic: name.clone(),
            partitions: 2,
            replicas: 1,
        },
    )
    .await?;
    let work = async {
        let mut ready = false;
        for _ in 0..30 {
            if mq::execute(
                &c,
                Action::Route {
                    topic: name.clone(),
                },
            )
            .await
            .is_ok()
            {
                ready = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        ensure!(ready, "created RocketMQ topic never acquired route");
        let begin = chrono::Utc::now().timestamp_millis() - 1000;
        let send = mq::execute(
            &c,
            Action::Send {
                topic: name.clone(),
                partition: None,
                key: "fixture-key".into(),
                body: "{\"消息\":\"真实 RocketMQ 往返\"}".into(),
                headers: BTreeMap::new(),
                tags: "fixture-tag".into(),
                delay: 0,
            },
        )
        .await?;
        let id = send.detail["msgId"]
            .as_str()
            .context("missing msgId")?
            .to_owned();
        // SEND_OK acknowledges the commit log, before consume-queue dispatch.
        // Wait for visibility with read-only polls; never replay the send.
        let visible_by = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let messages = loop {
            let rows = mq::execute(
                &c,
                Action::Messages {
                    topic: name.clone(),
                    query: Query::Offset {
                        partition: 0,
                        offset: 0,
                        limit: 10,
                    },
                },
            )
            .await?;
            if !rows.rows.is_empty() {
                break rows;
            }
            ensure!(
                tokio::time::Instant::now() < visible_by,
                "Broker did not publish the acknowledged message in queue 0"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };
        assert_eq!(messages.rows.len(), 1);
        assert_eq!(messages.rows[0]["tags"], "fixture-tag");
        assert_eq!(
            messages.rows[0]["body"],
            "{\"消息\":\"真实 RocketMQ 往返\"}"
        );
        let by_id = mq::execute(
            &c,
            Action::Messages {
                topic: name.clone(),
                query: Query::Id(id),
            },
        )
        .await?;
        assert_eq!(by_id.rows.len(), 1);
        let by_time = mq::execute(
            &c,
            Action::Messages {
                topic: name.clone(),
                query: Query::Time {
                    begin,
                    end: chrono::Utc::now().timestamp_millis() + 1000,
                },
            },
        )
        .await?;
        assert_eq!(by_time.rows.len(), 1);
        // Broker's key index is asynchronous. Wait for its own message instead of increasing RPC timeouts.
        let mut indexed = false;
        for _ in 0..30 {
            let result = mq::execute(
                &c,
                Action::Messages {
                    topic: name.clone(),
                    query: Query::Key("fixture-key".into()),
                },
            )
            .await?;
            if !result.rows.is_empty() {
                indexed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(indexed);
        assert_eq!(
            mq::execute(
                &c,
                Action::Stats {
                    topic: name.clone()
                }
            )
            .await?
            .rows
            .len(),
            2
        );
        let cluster = mq::execute(&c, Action::Cluster).await?;
        assert!(!cluster.rows.is_empty());
        let address = cluster.rows[0]["address"].as_str().unwrap().to_owned();
        assert!(
            !mq::execute(&c, Action::Broker { address })
                .await?
                .detail
                .is_null()
        );

        let group = format!("owned_group_{}", uuid::Uuid::new_v4().simple());
        let broker = std::env::var("TOMATO_ROCKET_BROKER_FIXTURE")?;
        let consumer = seed::rocket_consumer(&broker, &name, &group).await?;
        let groups = mq::execute(&c, Action::Groups).await?;
        assert!(groups.rows.iter().any(|v| v["group"] == group));
        assert!(
            mq::execute(
                &c,
                Action::Group {
                    group: group.clone()
                }
            )
            .await?
            .rows
            .iter()
            .any(|v| v["consumerOffset"] == 0 && v["brokerOffset"] == 1)
        );
        assert_eq!(
            mq::execute(
                &c,
                Action::Messages {
                    topic: name.clone(),
                    query: Query::Unconsumed {
                        group: group.clone(),
                        limit: 10
                    }
                }
            )
            .await?
            .rows
            .len(),
            1
        );
        let id = send.detail["msgId"].as_str().unwrap().to_owned();
        assert!(
            mq::execute(
                &c,
                Action::Track {
                    topic: name.clone(),
                    id: id.clone()
                }
            )
            .await?
            .rows
            .iter()
            .any(|v| v["group"] == group && v["trackType"] == "NOT_CONSUME_YET")
        );
        let result = mq::execute(
            &c,
            Action::Reconsume {
                topic: name.clone(),
                group: group.clone(),
                id: id.clone(),
            },
        )
        .await?;
        assert_eq!(result.detail["consumeResult"], "CR_SUCCESS");
        assert_eq!(
            consumer
                .deliveries
                .load(std::sync::atomic::Ordering::Acquire),
            1
        );
        seed::rocket_offset(&broker, &name, &group, 1).await?;
        assert!(
            mq::execute(
                &c,
                Action::Track {
                    topic: name.clone(),
                    id
                }
            )
            .await?
            .rows
            .iter()
            .any(|v| v["group"] == group && v["trackType"] == "CONSUMED")
        );
        drop(consumer);
        mq::execute(&c, Action::DeleteGroup { group }).await?;
        actual_group_batch(&c, &name).await?;
        mq::execute(&c, Action::Groups).await?;
        for codec in 0..3 {
            seed::rocket_compressed(
                &broker,
                &name,
                codec,
                &format!("codec-{codec}:{}", "压缩消息".repeat(4096)),
            )
            .await?;
        }
        let compressed = mq::execute(
            &c,
            Action::Messages {
                topic: name.clone(),
                query: Query::Offset {
                    partition: 0,
                    offset: 1,
                    limit: 10,
                },
            },
        )
        .await?;
        assert_eq!(compressed.rows.len(), 3);
        for (codec, message) in compressed.rows.iter().enumerate() {
            assert_eq!(
                message["body"],
                format!("codec-{codec}:{}", "压缩消息".repeat(4096))
            );
        }
        let balanced = mq::execute(
            &c,
            Action::Send {
                topic: name.clone(),
                partition: None,
                key: String::new(),
                body: "轮询队列".into(),
                headers: BTreeMap::new(),
                tags: String::new(),
                delay: 0,
            },
        )
        .await?;
        assert_eq!(balanced.detail["queueId"], "1");
        // SEND_OK acknowledges the commit log; the asynchronous consume queue
        // may not have published this new queue yet. Retry only this read.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let second = loop {
            let result = mq::execute(
                &c,
                Action::Messages {
                    topic: name.clone(),
                    query: Query::Offset {
                        partition: 1,
                        offset: 0,
                        limit: 10,
                    },
                },
            )
            .await?;
            if !result.rows.is_empty() {
                break result;
            }
            ensure!(
                tokio::time::Instant::now() < deadline,
                "Broker did not publish the acknowledged message in queue 1"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };
        assert_eq!(second.rows[0]["body"], "轮询队列");
        Ok::<_, anyhow::Error>(())
    }
    .await;
    let remove = mq::execute(
        &c,
        Action::DeleteTopic {
            topic: name.clone(),
        },
    )
    .await;
    work?;
    remove?;
    Ok(())
}

// Independent brokers validate the shared-transport batch, including a Kafka
// failure between two valid deletions, duplicate suppression and real refresh.
async fn actual_group_batch(c: &tomato_core::Connection, topic: &str) -> Result<()> {
    let prefix = format!("batch_{}", uuid::Uuid::new_v4().simple());
    let groups: Vec<_> = (0..3).map(|i| format!("{prefix}_{i}")).collect();
    for group in &groups {
        if c.kind() == "KAFKA" {
            seed::kafka_group(&std::env::var("TOMATO_KAFKA_FIXTURE")?, topic, group, 0, 0).await?;
        } else {
            let broker = std::env::var("TOMATO_ROCKET_BROKER_FIXTURE")?;
            seed::rocket_call(&broker,200,serde_json::json!({}),serde_json::json!({"groupName":group,"consumeEnable":true,"retryQueueNums":1,"retryMaxTimes":16,"brokerId":0})).await?;
            seed::rocket_offset(&broker, topic, group, 0).await?;
        }
    }
    let absent = format!("{prefix}_absent");
    let mut requested = vec![groups[0].clone()];
    if c.kind() == "KAFKA" {
        requested.push(absent.clone());
    }
    requested.extend(groups[1..].iter().cloned());
    requested.push(groups[0].clone());
    let report = mq::execute(c, Action::DeleteGroups { groups: requested }).await?;
    assert_eq!(report.detail["deleted"], serde_json::json!(groups));
    if c.kind() == "KAFKA" {
        assert_eq!(report.detail["failed"].as_array().unwrap().len(), 1);
        assert_eq!(report.detail["failed"][0]["group"], absent);
    } else {
        assert!(report.detail["failed"].as_array().unwrap().is_empty());
    }
    let refreshed = mq::execute(c, Action::Groups).await?;
    assert!(
        refreshed
            .rows
            .iter()
            .all(|row| !groups.iter().any(|group| row["group"] == *group))
    );
    Ok(())
}
