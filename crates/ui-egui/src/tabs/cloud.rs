use serde_json::Value;
use tomato_services::{
    backup::Control,
    cloud::{
        Action, Output, Record,
        certificate::{Request, renewal::Deployment},
    },
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Products,
    Instances(String),
    Domains,
    Records(String),
    Properties,
}
pub struct State {
    pub ddns_path: std::path::PathBuf,
    pub page: Page,
    pub rows: Vec<Value>,
    pub selected: Option<usize>,
    pub detail: String,
    pub loaded: bool,
    pub pending: Option<Action>,
    pub followup: Option<Action>,
    pub status: String,
    pub certificate_refresh: bool,
    pub region: String,
    pub record: Option<Record>,
    pub delete: Option<Record>,
    pub certificate: Option<Certificate>,
    pub control: Option<Control>,
}
pub struct Certificate {
    pub request: Request,
    pub auto_renew: bool,
    pub auto_deploy: bool,
    pub storage_copy: bool,
    pub deployment: Deployment,
    pub tos: bool,
    pub selected_storage: String,
    pub buckets: Vec<String>,
    pub buckets_for: String,
    pub bucket_task: Option<u64>,
    pub bucket_error: Option<String>,
    pub issue_error: Option<String>,
}
impl State {
    pub fn new() -> Self {
        Self {
            ddns_path: tomato_services::cloud::ddns::path(),
            page: Page::Products,
            rows: Vec::new(),
            selected: None,
            detail: String::new(),
            loaded: false,
            pending: None,
            followup: None,
            status: String::new(),
            certificate_refresh: false,
            region: String::new(),
            record: None,
            delete: None,
            certificate: None,
            control: None,
        }
    }
    pub fn action(&self) -> Option<Action> {
        match &self.page {
            Page::Products => None,
            Page::Instances(_) => Some(Action::Instances {
                region: self.region.clone(),
            }),
            Page::Domains => Some(Action::Domains),
            Page::Records(domain) => Some(Action::Records {
                domain: domain.clone(),
            }),
            Page::Properties => None,
        }
    }
    pub fn accept(&mut self, output: Output) {
        self.status = output.message;
        let change = matches!(
            self.pending.take(),
            Some(Action::SaveRecord { .. } | Action::DeleteRecord { .. } | Action::SetDdns { .. })
        );
        if change {
            self.record = None;
            self.delete = None;
            self.followup = self.action();
        } else {
            self.rows = output.rows;
            self.selected = None;
            self.loaded = true;
        }
    }
    pub fn certificate(&mut self, domain: &str, zone: &str, email: &str) {
        if self.certificate.is_some() || self.control.is_some() {
            return;
        }
        self.certificate = Some(Certificate {
            request: Request {
                domain: domain.into(),
                dns_zone: zone.into(),
                email: email.into(),
                wildcard: false,
                storage: tomato_core::Connection::new("S3"),
                bucket: String::new(),
                prefix: format!("certificates/{domain}/"),
                server_types: vec![
                    "Nginx".into(),
                    "Apache".into(),
                    "Tomcat (PFX)".into(),
                    "IIS (PFX)".into(),
                    "Java JKS".into(),
                    "通用 PEM".into(),
                ],
                zip_package: true,
                keystore_password: String::new(),
                certificate_authority: "Let's Encrypt（90 天）".into(),
                eab_kid: String::new(),
                eab_hmac: String::new(),
            },
            auto_renew: true,
            auto_deploy: false,
            storage_copy: true,
            deployment: Deployment {
                reload_command: "nginx -t && nginx -s reload".into(),
                ..Default::default()
            },
            tos: true,
            selected_storage: String::new(),
            buckets: Vec::new(),
            buckets_for: String::new(),
            bucket_task: None,
            bucket_error: None,
            issue_error: None,
        });
    }
}
impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(c) = &self.control {
            c.cancel();
        }
    }
}
