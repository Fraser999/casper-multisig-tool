use std::{
    cmp, collections::HashMap, env, panic, path::PathBuf, thread, thread::JoinHandle,
    time::Duration,
};

use fltk::{
    app::{self, App, Scheme},
    button::{Button, CheckButton},
    dialog::{self, FileDialog, FileDialogOptions, FileDialogType},
    enums::{Align, Color, Font, FrameType},
    frame::Frame,
    group::{Pack, PackType},
    image::PngImage,
    output::Output,
    prelude::{DisplayExt, GroupExt, InputExt, ValuatorExt, WidgetBase, WidgetExt, WindowExt},
    text::{TextBuffer, TextDisplay},
    valuator::ValueInput,
    window::Window,
};

use casper_types::account::MAX_ASSOCIATED_KEYS;

// TODO:
//  * key-management threshold max set to total weights of keys, excluding primary if it's to be
//    deleted.  Need to also handle this in should_be_deleted checkbox callback.
//  * validate values in lib::set_associated_keys_and_thresholds
//  * provide lib API for validating and adjusting all input values (keys, weights, thresholds)
//  * best way to stream stdout/stderr back from lib when running child process?  Provide callback
//    which appends the data to a buffer?
//  * use logging rather than println
//  * remove unwraps
//  * handle author in Cargo.toml of generated contracts
//  * handle invalid contract names in `create()`
//  * create test project to test the contract and execute it
//  * run wasm-strip if available
//  * readme with install instructions for dependencies
//  * tooltips
//  * help/instructions on main page - mention tooltips on account hash boxes
//  * add button to run test

fn set_panic_handler() {
    panic::set_hook(Box::new(move |panic_info| {
        let message = if let Some(info) = panic_info.payload().downcast_ref::<&str>() {
            info.to_string()
        } else {
            panic_info.to_string()
        };
        dialog::alert_default(&format!(
            "Fatal error: {}\n\nTerminating program.",
            &message
        ));
        app::program_should_quit(true);
    }));
}

const TOOL_NAME: &str = "Casper Multisig Account Creation Tool";
const WINDOW_WIDTH: i32 = 1800;
const BUTTON_WIDTH: i32 = 300;
const BUTTON_HEIGHT: i32 = 40;
const OUTPUT_ROW_HEIGHT: i32 = 40;
const PADDING: i32 = 10;
const BUTTON_COLOR: u32 = 0xd1d0ce;

type AccountHashWidget = Output;
type WeightWidget = ValueInput;
type DeleteButton = Button;
type MainKeyShouldBeDeletedWidget = CheckButton;

/// The indices of each widget in the `AssociatedKeyPack` widget.
#[repr(i32)]
enum AssociatedKeyPackIndices {
    AccountHash,
    Weight,
    Delete,
    MainKeyShouldBeDeleted,
}

/// The indices of each widget in the `ActionThresholdsPack` widget.
#[repr(i32)]
enum ActionThresholdsPackIndices {
    KeyManagementWeight,
    DeploymentWeight,
}

/// The indices of each widget in the main window widget.
#[repr(i32)]
enum WindowIndices {
    TopFrame,
    AddKeyButtonPack,
    MiddleFrame,
    MainKeyFrame,
    MainOutputPack,
    BottomFrame,
    ActionThresholdsPack,
    RustOutput,
    GenerateButton,
}

/// A wrapper for the horizontal `Pack` widget holding an individual associated key's widgets.
#[derive(Clone)]
struct AssociatedKeyPack {
    pack: Pack,
}

