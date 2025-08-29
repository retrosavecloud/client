use egui::{Context, Window, Grid, Button, RichText, Color32, ScrollArea};
use crate::sync::conflict_resolution::{SaveConflict, ResolutionAction, ConflictType};
use std::sync::Arc;
use tokio::sync::Mutex;

/// UI dialog for resolving save conflicts
pub struct ConflictDialog {
    conflicts: Vec<SaveConflict>,
    resolutions: Vec<ResolutionAction>,
    current_index: usize,
    show_dialog: bool,
    resolution_callback: Option<Arc<Mutex<dyn FnMut(Vec<ResolutionAction>) + Send>>>,
}

impl ConflictDialog {
    pub fn new(conflicts: Vec<SaveConflict>) -> Self {
        let resolutions = conflicts
            .iter()
            .map(|c| c.recommended_action.clone())
            .collect();
            
        Self {
            conflicts,
            resolutions,
            current_index: 0,
            show_dialog: true,
            resolution_callback: None,
        }
    }
    
    pub fn set_callback<F>(&mut self, callback: F) 
    where
        F: FnMut(Vec<ResolutionAction>) + Send + 'static
    {
        self.resolution_callback = Some(Arc::new(Mutex::new(callback)));
    }
    
    pub fn show(&mut self, ctx: &Context) {
        if !self.show_dialog || self.conflicts.is_empty() {
            return;
        }
        
        let current_index = self.current_index;
        let conflict = self.conflicts[current_index].clone();
        
        Window::new("🎮 Save Conflict Detected")
            .collapsible(false)
            .resizable(true)
            .default_width(500.0)
            .show(ctx, |ui| {
                // Header
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚠️").size(24.0).color(Color32::YELLOW));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&conflict.game_name).size(18.0).strong());
                        ui.label(format!("Game ID: {}", conflict.game_id));
                    });
                });
                
                ui.separator();
                
                // Conflict type description
                ui.label(self.describe_conflict(&conflict));
                
                ui.add_space(10.0);
                
                // Version comparison
                Grid::new("version_comparison")
                    .num_columns(3)
                    .spacing([20.0, 10.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Local Version").strong());
                        ui.label("");
                        ui.label(RichText::new("Cloud Version").strong());
                        ui.end_row();
                        
                        // Timestamp
                        ui.label(format!("📅 {}", 
                            conflict.local_version.timestamp.format("%Y-%m-%d %H:%M")));
                        ui.label("vs");
                        ui.label(format!("📅 {}", 
                            conflict.cloud_version.timestamp.format("%Y-%m-%d %H:%M")));
                        ui.end_row();
                        
                        // Save count (for memory cards)
                        if conflict.local_version.save_count > 0 || conflict.cloud_version.save_count > 0 {
                            ui.label(format!("💾 {} saves", conflict.local_version.save_count));
                            ui.label("vs");
                            ui.label(format!("💾 {} saves", conflict.cloud_version.save_count));
                            ui.end_row();
                        }
                        
                        // Device name if available
                        if let Some(ref device) = conflict.local_version.device_name {
                            ui.label(format!("🖥️ {}", device));
                        } else {
                            ui.label("🖥️ This device");
                        }
                        ui.label("");
                        if let Some(ref device) = conflict.cloud_version.device_name {
                            ui.label(format!("☁️ {}", device));
                        } else {
                            ui.label("☁️ Cloud");
                        }
                        ui.end_row();
                    });
                
                ui.add_space(10.0);
                ui.separator();
                
                // Resolution options
                ui.label(RichText::new("Choose Resolution:").strong());
                
                ui.horizontal(|ui| {
                    // Keep Local button
                    let local_color = if self.resolutions[current_index] == ResolutionAction::KeepLocal {
                        Color32::GREEN
                    } else {
                        Color32::from_rgb(100, 150, 100)
                    };
                    
                    if ui.add(
                        Button::new(RichText::new("📂 Keep Local").color(Color32::WHITE))
                            .fill(local_color)
                    ).clicked() {
                        self.resolutions[current_index] = ResolutionAction::KeepLocal;
                    }
                    
                    // Use Cloud button
                    let cloud_color = if self.resolutions[current_index] == ResolutionAction::UseCloud {
                        Color32::BLUE
                    } else {
                        Color32::from_rgb(100, 100, 150)
                    };
                    
                    if ui.add(
                        Button::new(RichText::new("☁️ Use Cloud").color(Color32::WHITE))
                            .fill(cloud_color)
                    ).clicked() {
                        self.resolutions[current_index] = ResolutionAction::UseCloud;
                    }
                    
                    // Skip button
                    let skip_color = if self.resolutions[current_index] == ResolutionAction::Skip {
                        Color32::GRAY
                    } else {
                        Color32::from_rgb(120, 120, 120)
                    };
                    
                    if ui.add(
                        Button::new(RichText::new("⏭️ Skip").color(Color32::WHITE))
                            .fill(skip_color)
                    ).clicked() {
                        self.resolutions[current_index] = ResolutionAction::Skip;
                    }
                });
                
                // Recommendation
                ui.add_space(5.0);
                ui.label(
                    RichText::new(format!("💡 Recommended: {}", 
                        self.action_to_string(&conflict.recommended_action)))
                    .italics()
                    .color(Color32::LIGHT_GRAY)
                );
                
                ui.add_space(10.0);
                ui.separator();
                
                // Navigation buttons
                ui.horizontal(|ui| {
                    ui.label(format!("Conflict {} of {}", 
                        current_index + 1, 
                        self.conflicts.len()
                    ));
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Apply All button
                        if ui.button("✅ Apply All").clicked() {
                            self.apply_resolutions();
                            self.show_dialog = false;
                        }
                        
                        // Next/Previous
                        if current_index < self.conflicts.len() - 1 {
                            if ui.button("Next ➡️").clicked() {
                                self.current_index += 1;
                            }
                        }
                        
                        if current_index > 0 {
                            if ui.button("⬅️ Previous").clicked() {
                                self.current_index -= 1;
                            }
                        }
                        
                        // Cancel button
                        if ui.button("❌ Cancel").clicked() {
                            self.show_dialog = false;
                        }
                    });
                });
            });
    }
    
    fn describe_conflict(&self, conflict: &SaveConflict) -> String {
        match conflict.conflict_type {
            ConflictType::BothModified => {
                "Both local and cloud versions have been modified since last sync.".to_string()
            }
            ConflictType::LocalNewer => {
                let duration = conflict.local_version.timestamp
                    .signed_duration_since(conflict.cloud_version.timestamp);
                format!("Your local save is newer by {}.", 
                    self.format_duration(duration))
            }
            ConflictType::CloudNewer => {
                let duration = conflict.cloud_version.timestamp
                    .signed_duration_since(conflict.local_version.timestamp);
                format!("The cloud save is newer by {}.", 
                    self.format_duration(duration))
            }
            ConflictType::SameTimeButDifferent => {
                "Both saves have the same timestamp but different content.".to_string()
            }
            ConflictType::LocalOnly => {
                format!("{} only exists locally and not in the cloud.", conflict.game_name)
            }
            ConflictType::CloudOnly => {
                format!("{} only exists in the cloud and not locally.", conflict.game_name)
            }
        }
    }
    
    fn format_duration(&self, duration: chrono::Duration) -> String {
        let days = duration.num_days();
        let hours = duration.num_hours() % 24;
        let minutes = duration.num_minutes() % 60;
        
        if days > 0 {
            format!("{} days, {} hours", days, hours)
        } else if hours > 0 {
            format!("{} hours, {} minutes", hours, minutes)
        } else {
            format!("{} minutes", minutes)
        }
    }
    
    fn action_to_string(&self, action: &ResolutionAction) -> &str {
        match action {
            ResolutionAction::KeepLocal => "Keep Local",
            ResolutionAction::UseCloud => "Use Cloud",
            ResolutionAction::Merge => "Merge",
            ResolutionAction::Skip => "Skip",
            ResolutionAction::AskUser => "Ask User",
        }
    }
    
    fn apply_resolutions(&mut self) {
        if let Some(callback) = &self.resolution_callback {
            let resolutions = self.resolutions.clone();
            let callback = Arc::clone(callback);
            
            tokio::spawn(async move {
                let mut cb = callback.lock().await;
                cb(resolutions);
            });
        }
    }
    
    pub fn is_open(&self) -> bool {
        self.show_dialog
    }
}