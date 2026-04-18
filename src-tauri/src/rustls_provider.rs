use std::sync::Once;

/// Install a process-wide rustls crypto provider before any client/server config is built.
///
/// Our dependency graph enables both `aws-lc-rs` and `ring`, so rustls can no longer
/// infer a default provider on its own. Installing one explicitly keeps reqwest,
/// hyper-rustls, and tokio-rustls from panicking at runtime.
pub(crate) fn ensure_rustls_crypto_provider() {
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        if rustls::crypto::CryptoProvider::get_default().is_some() {
            return;
        }

        match rustls::crypto::aws_lc_rs::default_provider().install_default() {
            Ok(()) => {
                log::info!("Installed rustls aws-lc-rs crypto provider");
            }
            Err(_) if rustls::crypto::CryptoProvider::get_default().is_some() => {
                log::debug!("rustls crypto provider was installed by another caller");
            }
            Err(_) => {
                log::warn!(
                    "Failed to install rustls aws-lc-rs crypto provider and no default is set"
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::ensure_rustls_crypto_provider;

    #[test]
    fn ensure_rustls_crypto_provider_is_idempotent() {
        ensure_rustls_crypto_provider();
        ensure_rustls_crypto_provider();

        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