impl AssociatedKeyPack {
    fn new(account_hash_value: &str, tooltip: &str, parent: MainOutputPack) -> Self {
        let mut account_hash = AccountHashWidget::new(0, 0, 800, 0, None);
        account_hash.set_value(account_hash_value);
        account_hash.set_tooltip(tooltip);
        account_hash.set_text_font(Font::Courier);
        account_hash.set_text_size(16);
        account_hash.show();

        let mut weight = WeightWidget::new(100, 0, 50, 0, "");
        weight.set_value(1.0);
        weight.set_align(Align::Top);
        weight.set_tooltip("The weight of the given key");
        weight.set_text_font(Font::Courier);
        weight.set_text_size(16);
        weight.set_minimum(0.0);
        weight.set_maximum(255.0);
        weight.set_soft(false);
        weight.set_step(1.0, 1);
        weight.show();
        weight.set_callback(move |weight| {
            if weight.value() > weight.maximum() {
                weight.set_value(weight.maximum());
            }
            parent.redraw_window();
        });

        // let mut weight = Counter::new(100, 0, 100, 0, "Weight");
        // weight.set_value(1.0);
        // weight.set_tooltip("The weight of the given key");
        // weight.set_minimum(0.0);
        // weight.set_maximum(255.0);
        // weight.set_type(CounterType::Simple);
        // weight.set_step(1.0, 1);
        // weight.set_align(Align::Left);
        // weight.show();

        // The callback for the delete button will be set in the MainOutputPack, since it needs to
        // remove itself from that parent pack.
        let mut delete_button = DeleteButton::new(0, 0, 100, 40, "Delete");
        delete_button.set_color(Color::from_u32(BUTTON_COLOR));

        let mut pack = Pack::new(PADDING, PADDING, 1400, OUTPUT_ROW_HEIGHT, None);
        pack.end();
        pack.set_spacing(30);
        pack.set_type(PackType::Horizontal);
        pack.insert(&account_hash, AssociatedKeyPackIndices::AccountHash as i32);
        pack.insert(&weight, AssociatedKeyPackIndices::Weight as i32);
        pack.insert(&delete_button, AssociatedKeyPackIndices::Delete as i32);

        AssociatedKeyPack { pack }
    }

    /// Returns the account hash widget.
    fn account_hash(&self) -> AccountHashWidget {
        let account_hash = self
            .pack
            .child(AssociatedKeyPackIndices::AccountHash as i32)
            .unwrap();
        unsafe { AccountHashWidget::from_widget_ptr(account_hash.as_widget_ptr() as *mut _) }
    }

    /// Returns the weight widget.
    fn weight(&self) -> WeightWidget {
        let weight = self
            .pack
            .child(AssociatedKeyPackIndices::Weight as i32)
            .unwrap();
        unsafe { WeightWidget::from_widget_ptr(weight.as_widget_ptr() as *mut _) }
    }

    /// Returns the delete button widget.
    fn delete_button(&self) -> DeleteButton {
        let delete_button = self
            .pack
            .child(AssociatedKeyPackIndices::Delete as i32)
            .unwrap();
        unsafe { DeleteButton::from_widget_ptr(delete_button.as_widget_ptr() as *mut _) }
    }

    /// Returns the "main key should be deleted" widget.
    fn main_key_should_be_deleted(&self) -> Option<MainKeyShouldBeDeletedWidget> {
        let should_be_deleted = self
            .pack
            .child(AssociatedKeyPackIndices::MainKeyShouldBeDeleted as i32)?;
        Some(unsafe {
            MainKeyShouldBeDeletedWidget::from_widget_ptr(
                should_be_deleted.as_widget_ptr() as *mut _
            )
        })
    }
}

/// A wrapper for the horizontal `Pack` widget holding the action thresholds.
#[derive(Clone)]
struct ActionThresholdsPack {
    pack: Pack,
}

