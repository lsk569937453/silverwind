use crate::app_error;
use crate::utils::fs_utils::get_domain_path;
use crate::vojo::app_error::AppError;
use crate::vojo::sni_cert_resolver::SniCertResolver;
use notify::RecommendedWatcher;
use notify::RecursiveMode;
use notify::Watcher;
use rcgen::KeyPair;
use rcgen::{CertificateParams, DistinguishedName};
use rustls::crypto::ring::sign::any_supported_type;
use rustls::pki_types::PrivateKeyDer;
use rustls::sign::CertifiedKey;
use rustls::ServerConfig;
use rustls_pki_types::PrivatePkcs8KeyDer;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use tracing::info;

fn create_self_signed_certified_key(domain: &str) -> Result<CertifiedKey, AppError> {
    info!(
        "Generating self-signed certificate for domain '{}'...",
        domain
    );
    let mut params = CertificateParams::new(vec![domain.to_string()])?;
    params.distinguished_name = DistinguishedName::new();
    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;
    let cert_der = cert.der().clone();
    let private_key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
    let signing_key = any_supported_type(&private_key)
        .map_err(|e| app_error!("Private key for '{domain}' not supported by rustls: {e}"))?;
    info!(
        "Successfully generated self-signed certificate for domain '{}'.",
        domain
    );

    Ok(CertifiedKey::new(vec![cert_der], signing_key))
}

/// Loads the PEM certificate chain and key stored for `domain` under
/// `~/.spire/domains/<domain>/`. Fails if the files are missing, cannot be
/// parsed, or the certificate is expired/not yet valid.
pub fn load_domain_certified_key(domain: &str) -> Result<CertifiedKey, AppError> {
    let cert_dir = get_domain_path(domain)?;
    let cert_path = cert_dir.join("cert.pem");
    let key_path = cert_dir.join("key.pem");

    if !cert_path.exists() || !key_path.exists() {
        return Err(app_error!(
            "Certificate for domain '{}' not found at '{}'",
            domain,
            cert_dir.display()
        ));
    }

    let cert_file = File::open(&cert_path).map_err(|e| {
        app_error!(
            "Failed to open cert file '{}': {}",
            cert_path.display(),
            e
        )
    })?;
    let certs = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| app_error!("Failed to parse certificate file for '{domain}': {e}"))?;

    if certs.is_empty() {
        return Err(app_error!(
            "No certificates found in '{}'",
            cert_path.display()
        ));
    }

    let first_cert = x509_parser::parse_x509_certificate(&certs[0])
        .map_err(|e| app_error!("Failed to parse certificate for '{domain}': {e:?}"))?
        .1;
    if !first_cert.validity().is_valid() {
        return Err(app_error!(
            "Certificate for domain '{domain}' has expired or is not yet valid"
        ));
    }
    info!("Certificate for '{}' is valid.", domain);

    let key_file = File::open(&key_path)
        .map_err(|e| app_error!("Failed to open key file '{}': {}", key_path.display(), e))?;
    let private_key = rustls_pemfile::private_key(&mut BufReader::new(key_file))?
        .ok_or_else(|| app_error!("Failed to parse private key for '{domain}'"))?;

    let signing_key = any_supported_type(&private_key)
        .map_err(|e| app_error!("Private key for '{domain}' not supported by rustls: {e}"))?;

    Ok(CertifiedKey::new(certs, signing_key))
}

/// Builds a TLS config that serves a certificate per domain, selected by the
/// SNI sent by the client. The first domain acts as the default for clients
/// that send no SNI or an unknown name. Domains without a usable stored
/// certificate fall back to a self-signed one.
pub fn load_tls_config_multi(domains: &[String]) -> Result<ServerConfig, AppError> {
    if domains.is_empty() {
        return Err(app_error!(
            "Cannot create TLS configuration because the domains list is empty."
        ));
    }

    let mut resolver = SniCertResolver::new();
    for (index, domain) in domains.iter().enumerate() {
        let certified_key = match load_domain_certified_key(domain) {
            Ok(certified_key) => certified_key,
            Err(e) => {
                warn!("Failed to load certificate for '{domain}': {e}. Falling back to a self-signed certificate.");
                create_self_signed_certified_key(domain)?
            }
        };
        resolver.add_certified_key(domain, certified_key, index == 0);
    }

    let config = ServerConfig::builder_with_protocol_versions(&[
        &rustls::version::TLS13,
        &rustls::version::TLS12,
    ])
    .with_no_client_auth()
    .with_cert_resolver(Arc::new(resolver));

    Ok(config)
}

pub async fn watch_for_certificate_changes(
    domains: Vec<String>,
    tls_config: Arc<RwLock<rustls::ServerConfig>>,
) -> Result<(), AppError> {
    let mut watched_files = HashSet::new();
    let mut cert_dirs = Vec::new();
    for domain in &domains {
        let cert_dir = get_domain_path(domain)?;
        if let Err(e) = tokio::fs::create_dir_all(&cert_dir).await {
            error!(
                "Failed to create certificate directory '{}': {}",
                cert_dir.display(),
                e
            );
            continue;
        }
        watched_files.insert(cert_dir.join("cert.pem"));
        watched_files.insert(cert_dir.join("key.pem"));
        cert_dirs.push(cert_dir);
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel(1);

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if matches!(
                    event.kind,
                    notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                ) && event
                    .paths
                    .iter()
                    .any(|p| watched_files.contains(p))
                {
                    info!("Certificate or key file change detected: {:?}", event.kind);
                    let _ = tx.blocking_send(());
                }
            }
        },
        notify::Config::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            error!("Failed to create file watcher: {e}");
            return Ok(());
        }
    };

    for cert_dir in &cert_dirs {
        if let Err(e) = watcher.watch(cert_dir, RecursiveMode::NonRecursive) {
            error!(
                "Failed to watch certificate directory at '{}': {}",
                cert_dir.display(),
                e
            );
        }
    }

    info!(
        "Started watching for certificate changes in directories: {:?}",
        cert_dirs
    );

    while rx.recv().await.is_some() {
        tokio::time::sleep(Duration::from_secs(1)).await;

        info!("Detected change in certificate/key files. Attempting to reload.");
        match load_tls_config_multi(&domains) {
            Ok(new_config) => {
                let mut config_writer = tls_config.write().map_err(|e| AppError(e.to_string()))?;
                *config_writer = new_config;
                info!("Successfully reloaded TLS certificates.");
            }
            Err(e) => {
                error!("Failed to reload TLS certificates: {e}. Keeping the old one.");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_domain_config_falls_back_to_self_signed() {
        let domains = vec![
            "spire-test-does-not-exist-a.com".to_string(),
            "spire-test-does-not-exist-b.com".to_string(),
        ];

        // Building the config must succeed even when none of the domains has a
        // stored certificate: rustls then installs the self-signed fallback
        // resolver. In rustls 0.23 the resolver is a non-optional field
        // (`Arc<dyn ResolvesServerCert>`), so a successful build is the
        // assertion here.
        load_tls_config_multi(&domains).unwrap();
    }

    #[test]
    fn multi_domain_config_rejects_empty_domain_list() {
        assert!(load_tls_config_multi(&[]).is_err());
    }

    #[test]
    fn domain_certified_key_fails_for_missing_files() {
        assert!(load_domain_certified_key("spire-test-does-not-exist-c.com").is_err());
    }
}
