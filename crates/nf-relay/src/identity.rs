use std::path::Path;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::RelayError;

const CERTIFICATE_FILE: &str = "relay-certificate.der";
const KEY_FILE: &str = "relay-key.der";

pub struct Identity {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
}

pub fn fingerprint_of(certificate: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, certificate)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl Identity {
    pub fn generate(names: Vec<String>) -> Result<Self, RelayError> {
        let generated = rcgen::generate_simple_self_signed(names).map_err(|error| RelayError::Identity(error.to_string()))?;
        Ok(Self {
            chain: vec![generated.cert.der().clone()],
            key: PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der()).into(),
        })
    }

    pub fn load_or_generate(directory: &Path, names: Vec<String>) -> Result<Self, RelayError> {
        let certificate_path = directory.join(CERTIFICATE_FILE);
        let key_path = directory.join(KEY_FILE);
        if let (Ok(certificate), Ok(key)) = (std::fs::read(&certificate_path), std::fs::read(&key_path)) {
            return Ok(Self {
                chain: vec![CertificateDer::from(certificate)],
                key: PrivatePkcs8KeyDer::from(key).into(),
            });
        }
        let identity = Self::generate(names)?;
        std::fs::create_dir_all(directory).map_err(|error| RelayError::Identity(error.to_string()))?;
        std::fs::write(&certificate_path, &identity.chain[0]).map_err(|error| RelayError::Identity(error.to_string()))?;
        std::fs::write(&key_path, identity.key.secret_der()).map_err(|error| RelayError::Identity(error.to_string()))?;
        Ok(identity)
    }

    pub fn from_pem_files(certificate: &Path, key: &Path) -> Result<Self, RelayError> {
        let chain = CertificateDer::pem_file_iter(certificate)
            .and_then(|certificates| certificates.collect::<Result<Vec<_>, _>>())
            .map_err(|error| RelayError::Identity(format!("{}: {error}", certificate.display())))?;
        let key = PrivateKeyDer::from_pem_file(key).map_err(|error| RelayError::Identity(format!("{}: {error}", key.display())))?;
        if chain.is_empty() {
            return Err(RelayError::Identity(format!("{} contains no certificate", certificate.display())));
        }
        Ok(Self { chain, key })
    }

    pub fn fingerprint(&self) -> String {
        fingerprint_of(&self.chain[0])
    }
}