impl ActionThresholdsPack {
    fn new(parent: MainOutputPack) -> Self {
        let mut key_management_weight = WeightWidget::new(0, 0, 50, 40, "Key-management threshold");
        key_management_weight.set_value(1.0);
        key_management_weight.set_align(Align::Left);
        key_management_weight.set_tooltip(
            "The minimum total weight of signatories required to modify the associated keys.\n\n\
        Cannot exceed the total weights of all keys, excluding the main key if it is set to be \
        deleted after account creation",
        );
        key_management_weight.set_text_font(Font::Courier);
        key_management_weight.set_text_size(16);
        key_management_weight.set_minimum(1.0);
        key_management_weight.set_maximum(255.0);
        key_management_weight.set_bounds(1.0, 255.0);
        key_management_weight.set_soft(false);
        key_management_weight.set_step(1.0, 1);
        key_management_weight.show();

        let mut deployment_weight = WeightWidget::new(300, 0, 50, 40, "Deploy-execution threshold");
        deployment_weight.set_value(1.0);
        deployment_weight.set_align(Align::Left);
        deployment_weight.set_tooltip(
            "The minimum total weight of signatories required to execute a deploy.\n\nCannot exceed \
        the key-execution threshold",
        );
        deployment_weight.set_text_font(Font::Courier);
        deployment_weight.set_text_size(16);
        deployment_weight.set_bounds(1.0, 255.0);
        deployment_weight.set_minimum(1.0);
        deployment_weight.set_maximum(1.0);
        deployment_weight.set_soft(false);
        deployment_weight.set_step(1.0, 1);
        deployment_weight.show();

        let parent_clone = parent.clone();
        deployment_weight.set_callback(move |weight| {
            if weight.value() > weight.maximum() {
                weight.set_value(weight.maximum());
            }
            parent_clone.redraw_window();
        });

        let mut deployment_weight_clone = deployment_weight.clone();
        key_management_weight.set_callback(move |weight| {
            if weight.value() > weight.maximum() {
                weight.set_value(weight.maximum());
            }
            if weight.value() > weight.maximum() {
                weight.set_value(weight.maximum());
            }
            deployment_weight_clone.set_maximum(weight.value());
            if weight.value() < deployment_weight_clone.value() {
                deployment_weight_clone.set_value(weight.value());
            }
            parent.redraw_window();
        });

        let mut pack = Pack::new(250, PADDING + 20, 650, BUTTON_HEIGHT, None);
        pack.set_spacing(300);
        pack.set_type(PackType::Horizontal);
        pack.end();

        pack.insert(
            &key_management_weight,
            ActionThresholdsPackIndices::KeyManagementWeight as i32,
        );
        pack.insert(
            &deployment_weight,
            ActionThresholdsPackIndices::DeploymentWeight as i32,
        );

        ActionThresholdsPack { pack }
    }

    /// Returns the key management weight widget.
    fn key_management_weight(&self) -> WeightWidget {
        let weight = self
            .pack
            .child(ActionThresholdsPackIndices::KeyManagementWeight as i32)
            .unwrap();
        unsafe { WeightWidget::from_widget_ptr(weight.as_widget_ptr() as *mut _) }
    }

    /// Returns the deployment weight widget.
    fn deployment_weight(&self) -> WeightWidget {
        let weight = self
            .pack
            .child(ActionThresholdsPackIndices::DeploymentWeight as i32)
            .unwrap();
        unsafe { WeightWidget::from_widget_ptr(weight.as_widget_ptr() as *mut _) }
    }
}

/// A wrapper for the vertical `Pack` widget holding all the individual associated key `Pack`s.
#[derive(Clone)]
struct MainOutputPack {
    pack: Pack,
    add_public_key_from_file_button: Button,
    add_public_key_from_hex_button: Button,
    add_account_hash_button: Button,
    rust_output_buffer: TextBuffer,
}

impl MainOutputPack {
    fn new(
        add_public_key_from_file_button: Button,
        add_public_key_from_hex_button: Button,
        add_account_hash_button: Button,
        rust_output_buffer: TextBuffer,
    ) -> Self {
        let mut pack = Pack::new(20, 180, 1460, 0, None);
        pack.set_spacing(10);
        pack.end();
        MainOutputPack {
            pack,
            add_public_key_from_file_button,
            add_public_key_from_hex_button,
            add_account_hash_button,
            rust_output_buffer,
        }
    }

    /// Returns the main window widget.
    fn window(&self) -> Box<dyn WindowExt> {
        self.pack.window().unwrap()
    }

    /// Returns the middle frame (surrounding the main output pack) widget.
    fn middle_frame(&self) -> Box<dyn WidgetExt> {
        self.window()
            .child(WindowIndices::MiddleFrame as i32)
            .unwrap()
    }

