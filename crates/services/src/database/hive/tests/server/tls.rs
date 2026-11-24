use super::*;
pub(super) fn client(
    root: &std::path::Path,
    mode: &str,
) -> Result<Arc<dyn rustls::server::danger::ClientCertVerifier>> {
    use rcgen::{CertificateParams, KeyPair};
    let mut ca_params = CertificateParams::new(Vec::<String>::new())?;
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];
    let ca_key = KeyPair::generate()?;
    let ca = ca_params.self_signed(&ca_key)?;
    let mut params = CertificateParams::new(vec!["fixture-client".into()])?;
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    let key = KeyPair::generate()?;
    let certificate = params.signed_by(&key, &rcgen::Issuer::from_params(&ca_params, &ca_key))?;
    let path = root.join("client-keystore");
    match mode {
        "mtls-jks" => {
            let mut store = jks::KeyStore::new();
            store.set_private_key_entry(
                "fixture-client",
                jks::PrivateKeyEntry {
                    creation_time: std::time::SystemTime::now(),
                    private_key: key.serialize_der(),
                    certificate_chain: [certificate.der(), ca.der()]
                        .into_iter()
                        .map(|certificate| jks::Certificate {
                            cert_type: "X.509".into(),
                            content: certificate.to_vec(),
                        })
                        .collect(),
                },
                b"fixture-pass",
            )?;
            store.store(std::fs::File::create(path)?, b"fixture-pass")?;
        }
        "mtls-p12" => {
            let mut store = p12_keystore::KeyStore::new();
            let chain = p12_keystore::PrivateKeyChain::new(
                &b"fixture-client"[..],
                p12_keystore::PrivateKey::from_der(&key.serialize_der())?,
                [certificate.der(), ca.der()]
                    .into_iter()
                    .map(|cert| p12_keystore::Certificate::from_der(cert))
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            );
            store.add_entry(
                "fixture-client",
                p12_keystore::KeyStoreEntry::PrivateKeyChain(chain),
            );
            std::fs::write(path, store.writer("fixture-pass").write()?)?;
        }
        _ => std::fs::write(
            path,
            format!("{}{}{}", certificate.pem(), ca.pem(), key.serialize_pem()),
        )?,
    }
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca.der().clone())?;
    Ok(rustls::server::WebPkiClientVerifier::builder(Arc::new(roots)).build()?)
}
