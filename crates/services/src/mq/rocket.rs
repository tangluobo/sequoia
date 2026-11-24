//! RocketMQ remoting/commit-log adapter; no Java process or HTTP proxy is required.
use super::*;
use anyhow::{Context, bail};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[derive(Debug)]
struct Reply {
    code: i64,
    fields: Value,
    body: Vec<u8>,
    remark: String,
}
impl Reply {
    fn checked(self) -> Result<Self> {
        ensure!(self.code == 0, "RocketMQ {}: {}", self.code, self.remark);
        Ok(self)
    }
    fn json(self) -> Result<Value> {
        let reply = self.checked()?;
        parse_json(&reply.body)
    }
}
struct Client {
    transport: transport::Transport,
    names: Vec<String>,
    opaque: i32,
}
impl Client {
    fn new(c: &Connection) -> Self {
        Self {
            transport: transport::Transport::new(c),
            names: transport::bootstrap(c, ';'),
            opaque: 0,
        }
    }
    async fn call(&mut self, addr: &str, code: i32, fields: Value, body: Vec<u8>) -> Result<Reply> {
        let mut socket = self.transport.dial(addr).await?;
        self.opaque += 1;
        let opaque = self.opaque;
        let ext: BTreeMap<String, String> = fields
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| {
                (
                    k.clone(),
                    if let Value::String(s) = v {
                        s.clone()
                    } else {
                        v.to_string()
                    },
                )
            })
            .collect();
        let header = serde_json::to_vec(
            &json!({"code":code,"language":"OTHER","version":433,"opaque":opaque,"flag":0,"extFields":ext,"serializeTypeCurrentRPC":"JSON"}),
        )?;
        ensure!(
            header.len() + body.len() <= 4 * 1024 * 1024,
            "RocketMQ 请求超过 4 MiB"
        );
        tokio::time::timeout(Duration::from_secs(15), async {
            socket
                .write_u32((4 + header.len() + body.len()) as u32)
                .await?;
            socket.write_u32(header.len() as u32).await?;
            socket.write_all(&header).await?;
            socket.write_all(&body).await?;
            let total = socket.read_u32().await? as usize;
            ensure!(
                (4..=16 * 1024 * 1024).contains(&total),
                "RocketMQ 响应长度无效"
            );
            let mut b = vec![0; total];
            socket.read_exact(&mut b).await?;
            let marker = u32::from_be_bytes(b[..4].try_into()?);
            ensure!(marker >> 24 == 0, "RocketMQ 返回非 JSON 报文头");
            let n = (marker & 0xffffff) as usize;
            ensure!(n <= total - 4, "RocketMQ 报文头截断");
            let h: Value = serde_json::from_slice(&b[4..4 + n])?;
            ensure!(
                h["opaque"].as_i64() == Some(i64::from(opaque))
                    && h["flag"].as_i64().unwrap_or(0) & 1 == 1,
                "RocketMQ 响应序号或类型无效"
            );
            Ok(Reply {
                code: h["code"].as_i64().context("缺少响应码")?,
                fields: h["extFields"].clone(),
                body: b[4 + n..].to_vec(),
                remark: text(&h, "remark"),
            })
        })
        .await
        .context("RocketMQ 请求超时")?
    }
    async fn names(&mut self, code: i32, fields: Value) -> Result<Value> {
        let mut last = None;
        for addr in self.names.clone() {
            match self
                .call(&addr, code, fields.clone(), Vec::new())
                .await
                .and_then(Reply::json)
            {
                Ok(v) => return Ok(v),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("NameServer 地址为空")))
    }
    async fn route(&mut self, topic: &str) -> Result<Value> {
        self.names(105, json!({"topic":topic,"acceptStandardJsonOnly":true}))
            .await
    }
    async fn cluster(&mut self) -> Result<Value> {
        self.names(106, json!({})).await
    }
    async fn masters(&mut self) -> Result<Vec<(String, String)>> {
        let c = self.cluster().await?;
        let mut rows = Vec::new();
        for (name, v) in c["brokerAddrTable"]
            .as_object()
            .context("NameServer 没有返回 Broker 目录")?
        {
            if let Some(address) = v["brokerAddrs"]["0"].as_str() {
                rows.push((name.clone(), address.into()));
            }
        }
        ensure!(
            !rows.is_empty() && rows.len() <= 100,
            "未发现可写的 Master Broker 或数量超过 100"
        );
        Ok(rows)
    }
    async fn brokers(&mut self, topic: &str) -> Result<Vec<(String, String)>> {
        let r = self.route(topic).await?;
        let mut out = Vec::new();
        for b in r["brokerDatas"]
            .as_array()
            .context("主题没有 Broker 路由")?
        {
            if let Some(a) = b["brokerAddrs"]["0"].as_str() {
                out.push((text(b, "brokerName"), a.into()));
            }
        }
        ensure!(out.len() <= 100, "主题 Broker 过多");
        Ok(out)
    }
    async fn stats(&mut self, topic: &str) -> Result<Vec<Value>> {
        let mut rows = Vec::new();
        for (_, addr) in self.brokers(topic).await? {
            let response = self
                .call(&addr, 202, json!({"topic":topic}), Vec::new())
                .await?
                .json()?;
            append_rows(offset_table(&response, "offsetTable")?, &mut rows)?;
        }
        Ok(rows)
    }
    async fn commits(&mut self, group: &str) -> Result<Vec<Value>> {
        let mut rows = Vec::new();
        for (_, addr) in self.masters().await? {
            let r = self
                .call(&addr, 208, json!({"consumerGroup":group}), Vec::new())
                .await?;
            if r.code == 206 || r.code == 17 || r.code == 209 {
                continue;
            }
            let v = r.json()?;
            for mut row in offset_table(&v, "offsetTable")? {
                row["group"] = json!(group);
                let end = row["brokerOffset"].as_i64().unwrap_or(0);
                let begin = row["consumerOffset"].as_i64().unwrap_or(0);
                row["diff"] = json!((end - begin).max(0));
                rows.push(row);
            }
        }
        Ok(rows)
    }
    async fn groups(&mut self) -> Result<Vec<String>> {
        let mut groups = std::collections::BTreeSet::new();
        for (_, addr) in self.masters().await? {
            let v = self.call(&addr, 201, json!({}), Vec::new()).await?.json()?;
            for g in v["subscriptionGroupTable"]
                .as_object()
                .into_iter()
                .flat_map(|m| m.keys())
            {
                if !g.starts_with("tomato_") {
                    groups.insert(g.clone());
                    ensure!(groups.len() <= 1000, "消费组超过 1000");
                }
            }
        }
        Ok(groups.into_iter().collect())
    }
    async fn pull(
        &mut self,
        addr: &str,
        topic: &str,
        queue: i64,
        offset: i64,
        limit: usize,
        end: Option<i64>,
    ) -> Result<Vec<Value>> {
        let mut rows = Vec::new();
        let mut next = offset;
        let limit = limit.min(1000);
        let mut budget = 0;
        for _ in 0..64 {
            if rows.len() >= limit {
                break;
            }
            let r=self.call(addr,11,json!({"consumerGroup":"tomato_query","topic":topic,"queueId":queue,"queueOffset":next,"maxMsgNums":(limit-rows.len()).min(32),"sysFlag":4,"commitOffset":0,"suspendTimeoutMillis":0,"subscription":"*","subVersion":0,"expressionType":"TAG"}),Vec::new()).await?;
            if matches!(r.code, 19 | 21) {
                break;
            }
            if r.code != 0 && r.code != 20 {
                r.checked()?;
                unreachable!()
            }
            let new_offset = text(&r.fields, "nextBeginOffset")
                .parse::<i64>()
                .unwrap_or(next);
            if r.code == 0 {
                for record in decode_messages(&r.body)? {
                    budget += serde_json::to_vec(&record)?.len();
                    ensure!(budget <= 8 * 1024 * 1024, "消息正文超过 8 MiB");
                    if end.is_some_and(|end| record["storeTimestamp"].as_i64().unwrap_or(0) > end) {
                        return Ok(rows);
                    }
                    rows.push(record);
                    if rows.len() >= limit {
                        break;
                    }
                }
            }
            if new_offset <= next {
                break;
            }
            next = new_offset;
        }
        Ok(rows)
    }
    async fn query_key(&mut self, topic: &str, key: &str) -> Result<Vec<Value>> {
        self.query_key_since(topic, key, 3).await
    }
    async fn query_key_since(&mut self, topic: &str, key: &str, days: i64) -> Result<Vec<Value>> {
        let end = chrono::Utc::now().timestamp_millis();
        let begin = end - days * 24 * 3600 * 1000;
        let mut rows = Vec::new();
        for (_, a) in self.brokers(topic).await? {
            let r=self.call(&a,12,json!({"topic":topic,"key":key,"maxNum":64,"beginTimestamp":begin,"endTimestamp":end}),Vec::new()).await?;
            if r.code == 22 {
                continue;
            }
            let r = r.checked()?;
            append_rows(decode_messages(&r.body)?, &mut rows)?;
            ensure!(rows.len() <= 1000, "Key 消息结果过多");
        }
        Ok(rows)
    }
    async fn delayed(
        &mut self,
        topic: &str,
        begin: i64,
        end: i64,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let schedule = "SCHEDULE_TOPIC_XXXX";
        match self.route(schedule).await {
            Err(e) if e.to_string().contains("RocketMQ 17:") => return Ok(Vec::new()),
            Err(e) => return Err(e),
            _ => {}
        }
        let brokers = self.brokers(schedule).await?;
        let mut rows = Vec::new();
        for stat in self.stats(schedule).await? {
            if rows.len() >= limit {
                break;
            }
            let Some((_, addr)) = brokers
                .iter()
                .find(|(b, _)| *b == text(&stat, "brokerName"))
            else {
                continue;
            };
            let queue = stat["queueId"].as_i64().unwrap_or(0);
            let r=self.call(addr,29,json!({"topic":schedule,"queueId":queue,"timestamp":begin,"boundaryType":"LOWER"}),Vec::new()).await?.checked()?;
            let offset = text(&r.fields, "offset").parse().unwrap_or(0);
            append_rows(
                self.pull(addr, schedule, queue, offset, limit - rows.len(), Some(end))
                    .await?
                    .into_iter()
                    .filter(|v| {
                        v["topic"] == topic && v["storeTimestamp"].as_i64().unwrap_or(0) >= begin
                    }),
                &mut rows,
            )?;
        }
        Ok(rows)
    }
    async fn message(&mut self, topic: &str, id: &str) -> Result<Value> {
        if let Ok(bytes) = unhex(id)
            && (bytes.len() == 16 || bytes.len() == 28)
        {
            let wanted = address(&bytes[..bytes.len() - 8])?;
            let offset = i64::from_be_bytes(bytes[bytes.len() - 8..].try_into()?);
            let route = self.route(topic).await?;
            let known: Vec<_> = route["brokerDatas"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|b| {
                    b["brokerAddrs"]
                        .as_object()
                        .into_iter()
                        .flat_map(|m| m.values())
                })
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            // A producer's unique key also has 32 hex digits. Never dial an arbitrary
            // address decoded from it; offset IDs must belong to an advertised broker.
            for broker in known {
                let matches = tokio::net::lookup_host(&broker)
                    .await
                    .map(|mut list| list.any(|a| a == wanted))
                    .unwrap_or(false);
                if !matches {
                    continue;
                }
                let r = self
                    .call(&broker, 33, json!({"offset":offset}), Vec::new())
                    .await?;
                if r.code == 0
                    && let Some(v) = decode_messages(&r.body)?.into_iter().next()
                {
                    return Ok(v);
                }
            }
        }
        self.query_key_since(topic, id, 7)
            .await?
            .into_iter()
            .find(|v| text(v, "msgId") == id || text(v, "uniqueId") == id)
            .context("未找到消息")
    }
    async fn connection(&mut self, group: &str) -> Result<Option<(String, Value)>> {
        for (_, addr) in self.masters().await? {
            let r = self
                .call(&addr, 203, json!({"consumerGroup":group}), Vec::new())
                .await?;
            if r.code == 206 {
                continue;
            }
            return Ok(Some((addr, r.json()?)));
        }
        Ok(None)
    }
    async fn track(&mut self, topic: &str, id: &str) -> Result<Vec<Value>> {
        let message = self.message(topic, id).await?;
        let queue = message["queueId"].as_i64().context("消息缺少队列 ID")?;
        let offset = message["queueOffset"].as_i64().context("消息缺少 Offset")?;
        let mut groups = std::collections::BTreeSet::new();
        let brokers = self.brokers(topic).await?;
        for (_, addr) in &brokers {
            let v = self
                .call(addr, 300, json!({"topic":topic}), Vec::new())
                .await?
                .json()?;
            for g in v["groupList"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                groups.insert(g.to_owned());
                ensure!(groups.len() <= 1000, "消费组超过 1000");
            }
        }
        let mut rows = Vec::new();
        for group in groups {
            let mut kind = "UNKNOWN";
            let mut exception = String::new();
            match self.connection(&group).await {
                Ok(None) => kind = "NOT_ONLINE",
                Ok(Some((_, connection))) => {
                    if connection["consumeType"] == "CONSUME_ACTIVELY" {
                        kind = "PULL";
                    } else if connection["messageModel"] == "BROADCASTING" {
                        kind = "CONSUME_BROADCASTING";
                    } else {
                        match self.commits(&group).await {
                            Ok(commits) => {
                                let consumed = commits.iter().any(|p| {
                                    p["topic"] == topic
                                        && p["queueId"] == queue
                                        && p["consumerOffset"].as_i64().unwrap_or(0) > offset
                                        && brokers.iter().any(|(name, addr)| {
                                            p["brokerName"] == *name
                                                && *addr == text(&message, "storeHost")
                                        })
                                });
                                kind = if consumed {
                                    "CONSUMED"
                                } else {
                                    "NOT_CONSUME_YET"
                                };
                                if consumed
                                    && let Some(tags) =
                                        connection["subscriptionTable"][topic]["tagsSet"].as_array()
                                    && !tags.is_empty()
                                    && !tags.iter().any(|t| t == "*" || *t == message["tags"])
                                {
                                    kind = "CONSUMED_BUT_FILTERED";
                                }
                            }
                            Err(e) => exception = e.to_string(),
                        }
                    }
                }
                Err(e) => exception = e.to_string(),
            }
            rows.push(json!({"group":group,"trackType":kind,"exceptionDesc":exception}));
        }
        Ok(rows)
    }
}
pub async fn execute(c: &Connection, action: Action) -> Result<Output> {
    let mut client = Client::new(c);
    match action {
        Action::Topics => {
            let v = client.names(206, json!({})).await?;
            Ok(Output::rows(v["topicList"].as_array().into_iter().flatten().map(|x|json!({"topic":x,"topicType":if x.as_str().is_some_and(|s|s.starts_with('%')){"SYSTEM"}else{"NORMAL"}})).collect()))
        }
        Action::Route { topic } => Ok(Output::detail(client.route(&topic).await?)),
        Action::Stats { topic } => Ok(Output::rows(client.stats(&topic).await?)),
        Action::Cluster => {
            let v = client.cluster().await?;
            let mut rows = Vec::new();
            for (name, b) in v["brokerAddrTable"].as_object().into_iter().flatten() {
                for (id, a) in b["brokerAddrs"].as_object().into_iter().flatten() {
                    let mut row = json!({"brokerName":name,"brokerId":id,"address":a,"role":if id=="0"{"MASTER"}else{"SLAVE"}});
                    if let Some(a) = a.as_str() {
                        let stats = client.call(a, 28, json!({}), Vec::new()).await?.json()?;
                        row["version"] = stats["table"]["brokerVersionDesc"].clone();
                    }
                    rows.push(row);
                }
            }
            Ok(Output::rows(rows))
        }
        Action::Broker { address } => Ok(Output::detail(
            client
                .call(&address, 28, json!({}), Vec::new())
                .await?
                .json()?,
        )),
        Action::CreateTopic {
            topic, partitions, ..
        } => {
            ensure!(
                !topic.is_empty() && (1..=1024).contains(&partitions),
                "主题参数无效"
            );
            for (_, addr) in client.masters().await? {
                client.call(&addr,17,json!({"topic":topic,"defaultTopic":"TBW102","readQueueNums":partitions,"writeQueueNums":partitions,"perm":6,"topicFilterType":"SINGLE_TAG","topicSysFlag":0,"order":false,"attributes":""}),Vec::new()).await?.checked()?;
            }
            Ok(Output::changed("创建主题成功"))
        }
        Action::DeleteTopic { topic } => {
            for (_, addr) in client.masters().await? {
                client
                    .call(&addr, 215, json!({"topic":topic}), Vec::new())
                    .await?
                    .checked()?;
            }
            for addr in client.names.clone() {
                client
                    .call(&addr, 216, json!({"topic":topic}), Vec::new())
                    .await?
                    .checked()?;
            }
            Ok(Output::changed("删除主题成功"))
        }
        Action::Groups => {
            let mut rows = Vec::new();
            for g in client.groups().await? {
                let commits = client.commits(&g).await?;
                let diff: i64 = commits.iter().filter_map(|v| v["diff"].as_i64()).sum();
                rows.push(json!({"group":g,"diffTotal":diff}));
            }
            Ok(Output::rows(rows))
        }
        Action::Group { group } => {
            let rows = client.commits(&group).await?;
            let mut connections = Vec::new();
            for (_, addr) in client.masters().await? {
                let r = client
                    .call(&addr, 203, json!({"consumerGroup":group}), Vec::new())
                    .await?;
                if r.code == 206 {
                    continue;
                }
                let v = r.json()?;
                connections.extend(v["connectionSet"].as_array().into_iter().flatten().cloned());
            }
            let mut output = Output::rows(rows);
            output.detail = json!({"connections":connections});
            Ok(output)
        }
        Action::DeleteGroups { groups } => {
            let masters: Vec<_> = client
                .masters()
                .await?
                .into_iter()
                .map(|(_, address)| address)
                .collect();
            ensure!(!masters.is_empty(), "找不到可执行删除的 RocketMQ Broker");
            batch::run(
                groups,
                &mut GroupRemover {
                    client: &mut client,
                    masters,
                },
            )
            .await
        }
        Action::DeleteGroup { group } => {
            for (_, addr) in client.masters().await? {
                client
                    .call(
                        &addr,
                        207,
                        json!({"groupName":group,"removeOffset":true}),
                        Vec::new(),
                    )
                    .await?
                    .checked()?;
            }
            Ok(Output::changed("删除消费组成功"))
        }
        Action::ConsumeStatus { topic } => {
            let mut out = Vec::new();
            for group in client.groups().await? {
                out.extend(
                    client
                        .commits(&group)
                        .await?
                        .into_iter()
                        .filter(|v| v["topic"] == topic),
                );
            }
            Ok(Output::rows(out))
        }
        Action::Messages { topic, query } => {
            let mut rows = Vec::new();
            let range = if let Query::Time { begin, end } = &query {
                Some((*begin, *end))
            } else {
                None
            };
            match query {
                Query::Id(id) => rows.push(client.message(&topic, &id).await?),
                Query::Key(key) => rows = client.query_key(&topic, &key).await?,
                Query::Offset {
                    partition,
                    offset,
                    limit,
                } => {
                    let brokers = client.brokers(&topic).await?;
                    let (_, addr) = brokers.first().context("主题没有 Broker")?;
                    rows = client
                        .pull(addr, &topic, i64::from(partition), offset, limit, None)
                        .await?;
                }
                Query::Time { begin, end } => {
                    ensure!(begin <= end, "开始时间不能晚于结束时间");
                    let brokers = client.brokers(&topic).await?;
                    for stat in client.stats(&topic).await? {
                        if rows.len() >= 256 {
                            break;
                        }
                        let Some((_, addr)) = brokers
                            .iter()
                            .find(|(b, _)| *b == text(&stat, "brokerName"))
                        else {
                            continue;
                        };
                        let q = stat["queueId"].as_i64().unwrap_or(0);
                        let r=client.call(addr,29,json!({"topic":topic,"queueId":q,"timestamp":begin,"boundaryType":"LOWER"}),Vec::new()).await?.checked()?;
                        let offset = text(&r.fields, "offset").parse().unwrap_or(0);
                        append_rows(
                            client
                                .pull(addr, &topic, q, offset, 256 - rows.len(), Some(end))
                                .await?,
                            &mut rows,
                        )?;
                    }
                }
                Query::Unconsumed { group, limit } => {
                    let brokers = client.brokers(&topic).await?;
                    for stat in client
                        .commits(&group)
                        .await?
                        .into_iter()
                        .filter(|v| v["topic"] == topic)
                    {
                        if rows.len() >= limit.min(1000) {
                            break;
                        }
                        let Some((_, addr)) = brokers
                            .iter()
                            .find(|(b, _)| *b == text(&stat, "brokerName"))
                        else {
                            continue;
                        };
                        let q = stat["queueId"].as_i64().unwrap_or(0);
                        let offset = stat["consumerOffset"].as_i64().unwrap_or(0).max(0);
                        append_rows(
                            client
                                .pull(addr, &topic, q, offset, limit.min(1000) - rows.len(), None)
                                .await?,
                            &mut rows,
                        )?;
                    }
                }
            }
            let mut notice = String::new();
            if let Some((begin, end)) = range {
                rows.retain(|r| {
                    r["storeTimestamp"]
                        .as_i64()
                        .is_some_and(|t| t >= begin && t <= end)
                });
                if rows.len() < 256 {
                    match client.delayed(&topic, begin, end, 256 - rows.len()).await {
                        Ok(delayed) => append_rows(delayed, &mut rows)?,
                        Err(e) => notice = format!("普通消息已读取，延迟队列查询失败：{e}"),
                    }
                }
            }
            let mut output = Output::rows(rows);
            output.message = notice;
            Ok(output)
        }
        Action::Send {
            topic,
            key,
            body,
            tags,
            delay,
            ..
        } => {
            ensure!((0..=18).contains(&delay), "延迟级别应为 0–18");
            let route = client.route(&topic).await?;
            let queues = route["queueDatas"].as_array().context("主题没有队列")?;
            let writable = |q: &&Value| {
                q["perm"].as_i64().unwrap_or(0) & 2 != 0
                    && q["writeQueueNums"].as_u64().unwrap_or(0) > 0
            };
            let total = queues.iter().filter(writable).try_fold(0u64, |total, q| {
                total
                    .checked_add(q["writeQueueNums"].as_u64().unwrap_or(0))
                    .context("队列总数溢出")
            })?;
            ensure!(
                total > 0 && total <= 10000,
                "主题没有可写队列或队列数量超限"
            );
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let mut index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % total;
            let (queue, queue_id) = queues
                .iter()
                .filter(writable)
                .find_map(|q| {
                    let count = q["writeQueueNums"].as_u64().unwrap_or(0);
                    if index < count {
                        Some((q, index))
                    } else {
                        index -= count;
                        None
                    }
                })
                .context("主题没有可写队列")?;
            let broker = route["brokerDatas"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|b| b["brokerName"] == queue["brokerName"])
                .context("主题没有 Broker")?;
            let addr = broker["brokerAddrs"]["0"]
                .as_str()
                .context("主题没有 Master Broker")?;
            let id = uuid::Uuid::new_v4().simple().to_string().to_uppercase();
            let mut props = vec![("UNIQ_KEY", id.clone()), ("KEYS", key), ("TAGS", tags)];
            if delay > 0 {
                props.push(("DELAY", delay.to_string()));
            }
            let properties = props
                .into_iter()
                .map(|(k, v)| {
                    ensure!(!v.contains(['\u{1}', '\u{2}']), "消息属性含非法分隔符");
                    Ok(format!("{k}\u{1}{v}\u{2}"))
                })
                .collect::<Result<Vec<_>>>()?
                .join("");
            let r=client.call(addr,10,json!({"producerGroup":"tomato_producer","topic":topic,"defaultTopic":"TBW102","defaultTopicQueueNums":4,"queueId":queue_id,"sysFlag":0,"bornTimestamp":chrono::Utc::now().timestamp_millis(),"flag":0,"properties":properties,"reconsumeTimes":0,"unitMode":false,"batch":false}),body.into_bytes()).await?.checked()?;
            let mut out = Output::changed("发送成功");
            out.detail = json!({"msgId":r.fields["msgId"],"uniqueId":id,"queueId":r.fields["queueId"],"queueOffset":r.fields["queueOffset"]});
            Ok(out)
        }
        Action::Track { topic, id } => Ok(Output::rows(client.track(&topic, &id).await?)),
        Action::Reconsume { topic, group, id } => {
            let message = client.message(&topic, &id).await?;
            for (_, addr) in client.brokers(&topic).await? {
                let reply = client
                    .call(&addr, 203, json!({"consumerGroup":group}), Vec::new())
                    .await?;
                if reply.code == 206 {
                    continue;
                }
                let v = reply.json()?;
                if let Some(connection) = v["connectionSet"].as_array().and_then(|a| a.first()) {
                    let v=client.call(&addr,309,json!({"consumerGroup":group,"clientId":connection["clientId"],"msgId":message["msgId"],"topic":topic}),Vec::new()).await?.json()?;
                    return Ok(Output::detail(v));
                }
            }
            bail!("消费者组没有在线的消费者实例")
        }
    }
}
fn offset_table(value: &Value, name: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for (k, v) in value[name].as_object().into_iter().flatten() {
        let mut row: Value = serde_json::from_str(k).context("RocketMQ 队列键无效")?;
        if let (Some(a), Some(b)) = (row.as_object_mut(), v.as_object()) {
            a.extend(b.clone());
        }
        out.push(row);
    }
    Ok(out)
}
fn unhex(s: &str) -> Result<Vec<u8>> {
    ensure!(s.len().is_multiple_of(2) && s.len() <= 128, "消息 ID 无效");
    s.as_bytes()
        .chunks(2)
        .map(|x| Ok(u8::from_str_radix(std::str::from_utf8(x)?, 16)?))
        .collect()
}
/// Fastjson's map keys can be objects. Normalize those keys without modifying values.
pub fn parse_json(b: &[u8]) -> Result<Value> {
    struct Parser<'a> {
        b: &'a [u8],
        p: usize,
    }
    impl Parser<'_> {
        fn ws(&mut self) {
            while self.b.get(self.p).is_some_and(u8::is_ascii_whitespace) {
                self.p += 1;
            }
        }
        fn take(&mut self, c: u8) -> Result<()> {
            self.ws();
            ensure!(self.b.get(self.p) == Some(&c), "RocketMQ JSON 语法错误");
            self.p += 1;
            Ok(())
        }
        fn value(&mut self, depth: usize) -> Result<Value> {
            ensure!(depth < 64, "RocketMQ JSON 嵌套过深");
            self.ws();
            let c = *self.b.get(self.p).context("RocketMQ JSON 截断")?;
            if c == b'{' {
                self.p += 1;
                self.ws();
                let mut m = serde_json::Map::new();
                if self.b.get(self.p) == Some(&b'}') {
                    self.p += 1;
                    return Ok(Value::Object(m));
                }
                loop {
                    let k = self.value(depth + 1)?;
                    let k = if let Value::String(s) = k {
                        s
                    } else {
                        serde_json::to_string(&k)?
                    };
                    self.take(b':')?;
                    m.insert(k, self.value(depth + 1)?);
                    self.ws();
                    if self.b.get(self.p) == Some(&b'}') {
                        self.p += 1;
                        break;
                    }
                    self.take(b',')?;
                }
                Ok(Value::Object(m))
            } else if c == b'[' {
                self.p += 1;
                self.ws();
                let mut a = Vec::new();
                if self.b.get(self.p) == Some(&b']') {
                    self.p += 1;
                    return Ok(Value::Array(a));
                }
                loop {
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    if self.b.get(self.p) == Some(&b']') {
                        self.p += 1;
                        break;
                    }
                    self.take(b',')?;
                }
                Ok(Value::Array(a))
            } else {
                let start = self.p;
                if c == b'"' {
                    self.p += 1;
                    loop {
                        let c = *self.b.get(self.p).context("RocketMQ 字符串截断")?;
                        self.p += 1;
                        if c == b'\\' {
                            self.p += 1;
                        } else if c == b'"' {
                            break;
                        }
                    }
                } else {
                    while self
                        .b
                        .get(self.p)
                        .is_some_and(|b| !b.is_ascii_whitespace() && !b",]}:".contains(b))
                    {
                        self.p += 1;
                    }
                }
                Ok(serde_json::from_slice(&self.b[start..self.p])?)
            }
        }
    }
    let mut p = Parser { b, p: 0 };
    let v = p.value(0)?;
    p.ws();
    ensure!(p.p == b.len(), "RocketMQ JSON 有额外内容");
    Ok(v)
}
pub fn decode_messages(mut b: &[u8]) -> Result<Vec<Value>> {
    use std::io::Read;
    let mut out = Vec::new();
    let mut budget = 0;
    while !b.is_empty() {
        ensure!(b.len() >= 4, "RocketMQ 消息截断");
        let n = i32::from_be_bytes(b[..4].try_into()?) as usize;
        ensure!(
            n >= 91 && n <= b.len() && n <= 4 * 1024 * 1024,
            "RocketMQ 消息长度无效"
        );
        let mut c = bytes::Bytes::copy_from_slice(&b[..n]);
        b = &b[n..];
        use bytes::Buf;
        c.advance(4);
        let magic = c.get_i32();
        ensure!(
            magic == -626843481 || magic == -626843477,
            "RocketMQ 消息魔数无效"
        );
        let _crc = c.get_u32();
        let queue = c.get_i32();
        let flag = c.get_i32();
        let offset = c.get_i64();
        let commit = c.get_i64();
        let sys = c.get_i32();
        let born = c.get_i64();
        let born_len = if sys & 16 != 0 { 20 } else { 8 };
        ensure!(c.remaining() >= born_len + 8, "RocketMQ bornHost 截断");
        let born_host = c.copy_to_bytes(born_len);
        let stored = c.get_i64();
        let host_len = if sys & 32 != 0 { 20 } else { 8 };
        ensure!(c.remaining() >= host_len + 16, "RocketMQ storeHost 截断");
        let store_host = c.copy_to_bytes(host_len);
        let reconsume = c.get_i32();
        c.advance(8);
        let size = c.get_i32();
        ensure!(
            size >= 0 && size as usize <= c.remaining(),
            "RocketMQ 正文长度无效"
        );
        let body = c.copy_to_bytes(size as usize);
        let body = if sys & 1 != 0 {
            let reader: Box<dyn Read> = match (sys >> 8) & 7 {
                1 => Box::new(lz4::Decoder::new(body.as_ref())?),
                2 => Box::new(zstd::stream::read::Decoder::new(body.as_ref())?),
                _ => Box::new(flate2::read::ZlibDecoder::new(body.as_ref())),
            };
            let mut bytes = Vec::new();
            reader.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 4 * 1024 * 1024,
                "RocketMQ 正文解压超过 4 MiB"
            );
            bytes
        } else {
            body.to_vec()
        };
        budget += body.len();
        ensure!(budget <= 8 * 1024 * 1024, "RocketMQ 正文超过 8 MiB");
        ensure!(c.remaining() >= 2, "RocketMQ 主题截断");
        let topic_len = if magic == -626843477 {
            c.get_u16() as usize
        } else {
            c.get_u8() as usize
        };
        ensure!(topic_len + 2 <= c.remaining(), "RocketMQ 主题长度无效");
        let topic = c.copy_to_bytes(topic_len);
        let prop_len = c.get_u16() as usize;
        ensure!(prop_len <= c.remaining(), "RocketMQ 属性截断");
        let props = c.copy_to_bytes(prop_len);
        let props = String::from_utf8_lossy(&props).into_owned();
        let mut properties = BTreeMap::new();
        for part in props.split('\u{2}') {
            if let Some((k, v)) = part.split_once('\u{1}') {
                properties.insert(k, v);
            }
        }
        let mut id = store_host.to_vec();
        id.extend_from_slice(&commit.to_be_bytes());
        let id = id.iter().map(|b| format!("{b:02X}")).collect::<String>();
        out.push(json!({"msgId":id,"uniqueId":properties.get("UNIQ_KEY"),"topic":properties.get("REAL_TOPIC").copied().unwrap_or(std::str::from_utf8(&topic)?),"queueId":queue,"queueOffset":offset,"commitLogOffset":commit,"flag":flag,"storeSize":n,"bornTimestamp":born,"storeTimestamp":stored,"timestamp":stored,"bornHost":address(&born_host)?.to_string(),"storeHost":address(&store_host)?.to_string(),"body":String::from_utf8_lossy(&body),"keys":properties.get("KEYS"),"tags":properties.get("TAGS"),"reconsumeTimes":reconsume,"delayed":properties.contains_key("REAL_TOPIC"),"properties":properties}));
        ensure!(out.len() <= 1000, "RocketMQ 消息条数过多");
    }
    Ok(out)
}

fn address(bytes: &[u8]) -> Result<std::net::SocketAddr> {
    ensure!(
        bytes.len() == 8 || bytes.len() == 20,
        "RocketMQ 地址长度无效"
    );
    let len = bytes.len() - 4;
    let ip = if len == 4 {
        std::net::IpAddr::V4(std::net::Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[..4])?))
    } else {
        std::net::IpAddr::V6(std::net::Ipv6Addr::from(<[u8; 16]>::try_from(
            &bytes[..16],
        )?))
    };
    Ok(std::net::SocketAddr::new(
        ip,
        u16::try_from(u32::from_be_bytes(bytes[len..].try_into()?))?,
    ))
}

struct GroupRemover<'a> {
    client: &'a mut Client,
    masters: Vec<String>,
}
impl batch::Remove for GroupRemover<'_> {
    async fn remove(&mut self, group: &str) -> Result<()> {
        for address in &self.masters {
            self.client
                .call(
                    address,
                    207,
                    json!({"groupName":group,"removeOffset":true}),
                    Vec::new(),
                )
                .await?
                .checked()?;
        }
        Ok(())
    }
}