    /// Returns the main key frame (highlighting the main key) widget.
    fn main_key_frame(&self) -> Box<dyn WidgetExt> {
        self.window()
            .child(WindowIndices::MainKeyFrame as i32)
            .unwrap()
    }

    /// Returns the bottom frame (surrounding the action thresholds) widget.
    fn bottom_frame(&self) -> Box<dyn WidgetExt> {
        self.window()
            .child(WindowIndices::BottomFrame as i32)
            .unwrap()
    }

    /// Returns the Rust output TextDisplay widget.
    fn rust_output_text_display(&self) -> Box<dyn WidgetExt> {
        self.window()
            .child(WindowIndices::RustOutput as i32)
            .unwrap()
    }

    /// Returns the action thresholds pack widget.
    fn action_thresholds_pack(&self) -> ActionThresholdsPack {
        let action_thresholds_pack = self
            .window()
            .child(WindowIndices::ActionThresholdsPack as i32)
            .unwrap();
        let pack =
            unsafe { Pack::from_widget_ptr(action_thresholds_pack.as_widget_ptr() as *mut _) };
        ActionThresholdsPack { pack }
    }

    /// Returns the "Generate smart contract" button widget.
    fn generate_smart_contract_button(&self) -> Box<dyn WidgetExt> {
        self.window()
            .child(WindowIndices::GenerateButton as i32)
            .unwrap()
    }

    /// Returns the main key pack (the first child of `self`) widget.
    fn main_key_pack(&self) -> Option<AssociatedKeyPack> {
        let main_key_pack = self.pack.child(0).and_then(|child| child.as_group())?;
        let pack = unsafe { Pack::from_widget_ptr(main_key_pack.as_widget_ptr() as *mut _) };
        Some(AssociatedKeyPack { pack })
    }

    /// Adds a new associated key `Pack`.
    fn add_associated_key(&self, account_hash_value: &str, tooltip: &str) {
        // TODO - use lib function to do excessive key count/duplicate key check
        let associated_keys = self
            .associated_keys()
            .into_iter()
            .collect::<HashMap<_, _>>();

        if associated_keys.len() >= MAX_ASSOCIATED_KEYS {
            dialog::alert_default("Already have maximum number of associated keys");
            return;
        }

        if associated_keys.contains_key(account_hash_value) {
            dialog::alert_default(&format!(
                "{} is already added to associated keys",
                account_hash_value
            ));
            return;
        }

        let associated_key_pack = AssociatedKeyPack::new(account_hash_value, tooltip, self.clone());

        self.pack.clone().add(&associated_key_pack.pack);
        if associated_keys.is_empty() {
            self.style_main_key();
        }
        self.redraw_window();

        let self_clone = self.clone();
        associated_key_pack.delete_button().set_callback(move |_| {
            self_clone.remove_associated_key(&associated_key_pack.pack);
        });
    }

    /// If the main associated key has changed, this should be called to apply the styling and
    /// additional checkbox widget to the new main key `Pack`.
    fn style_main_key(&self) {
        let mut main_key_pack = self.main_key_pack().unwrap();

        main_key_pack.account_hash().set_tooltip(&format!(
            "This is the main associated key, used to create the account.\n\n{}",
            main_key_pack.account_hash().tooltip().unwrap()
        ));

        let mut main_key_pack_weight = main_key_pack.weight();
        main_key_pack_weight.set_label("Weight\n ");

        let mut should_be_deleted =
            MainKeyShouldBeDeletedWidget::new(0, 0, 40, 40, "Should delete\nafter creation\n ");
        should_be_deleted.set_align(Align::TopLeft);
        let self_clone = self.clone();
        should_be_deleted.set_callback(move |widget| {
            if widget.is_checked() {
                main_key_pack_weight.set_value(255.0);
                main_key_pack_weight.deactivate();
            } else {
                main_key_pack_weight.activate();
            }
            self_clone.redraw_window();
        });

        main_key_pack.pack.insert(
            &should_be_deleted,
            AssociatedKeyPackIndices::MainKeyShouldBeDeleted as i32,
        );

        self.main_key_frame().show();
        self.generate_smart_contract_button().activate();
    }

