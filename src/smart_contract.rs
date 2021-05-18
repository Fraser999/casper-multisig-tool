use std::{
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
};

use casper_types::account::{AccountHash, Weight};

use super::Error;

#[derive(Clone, PartialOrd, Ord, PartialEq, Eq, Debug)]
enum AssociatedKeyKind {
    Primary { remove_after_creation: bool },
    Secondary,
}

#[derive(Clone, PartialOrd, Ord, PartialEq, Eq, Debug)]
pub(super) struct AssociatedKey {
    account_hash: AccountHash,
    kind: AssociatedKeyKind,
    weight: Weight,
}

impl AssociatedKey {
    fn new_primary(
        formatted_account_hash: &str,
        weight: u8,
        remove_after_creation: bool,
    ) -> Result<Self, Error> {
        let account_hash =
            AccountHash::from_formatted_str(formatted_account_hash).map_err(|error| {
                Error::ParseAccountHash {
                    inner: error.to_string(),
                }
            })?;

        Ok(AssociatedKey {
            account_hash,
            kind: AssociatedKeyKind::Primary {
                remove_after_creation,
            },
            weight: Weight::new(weight),
        })
    }

    fn new_secondary(formatted_account_hash: &str, weight: u8) -> Result<Self, Error> {
        let account_hash =
            AccountHash::from_formatted_str(formatted_account_hash).map_err(|error| {
                Error::ParseAccountHash {
                    inner: error.to_string(),
                }
            })?;

        Ok(AssociatedKey {
            account_hash,
            kind: AssociatedKeyKind::Secondary,
            weight: Weight::new(weight),
        })
    }

    fn remove_after_creation(&self) -> bool {
        match self.kind {
            AssociatedKeyKind::Primary {
                remove_after_creation,
            } => remove_after_creation,
            AssociatedKeyKind::Secondary => false,
        }
    }
}

#[derive(Debug)]
pub(super) struct SmartContract {
    pub(super) root_dir: PathBuf,
    pub(super) contract_name: String,
    pub(super) associated_keys: Vec<AssociatedKey>,
    pub(super) key_management_weight: Weight,
    pub(super) deployment_weight: Weight,
    compile_worker: Option<JoinHandle<()>>,
}

impl Default for SmartContract {
    fn default() -> Self {
        SmartContract {
            root_dir: PathBuf::new(),
            contract_name: String::new(),
            associated_keys: Vec::new(),
            key_management_weight: Weight::new(0),
            deployment_weight: Weight::new(0),
            compile_worker: None,
        }
    }
}

impl SmartContract {
    pub(super) fn set_associated_keys_and_thresholds(
        &mut self,
        mut keys: Vec<(String, u8)>,
        primary_key_should_be_deleted: bool,
        key_management_weight: u8,
        deployment_weight: u8,
    ) -> Result<(), Error> {
        let mut associated_keys = Vec::new();

        let mut keys_iter = keys.drain(..);

        let (formatted_account_hash, weight) = keys_iter.next().ok_or(Error::NoKeys)?;
        let primary_key = AssociatedKey::new_primary(
            &formatted_account_hash,
            weight,
            primary_key_should_be_deleted,
        )?;
        associated_keys.push(primary_key);

        for (formatted_account_hash, weight) in keys_iter {
            let secondary_key = AssociatedKey::new_secondary(&formatted_account_hash, weight)?;
            associated_keys.push(secondary_key);
        }

        self.associated_keys = associated_keys;
        self.key_management_weight = Weight::new(key_management_weight);
        self.deployment_weight = Weight::new(deployment_weight);

        Ok(())
    }

    pub(super) fn create_and_compile(&mut self) -> Result<Receiver<String>, Error> {
        let project_dir = self.project_dir();
        fs::create_dir_all(&project_dir).unwrap();

        self.create_cargo_config()?;
        self.create_main_rs()?;
        self.create_cargo_toml()?;
        self.create_rust_toolchain()?;

        self.compile_contract()
    }

    fn create_cargo_config(&self) -> Result<(), Error> {
        let project_dir = self.project_dir();
        let cargo_config_dir = project_dir.join(".cargo");
        fs::create_dir_all(&cargo_config_dir).unwrap();

        fs::write(
            cargo_config_dir.join("config.toml"),
            br#"[build]
target = "wasm32-unknown-unknown"
"#,
        )
        .unwrap();

        Ok(())
    }

