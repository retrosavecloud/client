use eframe::egui;
use tokio::sync::mpsc;
use tracing::info;

#[derive(Debug, Clone)]
pub enum AuthEvent {
    Login { email: String, password: String },
    Register { email: String, username: String, password: String },
    Logout,
}

#[derive(Debug, Clone, PartialEq)]
enum AuthMode {
    Login,
    Register,
}

pub struct CloudAuthDialog {
    mode: AuthMode,
    event_tx: Option<mpsc::UnboundedSender<AuthEvent>>,
    
    // Form fields
    email: String,
    username: String,
    password: String,
    confirm_password: String,
    
    // State
    error_message: Option<String>,
}

impl CloudAuthDialog {
    pub fn new() -> Self {
        Self {
            mode: AuthMode::Login,
            event_tx: None,
            email: String::new(),
            username: String::new(),
            password: String::new(),
            confirm_password: String::new(),
            error_message: None,
        }
    }
    
    pub fn set_event_sender(&mut self, tx: mpsc::UnboundedSender<AuthEvent>) {
        self.event_tx = Some(tx);
    }
    
    pub fn show_login(&mut self) {
        self.mode = AuthMode::Login;
        self.clear_form();
    }
    
    pub fn show_register(&mut self) {
        self.mode = AuthMode::Register;
        self.clear_form();
    }
    
    fn clear_form(&mut self) {
        self.email.clear();
        self.username.clear();
        self.password.clear();
        self.confirm_password.clear();
        self.error_message = None;
    }
    
    pub fn ui(&mut self, ui: &mut egui::Ui, is_authenticated: bool, user_email: Option<&str>) {
        if is_authenticated {
            // Show logged in state
            ui.horizontal(|ui| {
                ui.label("Logged in as:");
                ui.label(user_email.unwrap_or("Unknown"));
            });
            
            if ui.button("Logout").clicked() {
                if let Some(tx) = &self.event_tx {
                    let _ = tx.send(AuthEvent::Logout);
                }
            }
        } else {
            // Show login/register form
            if let Some(error) = &self.error_message {
                ui.colored_label(egui::Color32::from_rgb(255, 100, 100), format!("❌ {}", error));
                ui.separator();
            }
            
            // Form fields
            ui.horizontal(|ui| {
                ui.label("Email:");
                ui.text_edit_singleline(&mut self.email);
            });
            
            if self.mode == AuthMode::Register {
                ui.horizontal(|ui| {
                    ui.label("Username:");
                    ui.text_edit_singleline(&mut self.username);
                });
            }
            
            ui.horizontal(|ui| {
                ui.label("Password:");
                ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
            });
            
            if self.mode == AuthMode::Register {
                ui.horizontal(|ui| {
                    ui.label("Confirm:");
                    ui.add(egui::TextEdit::singleline(&mut self.confirm_password).password(true));
                });
            }
            
            ui.separator();
            
            // Action buttons
            ui.horizontal(|ui| {
                if self.mode == AuthMode::Login {
                    if ui.button("Login").clicked() {
                        self.perform_login();
                    }
                    
                    ui.separator();
                    
                    if ui.link("Need an account? Register").clicked() {
                        self.show_register();
                    }
                } else {
                    if ui.button("Register").clicked() {
                        self.perform_register();
                    }
                    
                    ui.separator();
                    
                    if ui.link("Already have an account? Login").clicked() {
                        self.show_login();
                    }
                }
            });
        }
    }
    
    fn perform_login(&mut self) {
        // Validate
        if self.email.is_empty() || self.password.is_empty() {
            self.error_message = Some("Please fill in all fields".to_string());
            return;
        }
        
        self.error_message = None;
        
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(AuthEvent::Login {
                email: self.email.clone(),
                password: self.password.clone(),
            });
            info!("Login event sent");
        }
    }
    
    fn perform_register(&mut self) {
        // Validate
        if self.email.is_empty() || self.username.is_empty() || self.password.is_empty() {
            self.error_message = Some("Please fill in all fields".to_string());
            return;
        }
        
        if self.password != self.confirm_password {
            self.error_message = Some("Passwords do not match".to_string());
            return;
        }
        
        if self.password.len() < 8 {
            self.error_message = Some("Password must be at least 8 characters".to_string());
            return;
        }
        
        self.error_message = None;
        
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(AuthEvent::Register {
                email: self.email.clone(),
                username: self.username.clone(),
                password: self.password.clone(),
            });
            info!("Register event sent");
        }
    }
}