    /// Removes an associated key `Pack`.
    fn remove_associated_key(&self, associated_key_pack: &Pack) {
        let removed_index = self.pack.clone().find(associated_key_pack);
        self.pack.clone().remove(associated_key_pack);

        if self.pack.children() == 0 {
            self.main_key_frame().hide();
            self.generate_smart_contract_button().deactivate();
        } else if removed_index == 0 {
            self.style_main_key();
        }

        self.redraw_window();
    }

    /// Redraws the main window.
    fn redraw_window(&self) {
        self.update_smart_contract();

        let mut window = self.window();
        let associated_keys_count = self.pack.children();

        if associated_keys_count as usize >= MAX_ASSOCIATED_KEYS {
            self.add_public_key_from_file_button.clone().deactivate();
            self.add_public_key_from_hex_button.clone().deactivate();
            self.add_account_hash_button.clone().deactivate();
        } else {
            self.add_public_key_from_file_button.clone().activate();
            self.add_public_key_from_hex_button.clone().activate();
            self.add_account_hash_button.clone().activate();
        }

        let mut middle_frame = self.middle_frame();
        let middle_frame_height = ((OUTPUT_ROW_HEIGHT + PADDING) * associated_keys_count) + 60;
        middle_frame.set_size(middle_frame.width(), middle_frame_height);

        self.bottom_frame()
            .set_pos(self.bottom_frame().x(), middle_frame_height + 150);
        self.action_thresholds_pack().pack.set_pos(
            self.action_thresholds_pack().pack.x(),
            middle_frame_height + 190,
        );

        self.rust_output_text_display().set_pos(
            PADDING,
            self.bottom_frame().y() + self.bottom_frame().height() + PADDING,
        );

        self.generate_smart_contract_button().set_pos(
            self.generate_smart_contract_button().x(),
            middle_frame_height + 200,
        );

        let (_screen_width, screen_height) = app::screen_size();
        let rust_output_text_display_height =
            cmp::min(screen_height as i32 - middle_frame_height - 300, 800);
        self.rust_output_text_display().set_size(
            self.rust_output_text_display().width(),
            rust_output_text_display_height,
        );

        window.set_size(
            window.width(),
            middle_frame_height + rust_output_text_display_height + 260,
        );
        window.redraw();
    }

    /// Returns the associated keys as a map of formatted account hashes to weights.
    fn associated_keys(&self) -> Vec<(String, u8)> {
        let mut associated_keys = Vec::new();
        for child_pack in self.pack.clone().into_iter() {
            let child_pack = child_pack.as_group().unwrap();
            let associated_key_pack = AssociatedKeyPack {
                pack: unsafe { Pack::from_widget_ptr(child_pack.as_widget_ptr() as *mut _) },
            };

            let account_hash = associated_key_pack.account_hash().value();
            let weight = associated_key_pack.weight().value() as u8;
            associated_keys.push((account_hash, weight));
        }

        associated_keys
    }

    fn main_key_should_be_deleted(&self) -> bool {
        self.main_key_pack()
            .map(|pack| pack.main_key_should_be_deleted().unwrap().is_checked())
            .unwrap_or_default()
    }

    fn update_smart_contract(&self) {
        let associated_keys = self.associated_keys();
        let main_key_should_be_deleted = self.main_key_should_be_deleted();
        let key_management_weight = self
            .action_thresholds_pack()
            .key_management_weight()
            .value() as u8;
        let deployment_weight = self.action_thresholds_pack().deployment_weight().value() as u8;

        let main_rs_contents = if associated_keys.is_empty() {
            String::new()
        } else {
            if let Err(error) = casper_multisig_tool::set_associated_keys_and_thresholds(
                associated_keys,
                main_key_should_be_deleted,
                key_management_weight,
                deployment_weight,
            ) {
                dialog::alert_default(&format!("Error setting associated keys: {}", error));
            }

            casper_multisig_tool::main_rs_contents()
        };

        self.rust_output_buffer.clone().set_text(&main_rs_contents);
    }

