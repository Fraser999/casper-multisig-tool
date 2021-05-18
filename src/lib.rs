mod smart_contract;

use std::{
    fmt::{self, Display, Formatter},
    fs,
    path::PathBuf,
    sync::{mpsc::Receiver, Mutex},
};

use once_cell::sync::Lazy;
use thiserror::Error;

use casper_node::crypto::AsymmetricKeyExt;
use casper_types::{account::AccountHash, crypto::AsymmetricType, PublicKey};

use smart_contract::SmartContract;

static SMART_CONTRACT: Lazy<Mutex<SmartContract>> =
    Lazy::new(|| Mutex::new(SmartContract::default()));

#[derive(Error, Debug)]
pub enum Error {
    ParsePublicKeyFile { file: String, inner: Option<String> },
    ParseHexPublicKey { inner: String },
    ParseAccountHash { inner: String },
    NoKeys,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Error::ParsePublicKeyFile { file, inner } => {
                write!(formatter, "failed to parse {} as a public key", file)?;
                match inner {
                    Some(source) => write!(formatter, ": {}", source),
                    None => Ok(()),
                }
            }
            Error::ParseHexPublicKey { inner } => {
                write!(
                    formatter,
                    "failed to parse as a hex-encoded public key: {}",
                    inner
                )
            }
            Error::ParseAccountHash { inner } => {
                write!(
                    formatter,
                    "failed to parse as a formatted account hash: {}",
                    inner
                )
            }
            Error::NoKeys => write!(formatter, "at least one key must be provided"),
        }
    }
}

fn make_parse_file_error<T: ToString>(path: &str, error: T) -> Error {
    Error::ParsePublicKeyFile {
        file: path.to_string(),
        inner: Some(error.to_string()),
    }
}

/// Returns the hex-encoded account hash derived from the public key contained in the provided file.
///
/// The file must be a hex-encoded or PEM-encoded public key as is produced by the casper-client.
pub fn get_account_hash_from_file(path: &str) -> Result<String, Error> {
    match PublicKey::from_file(path) {
        Ok(public_key) => return Ok(public_key.to_account_hash().to_formatted_string()),
        Err(error) => {
            if path.ends_with(".pem") {
                return Err(make_parse_file_error(path, error));
            }
        }
    }

    let contents = fs::read_to_string(path).map_err(|error| make_parse_file_error(path, error))?;

    match PublicKey::from_hex(&contents) {
        Ok(public_key) => return Ok(public_key.to_account_hash().to_formatted_string()),
        Err(error) => {
            if path.ends_with("public_key_hex") {
                return Err(make_parse_file_error(path, error));
            }
        }
    }

    Err(Error::ParsePublicKeyFile {
        file: path.to_string(),
        inner: None,
    })
}

/// Returns the hex-encoded account hash derived from the provided hex-encoded public key.
///
/// The input must be a hex-encoded public key, prefixed with a hex-encoded tag indicating the
/// algorithm as is produced by the casper-client.
pub fn get_account_hash_from_hex_encoded_public_key(hex_public_key: &str) -> Result<String, Error> {
    match PublicKey::from_hex(hex_public_key) {
        Ok(public_key) => Ok(public_key.to_account_hash().to_formatted_string()),
        Err(error) => Err(Error::ParseHexPublicKey {
            inner: error.to_string(),
        }),
    }
}

/// Returns `Ok` if the provided account hash is correctly formatted, else `Err`.
///
/// The input must be a hex-encoded hash, prefixed with `account-hash-` as per the formatted
/// representation of account hashes.
pub fn validate_account_hash(formatted_account_hash: &str) -> Result<(), Error> {
    match AccountHash::from_formatted_str(formatted_account_hash) {
        Ok(_) => Ok(()),
        Err(error) => Err(Error::ParseAccountHash {
            inner: error.to_string(),
        }),
    }
}

/// Sets the values which will be written to the smart contract.
///
/// Can be called multiple times before actually generating the contract.
pub fn set_associated_keys_and_thresholds(
    keys: Vec<(String, u8)>,
    primary_key_should_be_deleted: bool,
    key_management_weight: u8,
    deployment_weight: u8,
) -> Result<(), Error> {
    SMART_CONTRACT
        .lock()
        .unwrap()
        .set_associated_keys_and_thresholds(
            keys,
            primary_key_should_be_deleted,
            key_management_weight,
            deployment_weight,
        )
}

/// Returns the root dir of the project which will hold the smart contract.
pub fn project_path() -> PathBuf {
    SMART_CONTRACT.lock().unwrap().root_dir.clone()
}

/// Returns the smart contract's name.
pub fn contract_name() -> String {
    SMART_CONTRACT.lock().unwrap().contract_name.clone()
}

/// Sets the root dir of the project which will hold the smart contract.
pub fn set_project_path(root_dir: &str) {
    SMART_CONTRACT.lock().unwrap().root_dir = PathBuf::from(root_dir);
}

/// Sets the smart contract's name.
pub fn set_contract_name(name: &str) {
    SMART_CONTRACT.lock().unwrap().contract_name = name.to_string();
}

pub fn main_rs_contents() -> String {
    SMART_CONTRACT.lock().unwrap().main_rs_contents()
}

/// Generates the Rust source for the contract and compiles it to Wasm.
pub fn generate_smart_contract() -> Result<Receiver<String>, Error> {
    SMART_CONTRACT.lock().unwrap().create_and_compile()
}