    pub(super) fn main_rs_contents(&self) -> String {
        if self.associated_keys.is_empty()
            || self.key_management_weight.value() == 0
            || self.deployment_weight.value() == 0
        {
            return String::new();
        }

        let mut iter = self.associated_keys.iter().enumerate();
        let (_, primary_key) = iter.next().unwrap();
        let mut contents = format!(
            r#"#![cfg_attr(
    not(target_arch = "wasm32"),
    crate_type = "target arch should be wasm32"
)]
#![no_main]

use casper_contract::{{contract_api::account, unwrap_or_revert::UnwrapOrRevert}};
use casper_types::account::{{AccountHash, ActionType, Weight}};

// {}
#[rustfmt::skip]
const MAIN_ACCOUNT_HASH: AccountHash = AccountHash::new({:?});
const MAIN_ACCOUNT_WEIGHT: u8 = {};

"#,
            primary_key.account_hash.to_formatted_string(),
            primary_key.account_hash.value(),
            primary_key.weight.value(),
        );

        for (index, secondary_key) in iter {
            contents = format!(
                r#"{contents}// {hex_hash}
#[rustfmt::skip]
const ACCOUNT_{index}_HASH: AccountHash = AccountHash::new({hash:?});
const ACCOUNT_{index}_WEIGHT: u8 = {weight};

"#,
                contents = contents,
                hex_hash = secondary_key.account_hash.to_formatted_string(),
                index = index,
                hash = secondary_key.account_hash.value(),
                weight = secondary_key.weight.value(),
            );
        }

        contents = format!(
            r#"{contents}const KEY_MANAGEMENT_WEIGHT: u8 = {km_weight};
const DEPLOYMENT_WEIGHT: u8 = {dp_weight};

#[no_mangle]
pub extern "C" fn call() {{
    // Update the main account key's weight.
    account::update_associated_key(MAIN_ACCOUNT_HASH, Weight::new(MAIN_ACCOUNT_WEIGHT))
        .unwrap_or_revert();

"#,
            contents = contents,
            km_weight = self.key_management_weight.value(),
            dp_weight = self.deployment_weight.value()
        );

        for index in 1..self.associated_keys.len() {
            contents = format!(
                r#"{contents}    // Add associated key {index}.
    account::add_associated_key(ACCOUNT_{index}_HASH, Weight::new(ACCOUNT_{index}_WEIGHT)).unwrap_or_revert();

"#,
                contents = contents,
                index = index
            );
        }

        let remove_main_account = if primary_key.remove_after_creation() {
            r#"
    // Remove the main account's key.
    account::remove_associated_key(MAIN_ACCOUNT_HASH).unwrap_or_revert();
"#
        } else {
            ""
        };

        contents = format!(
            r#"{contents}    // Set the action thresholds.
    account::set_action_threshold(
        ActionType::KeyManagement,
        Weight::new(KEY_MANAGEMENT_WEIGHT),
    )
    .unwrap_or_revert();
    account::set_action_threshold(ActionType::Deployment, Weight::new(DEPLOYMENT_WEIGHT))
        .unwrap_or_revert();
{remove_main_account}}}
"#,
            contents = contents,
            remove_main_account = remove_main_account
        );

        contents
    }

    fn create_main_rs(&self) -> Result<(), Error> {
        let project_dir = self.project_dir();
        let src_dir = project_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let contents = self.main_rs_contents();
        fs::write(src_dir.join("main.rs"), contents.as_bytes()).unwrap();
        Ok(())
    }

    fn create_cargo_toml(&self) -> Result<(), Error> {
        let project_dir = self.project_dir();

        let mut cargo_toml = BufWriter::new(File::create(project_dir.join("Cargo.toml")).unwrap());
        cargo_toml
            .write_all(
                format!(
                    r#"[package]
name = "{0}"
version = "0.1.0"
authors = ["Fraser Hutchison <fraser@casperlabs.io>"]
edition = "2018"

[dependencies]
casper-contract = "1"
casper-types = "1"

[[bin]]
name = "{0}"
path = "src/main.rs"
bench = false
doctest = false
test = false

[features]
default = ["casper-contract/std", "casper-types/std"]

[profile.release]
lto = true
codegen-units = 1
"#,
                    self.contract_name
                )
                .as_bytes(),
            )
            .unwrap();
        Ok(())
    }

    fn create_rust_toolchain(&self) -> Result<(), Error> {
        let project_dir = self.project_dir();

        let mut rust_toolchain =
            BufWriter::new(File::create(project_dir.join("rust-toolchain")).unwrap());
        rust_toolchain
            .write_all(
                br#"nightly-2020-12-16
"#,
            )
            .unwrap();
        Ok(())
    }

    fn compile_contract(&mut self) -> Result<Receiver<String>, Error> {
        let (sender, receiver) = mpsc::channel();
        let project_dir = self.project_dir();
        let contract_name = self.contract_name.clone();

        let compile_worker = thread::spawn(move || {
            let mut command = Command::new("cargo");
            command.args(&["build", "--release"]);
            command.current_dir(&project_dir);

            let _ = sender.send(format!(
                "Running {:?} in {}",
                command,
                project_dir.display()
            ));
            let _ = sender.send(String::new());

            let mut child = command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            let stdout_reader = BufReader::new(stdout);
            let stdout_lines = stdout_reader.lines();

            let stderr = child.stderr.take().unwrap();
            let stderr_reader = BufReader::new(stderr);
            let stderr_lines = stderr_reader.lines();

            let sender_clone = sender.clone();
            let stderr_thread = thread::spawn(move || {
                for line in stderr_lines {
                    let send_res = sender_clone.send(line.unwrap());
                    if let Err(error) = send_res {
                        println!("stopping sending stderr: {}", error);
                        break;
                    };
                }
            });

            for line in stdout_lines {
                if sender.send(line.unwrap()).is_err() {
                    println!("stopping sending stdout");
                    break;
                };
            }

            stderr_thread.join().unwrap();
            child.wait().unwrap();

            let _ = sender.send(String::new());
            let _ = sender.send("Smart contract source code:".to_string());
            let _ = sender.send(
                project_dir
                    .join("src")
                    .join("main.rs")
                    .display()
                    .to_string(),
            );
            let _ = sender.send(String::new());
            let _ = sender.send("Compiled smart contract:".to_string());
            let _ = sender.send(
                project_dir
                    .join("target")
                    .join("wasm32-unknown-unknown")
                    .join("release")
                    .join(format!("{}.wasm", contract_name))
                    .display()
                    .to_string(),
            );
        });

        self.compile_worker = Some(compile_worker);

        Ok(receiver)
    }

    fn project_dir(&self) -> PathBuf {
        self.root_dir.join(&self.contract_name)
    }
}