    fn generate_smart_contract(&self) -> Option<JoinHandle<()>> {
        let mut file_dialog = FileDialog::new(FileDialogType::BrowseDir);
        if let Some(start_dir) = get_current_or_default_project_path() {
            let _ =
                file_dialog.set_directory(start_dir.join(get_current_or_default_contract_name()));
        }
        file_dialog.set_option(FileDialogOptions::SaveAsConfirm);
        file_dialog.set_option(FileDialogOptions::NewFolder);
        file_dialog.set_title(
            "Choose a folder to save the smart contract.  The folder's name will be used as \
                the name of the contract.",
        );
        file_dialog.show();

        if file_dialog.filename() == PathBuf::default() {
            return None;
        }

        let project_path = file_dialog
            .filename()
            .parent()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        let contract_name = file_dialog
            .filename()
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(get_current_or_default_contract_name);
        casper_multisig_tool::set_project_path(&project_path);
        casper_multisig_tool::set_contract_name(&contract_name);

        let mut new_window = Window::default()
            .with_size(1000, 400)
            .with_label("Generating smart contract");
        new_window.make_modal(true);

        let mut text_display = TextDisplay::default().with_size(
            new_window.width(),
            new_window.height() - BUTTON_HEIGHT - (2 * PADDING),
        );
        let buffer = TextBuffer::default();
        text_display.set_buffer(Some(buffer));
        text_display.set_text_font(Font::Courier);
        text_display.set_text_size(14);

        let button_width = 100;
        let mut done_button = Button::new(
            new_window.width() - PADDING - button_width,
            new_window.height() - PADDING - BUTTON_HEIGHT,
            button_width,
            BUTTON_HEIGHT,
            "Done",
        );
        done_button.set_color(Color::from_u32(BUTTON_COLOR));
        done_button.deactivate();

        new_window.end();
        new_window.show();

        done_button.set_callback(move |button| {
            let mut window = button.window().unwrap();
            window.hide();
        });

        let receiver = casper_multisig_tool::generate_smart_contract().unwrap();

        Some(thread::spawn(move || {
            loop {
                match receiver.recv() {
                    Ok(line) => {
                        let mut buffer = text_display.buffer().unwrap();
                        buffer.append(&format!("{}\n", line));
                        text_display.set_insert_position(buffer.length());
                        text_display.scroll(text_display.count_lines(0, buffer.length(), true), 0);
                    }
                    Err(error) => {
                        println!("Stopping RECV: {}", error);
                        break;
                    }
                }
            }
            done_button.activate();
        }))
    }
}

/// Returns the account hash as a formatted string and a tooltip indicating the origin of the
/// account hash, or `None` if the user didn't enter a valid path or cancelled the operation.
fn get_account_hash_from_public_key_file() -> Option<(String, String)> {
    let mut file_dialog = FileDialog::new(FileDialogType::BrowseFile);
    if let Some(start_dir) = dirs::home_dir().or_else(|| env::current_dir().ok()) {
        let _ = file_dialog.set_directory(start_dir);
    }
    file_dialog.set_option(FileDialogOptions::UseFilterExt);
    file_dialog.set_title("Choose Public Key File");
    file_dialog.set_filter(
        "PEM-encoded Public Key Files \t*public_key*.pem\nHex-encoded Public Key Files \
        \t*public_key*_hex*",
    );
    file_dialog.show();

    if file_dialog.filename() == PathBuf::default() {
        return None;
    }

    let file_path = file_dialog.filename().to_string_lossy().to_string();

    match casper_multisig_tool::get_account_hash_from_file(&file_path) {
        Ok(account_hash) => {
            let tooltip = format!("Derived from contents of {}", file_path);
            Some((account_hash, tooltip))
        }
        Err(error) => {
            dialog::alert_default(error.to_string().as_str());
            None
        }
    }
}

