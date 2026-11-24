//! Provider adapters: Aliyun RPC HMAC-SHA1 and Tencent TC3-HMAC-SHA256.
use super::*;
use anyhow::{Context, bail};
use base64::Engine;
use ring::{digest, hmac};
use std::{collections::BTreeMap, time::Duration};
pub const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_ROWS: usize = 10000;
#[derive(Clone)]
pub struct Client {
    connection: Connection,
    http: reqwest::Client,
}
impl Client {
    pub fn new(c: &Connection) -> Result<Self> {
        ensure!(
            matches!(c.kind(), "ALIYUN" | "TENCENT_CLOUD"),
            "不是云平台连接"
        );
        ensure!(
            !c.text("username").is_empty() && !c.text("password").is_empty(),
            "请填写云平台访问密钥"
        );
        crate::init_crypto();
        Ok(Self {
            connection: c.clone(),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .build()?,
        })
    }
    fn endpoint(&self, host: &str) -> Result<String> {
        let custom = self.connection.text("cloudApiEndpoint");
        if custom.is_empty() {
            return Ok(format!("https://{host}/"));
        }
        let url = url::Url::parse(custom)?;
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && url
                        .host_str()
                        .and_then(|s| s.parse::<std::net::IpAddr>().ok())
                        .is_some_and(|ip| ip.is_loopback())),
            "云 API 端点需要 HTTPS"
        );
        Ok(custom.into())
    }
    pub async fn call(
        &self,
        service: &str,
        version: &str,
        action: &str,
        region: &str,
        params: Value,
    ) -> Result<Value> {
        let c = &self.connection;
        let request = if c.kind() == "TENCENT_CLOUD" {
            let host = format!("{service}.tencentcloudapi.com");
            let now = chrono::Utc::now();
            let timestamp = now.timestamp();
            let date = now.format("%Y-%m-%d").to_string();
            let body = serde_json::to_string(&params)?;
            let auth = tencent_signature(
                c.text("username"),
                c.text("password"),
                service,
                &host,
                &body,
                timestamp,
                &date,
            );
            let mut req = self
                .http
                .post(self.endpoint(&host)?)
                .header("host", host)
                .header("authorization", auth)
                .header("content-type", "application/json; charset=utf-8")
                .header("x-tc-action", action)
                .header("x-tc-version", version)
                .header("x-tc-timestamp", timestamp.to_string())
                .body(body);
            if !region.is_empty() {
                req = req.header("x-tc-region", region);
            }
            if !c.text("securityToken").is_empty() {
                req = req.header("x-tc-token", c.text("securityToken"));
            }
            req
        } else {
            let host = if service == "domain" {
                "domain.aliyuncs.com".into()
            } else {
                format!("{service}.aliyuncs.com")
            };
            let mut p: BTreeMap<String, String> =
                serde_json::from_value::<BTreeMap<String, Value>>(params)?
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            if let Value::String(s) = v {
                                s
                            } else {
                                v.to_string()
                            },
                        )
                    })
                    .collect();
            for (k, v) in [
                ("AccessKeyId", c.text("username").into()),
                ("Action", action.into()),
                ("Version", version.into()),
                ("Format", "JSON".into()),
                ("SignatureMethod", "HMAC-SHA1".into()),
                ("SignatureVersion", "1.0".into()),
                ("SignatureNonce", uuid::Uuid::new_v4().to_string()),
                (
                    "Timestamp",
                    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                ),
            ] {
                p.insert(k.into(), v);
            }
            if !region.is_empty() {
                p.insert("RegionId".into(), region.into());
            }
            if !c.text("securityToken").is_empty() {
                p.insert("SecurityToken".into(), c.text("securityToken").into());
            }
            let canonical = query(&p);
            let to_sign = format!("GET&%2F&{}", escape(&canonical));
            let key = hmac::Key::new(
                hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
                format!("{}&", c.text("password")).as_bytes(),
            );
            p.insert(
                "Signature".into(),
                base64::engine::general_purpose::STANDARD
                    .encode(hmac::sign(&key, to_sign.as_bytes()).as_ref()),
            );
            self.http.get(format!(
                "{}?{}",
                self.endpoint(&host)?.trim_end_matches('?'),
                query(&p)
            ))
        };
        let response = request
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("云 API 请求失败")?;
        let status = response.status();
        let value = bounded_json(response).await?;
        let value = if c.kind() == "TENCENT_CLOUD" {
            value
                .get("Response")
                .cloned()
                .context("腾讯云响应缺少 Response")?
        } else {
            value
        };
        if !status.is_success()
            || value.get("Error").is_some()
            || (value.get("Code").is_some() && value.get("RequestId").is_some())
        {
            let err = value.get("Error").unwrap_or(&value);
            // Only provider error fields, never request URLs, headers or credentials.
            bail!("{}: {}", text(err, "Code"), text(err, "Message"));
        }
        Ok(value)
    }
    pub async fn execute(&self, action: Action) -> Result<Output> {
        let tc = self.connection.kind() == "TENCENT_CLOUD";
        let mut rows = Vec::new();
        let mut retained = 0usize;
        let mut message = String::new();
        match action {
            Action::SetDdns { .. } => anyhow::bail!("DDNS 需要持久化控制器"),
            Action::Verify => {
                if tc {
                    self.call(
                        "sts",
                        "2018-08-13",
                        "GetCallerIdentity",
                        "ap-guangzhou",
                        json!({}),
                    )
                    .await?;
                } else {
                    self.regions().await?;
                }
                message = "连接成功".into();
            }
            Action::Regions => {
                rows = self.regions().await?;
            }
            Action::Instances { region } => {
                use futures_util::StreamExt;
                let all = region.is_empty();
                let regions: Vec<String> = if all {
                    self.regions()
                        .await?
                        .iter()
                        .map(|v| text(v, "regionId"))
                        .collect()
                } else {
                    vec![region]
                };
                let mut pending =
                    futures_util::stream::iter(regions.into_iter().map(|region| async move {
                        let result = self.region_instances(&region).await;
                        (region, result)
                    }))
                    .buffer_unordered(4);
                let mut failed = Vec::new();
                while let Some((region, result)) = pending.next().await {
                    match result {
                        Ok(values) => {
                            for row in values {
                                append(&mut rows, row, &mut retained)?;
                            }
                        }
                        Err(e) => {
                            if !all {
                                return Err(e);
                            }
                            failed.push(region);
                        }
                    }
                }
                ensure!(
                    !rows.is_empty() || failed.is_empty(),
                    "无法查询云实例地域：{}",
                    failed.join(", ")
                );
                if !failed.is_empty() {
                    message = format!("部分地域未能读取：{}", failed.join(", "));
                }
            }
            Action::Domains => {
                let page_size = 100;
                let mut offset = 0;
                loop {
                    let v = if tc {
                        self.call(
                            "dnspod",
                            "2021-03-23",
                            "DescribeDomainList",
                            "",
                            json!({"Limit":page_size,"Offset":offset}),
                        )
                        .await?
                    } else {
                        self.call(
                            "domain",
                            "2018-01-29",
                            "QueryDomainList",
                            "cn-hangzhou",
                            json!({"PageNum":offset/page_size+1,"PageSize":page_size,"Lang":"zh"}),
                        )
                        .await?
                    };
                    let source = if tc {
                        v["DomainList"].as_array()
                    } else {
                        v["Data"]["Domain"]
                            .as_array()
                            .or_else(|| v["Data"].as_array())
                    }
                    .cloned()
                    .unwrap_or_default();
                    let len = source.len();
                    for v in source {
                        let mut out = v.clone();
                        out["domainName"] = v[if tc { "Name" } else { "DomainName" }].clone();
                        out["instanceId"] = v[if tc { "DomainId" } else { "InstanceId" }].clone();
                        out["status"] = v[if tc { "Status" } else { "DomainStatus" }].clone();
                        append(&mut rows, out, &mut retained)?;
                        ensure!(rows.len() <= MAX_ROWS, "域名数量超过内存上限");
                    }
                    if len < page_size {
                        break;
                    }
                    offset += page_size;
                }
            }
            Action::Records { domain } => {
                let mut offset = 0;
                loop {
                    let v = if tc {
                        self.call(
                            "dnspod",
                            "2021-03-23",
                            "DescribeRecordList",
                            "",
                            json!({"Domain":domain,"Limit":100,"Offset":offset}),
                        )
                        .await?
                    } else {
                        self.call("alidns","2015-01-09","DescribeDomainRecords","cn-hangzhou",json!({"DomainName":domain,"PageSize":100,"PageNumber":offset/100+1,"Lang":"zh"})).await?
                    };
                    let source = if tc {
                        v["RecordList"].as_array()
                    } else {
                        v["DomainRecords"]["Record"].as_array()
                    }
                    .cloned()
                    .unwrap_or_default();
                    let len = source.len();
                    for v in source {
                        append(
                            &mut rows,
                            serde_json::to_value(Record {
                                record_id: text(&v, "RecordId"),
                                rr: text(&v, if tc { "Name" } else { "RR" }),
                                kind: text(&v, "Type"),
                                value: text(&v, "Value"),
                                ttl: v["TTL"].as_u64().unwrap_or(600),
                                line: text(&v, "Line"),
                                priority: v[if tc { "MX" } else { "Priority" }]
                                    .as_u64()
                                    .unwrap_or(1),
                                status: text(&v, "Status"),
                            })?,
                            &mut retained,
                        )?;
                        ensure!(rows.len() <= MAX_ROWS, "解析记录超过内存上限");
                    }
                    if len < 100 {
                        break;
                    }
                    offset += 100;
                }
            }
            Action::SaveRecord { domain, record } => {
                let id = self.save_record(&domain, &record).await?;
                append(&mut rows, json!({"recordId":id}), &mut retained)?;
                message = "保存成功".into();
            }
            Action::DeleteRecord { domain, id } => {
                self.delete_record(&domain, &id).await?;
                message = "删除成功".into();
            }
        }
        ensure!(
            serde_json::to_vec(&rows)?.len() <= MAX_BODY,
            "云查询结果超过内存上限"
        );
        Ok(Output { rows, message })
    }
    async fn region_instances(&self, region: &str) -> Result<Vec<Value>> {
        let tc = self.connection.kind() == "TENCENT_CLOUD";
        let mut rows = Vec::new();
        let mut retained = 0usize;

        let mut offset = 0;
        loop {
            let response = if tc {
                self.call(
                    "cvm",
                    "2017-03-12",
                    "DescribeInstances",
                    region,
                    json!({"Limit":100,"Offset":offset}),
                )
                .await?
            } else {
                self.call(
                    "ecs",
                    "2014-05-26",
                    "DescribeInstances",
                    region,
                    json!({"PageSize":100,"PageNumber":offset/100+1}),
                )
                .await?
            };
            let source = if tc {
                response["InstanceSet"].as_array()
            } else {
                response["Instances"]["Instance"].as_array()
            };
            let source = source.cloned().unwrap_or_default();
            let len = source.len();
            for v in source {
                let mut out = json!({});
                for (key, source) in [
                    ("instanceId", "InstanceId"),
                    ("instanceName", "InstanceName"),
                    ("status", if tc { "InstanceState" } else { "Status" }),
                    ("instanceType", "InstanceType"),
                    ("cpu", if tc { "CPU" } else { "Cpu" }),
                    ("memory", "Memory"),
                    ("osName", if tc { "OsName" } else { "OSName" }),
                    (
                        "createdTime",
                        if tc { "CreatedTime" } else { "CreationTime" },
                    ),
                    ("expiredTime", "ExpiredTime"),
                ] {
                    if tc
                        || matches!(
                            key,
                            "instanceId" | "instanceName" | "status" | "instanceType"
                        )
                    {
                        out[key] = v[source].clone();
                    }
                }
                out[if tc { "region" } else { "regionId" }] = json!(region);
                if tc {
                    out["zone"] = v["Placement"]["Zone"].clone();
                }
                out["publicIp"] = if tc {
                    v["PublicIpAddresses"][0].clone()
                } else {
                    v["PublicIpAddress"]["IpAddress"][0].clone()
                };
                out["privateIp"] = if tc {
                    v["PrivateIpAddresses"][0].clone()
                } else {
                    v["VpcAttributes"]["PrivateIpAddress"]["IpAddress"][0].clone()
                };
                append(&mut rows, out, &mut retained)?;
                ensure!(rows.len() <= MAX_ROWS, "云实例超过内存上限，请按地域查询");
            }
            if len < 100 {
                break;
            }
            offset += 100;
        }
        Ok(rows)
    }
    async fn regions(&self) -> Result<Vec<Value>> {
        let tc = self.connection.kind() == "TENCENT_CLOUD";
        let v = if tc {
            self.call(
                "cvm",
                "2017-03-12",
                "DescribeRegions",
                "ap-guangzhou",
                json!({}),
            )
            .await?
        } else {
            self.call(
                "ecs",
                "2014-05-26",
                "DescribeRegions",
                "cn-hangzhou",
                json!({}),
            )
            .await?
        };
        Ok(if tc{v["RegionSet"].as_array()}else{v["Regions"]["Region"].as_array()}.into_iter().flatten().filter(|x|!tc||text(x,"RegionState").is_empty()||text(x,"RegionState")=="AVAILABLE").map(|x|json!({"regionId":x[if tc{"Region"}else{"RegionId"}],"name":x[if tc{"RegionName"}else{"LocalName"}]})).collect())
    }
    pub async fn save_record(&self, domain: &str, r: &Record) -> Result<String> {
        ensure!(
            !domain.trim().is_empty()
                && !r.rr.trim().is_empty()
                && !r.kind.is_empty()
                && !r.value.is_empty(),
            "域名、主机记录、类型和记录值不能为空"
        );
        ensure!((1..=604800).contains(&r.ttl), "TTL 范围无效");
        let tc = self.connection.kind() == "TENCENT_CLOUD";
        let edit = !r.record_id.is_empty();
        let mut body = if tc {
            let line = if r.line.trim().is_empty() || r.line.eq_ignore_ascii_case("default") {
                "默认"
            } else {
                &r.line
            };
            let mut body = json!({
                "Domain": domain, "SubDomain": r.rr, "RecordType": r.kind,
                "Value": r.value, "TTL": r.ttl, "RecordLine": line
            });
            if matches!(r.kind.as_str(), "MX" | "HTTPS" | "SVCB") {
                body["MX"] = json!(r.priority);
            }
            body
        } else {
            json!({"DomainName":domain,"RR":r.rr,"Type":r.kind,"Value":r.value,"TTL":r.ttl,"Lang":"zh"})
        };
        if edit {
            body["RecordId"] = if tc {
                json!(r.record_id.parse::<u64>().context("DNSPod 记录 ID 无效")?)
            } else {
                json!(r.record_id)
            };
        }
        if !tc {
            if !r.line.is_empty() {
                body["Line"] = json!(r.line);
            }
            if r.kind == "MX" {
                body["Priority"] = json!(r.priority);
            }
        }
        let v = if tc {
            self.call(
                "dnspod",
                "2021-03-23",
                if edit { "ModifyRecord" } else { "CreateRecord" },
                "",
                body,
            )
            .await?
        } else {
            self.call(
                "alidns",
                "2015-01-09",
                if edit {
                    "UpdateDomainRecord"
                } else {
                    "AddDomainRecord"
                },
                "cn-hangzhou",
                body,
            )
            .await?
        };
        let id = text(&v, "RecordId");
        if id.is_empty() && edit {
            return Ok(r.record_id.clone());
        }
        ensure!(!id.is_empty(), "云平台创建记录后未返回记录 ID");
        Ok(id)
    }
    pub async fn delete_record(&self, domain: &str, id: &str) -> Result<()> {
        if self.connection.kind() == "TENCENT_CLOUD" {
            self.call(
                "dnspod",
                "2021-03-23",
                "DeleteRecord",
                "",
                json!({"Domain":domain,"RecordId":id.parse::<u64>()?}),
            )
            .await?;
        } else {
            self.call(
                "alidns",
                "2015-01-09",
                "DeleteDomainRecord",
                "cn-hangzhou",
                json!({"RecordId":id}),
            )
            .await?;
        }
        Ok(())
    }
}
pub async fn bounded_json(mut response: reqwest::Response) -> Result<Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)?
    {
        ensure!(
            bytes.len() + chunk.len() <= MAX_BODY,
            "云 API 响应超过 8 MiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).context("云 API 未返回 JSON")
}
pub fn escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char)
        } else {
            use std::fmt::Write;
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}
fn query(p: &BTreeMap<String, String>) -> String {
    p.iter()
        .map(|(k, v)| format!("{}={}", escape(k), escape(v)))
        .collect::<Vec<_>>()
        .join("&")
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn hash(s: &str) -> String {
    hex(digest::digest(&digest::SHA256, s.as_bytes()).as_ref())
}
fn mac(key: &[u8], s: &str) -> Vec<u8> {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), s.as_bytes())
        .as_ref()
        .to_vec()
}
pub fn tencent_signature(
    id: &str,
    secret: &str,
    service: &str,
    host: &str,
    body: &str,
    timestamp: i64,
    date: &str,
) -> String {
    let request = format!(
        "POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:{host}\n\ncontent-type;host\n{}",
        hash(body)
    );
    let scope = format!("{date}/{service}/tc3_request");
    let sign = format!("TC3-HMAC-SHA256\n{timestamp}\n{scope}\n{}", hash(&request));
    let k = mac(format!("TC3{secret}").as_bytes(), date);
    let k = mac(&k, service);
    let k = mac(&k, "tc3_request");
    format!(
        "TC3-HMAC-SHA256 Credential={id}/{scope}, SignedHeaders=content-type;host, Signature={}",
        hex(&mac(&k, &sign))
    )
}

fn append(rows: &mut Vec<Value>, row: Value, retained: &mut usize) -> Result<()> {
    let bytes = serde_json::to_vec(&row)?.len();
    ensure!(
        rows.len() < MAX_ROWS && retained.saturating_add(bytes) <= MAX_BODY,
        "云查询结果超过 8 MiB，请缩小查询范围"
    );
    *retained += bytes;
    rows.push(row);
    Ok(())
}