/// Returns the account hash as a formatted string and a tooltip indicating the origin of the
/// account hash, or `None` if the user didn't enter a valid public key or cancelled the operation.
fn get_account_hash_from_hex_public_key() -> Option<(String, String)> {
    // let mut window = Window::default().with_size(1000, 60).center_screen();
    // window.make_modal(true);
    //
    // let mut input = Input::new(10, 10, 800, 40, "Enter formatted public key");
    // input.set_trigger(CallbackTrigger::EnterKey);
    //
    // window.end();
    // window.show();
    //
    // let (sender, receiver) = app::channel::<()>();
    //
    // input.emit(sender, ());
    //
    // let _ = receiver.recv();
    // let hex_public_key = input.value();

    let hex_public_key = dialog::input_default("Enter formatted public key", "")?;

    match casper_multisig_tool::get_account_hash_from_hex_encoded_public_key(&hex_public_key) {
        Ok(account_hash) => {
            let tooltip = format!("Derived from public key {}", hex_public_key);
            Some((account_hash, tooltip))
        }
        Err(error) => {
            dialog::alert_default(error.to_string().as_str());
            None
        }
    }
}

/// Returns the account hash as a formatted string and a tooltip indicating the origin of the
/// account hash, or `None` if the user didn't enter a valid account hash or cancelled the
/// operation.
fn get_account_hash_from_formatted_account_hash() -> Option<(String, String)> {
    let hex_account_hash = dialog::input_default("Enter formatted account hash", "")?;

    match casper_multisig_tool::validate_account_hash(&hex_account_hash) {
        Ok(_) => {
            let tooltip = format!("Derived from account hash {}", hex_account_hash);
            Some((hex_account_hash, tooltip))
        }
        Err(error) => {
            dialog::alert_default(error.to_string().as_str());
            None
        }
    }
}

fn get_current_or_default_project_path() -> Option<PathBuf> {
    let current_project_path = casper_multisig_tool::project_path();
    if current_project_path != PathBuf::default() {
        return Some(current_project_path);
    }
    dirs::home_dir().or_else(|| env::current_dir().ok())
}

fn get_current_or_default_contract_name() -> String {
    let current_contract_name = casper_multisig_tool::contract_name();
    if !current_contract_name.is_empty() {
        return current_contract_name;
    }
    "multisig_setup_contract".to_string()
}

fn new_button(label: &str) -> Button {
    let mut button = Button::default()
        .with_size(BUTTON_WIDTH, BUTTON_HEIGHT)
        .with_label(label);
    button.set_color(Color::from_u32(BUTTON_COLOR));
    button
}

fn main() {
    set_panic_handler();

    let app = App::default().with_scheme(Scheme::Gtk);

    let mut top_frame = Frame::new(PADDING, PADDING, 980, 80, "Add public key")
        .with_align(Align::TopLeft | Align::Inside);
    top_frame.set_frame(FrameType::PlasticDownFrame);

    let mut add_key_button_pack = Pack::new(
        2 * PADDING,
        40,
        WINDOW_WIDTH - (2 * PADDING),
        BUTTON_HEIGHT,
        "",
    );
    add_key_button_pack.set_spacing(30);
    add_key_button_pack.set_type(PackType::Horizontal);
    top_frame.set_size(
        top_frame.width(),
        add_key_button_pack.height() + add_key_button_pack.y(),
    );

    let mut add_public_key_from_file_button = new_button("Import from file");
    let mut add_public_key_from_hex_button = new_button("Enter hex-encoded public key");
    let mut add_account_hash_button = new_button("Enter hex-encoded account hash");

    add_key_button_pack.end();

    let mut middle_frame = Frame::new(
        PADDING,
        120,
        WINDOW_WIDTH - (2 * PADDING),
        40,
        "Current associated keys",
    )
    .with_align(Align::TopLeft | Align::Inside);
    middle_frame.set_frame(FrameType::PlasticDownFrame);

    let mut main_key_frame = Frame::new(
        15,
        175,
        WINDOW_WIDTH - 35,
        OUTPUT_ROW_HEIGHT + PADDING,
        "Main account  ",
    );
    main_key_frame.set_align(Align::Right | Align::Inside);
    main_key_frame.set_color(Color::from_u32(0xaed6f1));
    main_key_frame.set_frame(FrameType::FlatBox);
    main_key_frame.hide();

    let mut rust_output = TextDisplay::new(PADDING, 0, WINDOW_WIDTH - (2 * PADDING), 800, None);
    let buffer = TextBuffer::default();
    rust_output.set_buffer(Some(buffer.clone()));
    rust_output.set_text_font(Font::Courier);
    rust_output.set_text_size(14);
    rust_output.set_color(Color::from_u32(0xe0e8ee));

    let main_output_pack = MainOutputPack::new(
        add_public_key_from_file_button.clone(),
        add_public_key_from_hex_button.clone(),
        add_account_hash_button.clone(),
        buffer,
    );

    let mut bottom_frame =
        Frame::new(10, 10, 650, 90, "Action thresholds").with_align(Align::TopLeft | Align::Inside);
    bottom_frame.set_frame(FrameType::PlasticDownFrame);

    let action_thresholds_pack = ActionThresholdsPack::new(main_output_pack.clone());

    let mut generate_smart_contract_button = Button::new(
        WINDOW_WIDTH - PADDING - BUTTON_WIDTH,
        PADDING,
        BUTTON_WIDTH,
        BUTTON_HEIGHT,
        "Generate smart contract",
    );
    generate_smart_contract_button.set_color(Color::from_u32(0xc3fdb8));
    generate_smart_contract_button.deactivate();

    let main_output_pack_clone = main_output_pack.clone();
    add_public_key_from_file_button.set_callback(move |_| {
        let (account_hash, tooltip) = match get_account_hash_from_public_key_file() {
            Some(value) => value,
            None => return,
        };
        main_output_pack_clone.add_associated_key(&account_hash, &tooltip);
    });

    let main_output_pack_clone = main_output_pack.clone();
    add_public_key_from_hex_button.set_callback(move |_| {
        let (account_hash, tooltip) = match get_account_hash_from_hex_public_key() {
            Some(value) => value,
            None => return,
        };
        main_output_pack_clone.add_associated_key(&account_hash, &tooltip);
    });

    let main_output_pack_clone = main_output_pack.clone();
    add_account_hash_button.set_callback(move |_| {
        let (account_hash, tooltip) = match get_account_hash_from_formatted_account_hash() {
            Some(value) => value,
            None => return,
        };
        main_output_pack_clone.add_associated_key(&account_hash, &tooltip);
    });

    let main_output_pack_clone = main_output_pack.clone();
    let mut _child_output_worker = None;
    generate_smart_contract_button.set_callback(move |_| {
        _child_output_worker = main_output_pack_clone.generate_smart_contract();
    });

    let mut window = Window::default()
        .with_size(WINDOW_WIDTH, 10)
        .with_label(TOOL_NAME);
    window.insert(&top_frame, WindowIndices::TopFrame as i32);
    window.insert(&add_key_button_pack, WindowIndices::AddKeyButtonPack as i32);
    window.insert(&middle_frame, WindowIndices::MiddleFrame as i32);
    window.insert(&main_key_frame, WindowIndices::MainKeyFrame as i32);
    window.insert(&main_output_pack.pack, WindowIndices::MainOutputPack as i32);
    window.insert(&bottom_frame, WindowIndices::BottomFrame as i32);
    window.insert(
        &action_thresholds_pack.pack,
        WindowIndices::ActionThresholdsPack as i32,
    );
    window.insert(&rust_output, WindowIndices::RustOutput as i32);
    window.insert(
        &generate_smart_contract_button,
        WindowIndices::GenerateButton as i32,
    );

    let icon_contents = include_bytes!("../casperlabs_logo.png");
    let maybe_image = PngImage::from_data(icon_contents.as_ref()).ok();
    window.set_icon(maybe_image);

    main_output_pack.redraw_window();
    window.show_with_args(&["-name", TOOL_NAME]);

    while app.wait() && !app::should_program_quit() {
        thread::sleep(Duration::from_millis(1));
    }
}
