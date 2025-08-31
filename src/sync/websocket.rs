use anyhow::{Result, Context};
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream, MaybeTlsStream};
use futures_util::{StreamExt, SinkExt};
use tokio::sync::{mpsc, RwLock};
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use tracing::{info, debug, warn, error};
use tokio::time::{Duration, interval};
use super::event_handler::EventHandler;

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// Authentication message
    Auth { token: String },
    
    /// Heartbeat/ping
    Ping,
    
    /// Heartbeat/pong response
    Pong,
    
    /// Save uploaded notification
    SaveUploaded {
        game_id: String,
        game_name: String,
        emulator: String,
        save_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    
    /// Save deleted notification
    SaveDeleted {
        save_id: String,
    },
    
    /// Game registered notification
    GameRegistered {
        game_id: String,
        game_name: String,
        emulator: String,
    },
    
    /// Sync started by another device
    SyncStarted {
        device_id: String,
        device_name: String,
    },
    
    /// Sync completed by another device
    SyncCompleted {
        device_id: String,
        device_name: String,
        uploads: usize,
        downloads: usize,
    },
    
    /// Request to sync
    RequestSync,
    
    /// Error message
    Error {
        message: String,
    },
    
    /// Real-time subscription update
    SubscriptionUpdated {
        tier: String,
        status: String,
        billing_period: String,
        limits: SubscriptionLimits,
    },
    
    /// Real-time usage update
    UsageUpdated {
        saves_count: i32,
        saves_limit: i32,
        storage_bytes: i64,
        storage_limit_bytes: i64,
        devices_count: i32,
        devices_limit: i32,
    },
    
    /// Device added notification
    DeviceAdded {
        device_id: String,
        device_name: String,
        device_type: String,
    },
    
    /// Device removed notification
    DeviceRemoved {
        device_id: String,
        device_name: String,
    },
    
    /// Storage limit warning
    StorageLimitWarning {
        percentage: f32,
        message: String,
    },
    
    /// Save limit warning
    SaveLimitWarning {
        percentage: f32,
        message: String,
    },
    
    /// Settings updated notification
    SettingsUpdated {
        settings: crate::sync::settings_sync::UserSettingsResponse,
    },
}

/// Subscription limits for WebSocket messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionLimits {
    pub saves: Option<i32>,
    pub storage_gb: i32,
    pub devices: i32,
    pub family_members: i32,
}

/// WebSocket client for real-time updates
pub struct WebSocketClient {
    url: String,
    token: Option<String>,
    connection: Arc<RwLock<Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>>,
    event_tx: mpsc::UnboundedSender<WsMessage>,
    event_handler: Arc<EventHandler>,
    reconnect_attempts: Arc<RwLock<u32>>,
}

impl WebSocketClient {
    /// Create a new WebSocket client
    pub fn new(base_url: String, event_tx: mpsc::UnboundedSender<WsMessage>) -> Self {
        // Convert HTTP URL to WebSocket URL
        let ws_url = base_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        
        Self {
            url: format!("{}/ws", ws_url),
            token: None,
            connection: Arc::new(RwLock::new(None)),
            event_tx,
            event_handler: Arc::new(EventHandler::new()),
            reconnect_attempts: Arc::new(RwLock::new(0)),
        }
    }
    
    /// Get a reference to the event handler for registering callbacks
    pub fn event_handler(&self) -> Arc<EventHandler> {
        self.event_handler.clone()
    }
    
    /// Set authentication token
    pub async fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }
    
    /// Connect to WebSocket server
    pub async fn connect(&self) -> Result<()> {
        info!("[DEBUG] WebSocketClient::connect: Starting connection to {}", self.url);
        
        info!("[DEBUG] WebSocketClient::connect: Calling connect_async...");
        let (ws_stream, response) = connect_async(&self.url).await
            .context("Failed to connect to WebSocket")?;
        info!("[DEBUG] WebSocketClient::connect: Connection established, response status: {:?}", response.status());
        
        info!("[DEBUG] WebSocketClient::connect: Acquiring write lock on connection");
        {
            let mut connection = self.connection.write().await;
            info!("[DEBUG] WebSocketClient::connect: Write lock acquired, storing stream");
            *connection = Some(ws_stream);
            info!("[DEBUG] WebSocketClient::connect: Stream stored, dropping write lock");
        } // Explicitly drop the write lock here
        
        // Reset reconnect attempts on successful connection
        info!("[DEBUG] WebSocketClient::connect: Resetting reconnect attempts");
        {
            let mut attempts = self.reconnect_attempts.write().await;
            *attempts = 0;
        } // Explicitly drop the attempts lock
        
        info!("WebSocket connected successfully");
        
        // Send authentication message if we have a token
        if let Some(token) = &self.token {
            info!("[DEBUG] WebSocketClient::connect: Sending auth message");
            match self.send_message(WsMessage::Auth { 
                token: token.clone() 
            }).await {
                Ok(_) => {
                    info!("[DEBUG] WebSocketClient::connect: Auth message sent successfully");
                }
                Err(e) => {
                    error!("[DEBUG] WebSocketClient::connect: Failed to send auth message: {}", e);
                    return Err(e);
                }
            }
        } else {
            info!("[DEBUG] WebSocketClient::connect: No token, skipping auth");
        }
        
        info!("[DEBUG] WebSocketClient::connect: Connection complete, returning Ok");
        Ok(())
    }
    
    /// Disconnect from WebSocket server
    pub async fn disconnect(&self) -> Result<()> {
        let mut connection = self.connection.write().await;
        if let Some(mut ws) = connection.take() {
            ws.close(None).await?;
            info!("WebSocket disconnected");
        }
        Ok(())
    }
    
    /// Send a message to the server
    pub async fn send_message(&self, message: WsMessage) -> Result<()> {
        info!("[DEBUG] send_message: Starting, acquiring read lock");
        let connection = self.connection.read().await;
        info!("[DEBUG] send_message: Read lock acquired, checking connection");
        
        if let Some(_ws) = connection.as_ref() {
            info!("[DEBUG] send_message: Connection exists, serializing message");
            let json = serde_json::to_string(&message)?;
            info!("[DEBUG] send_message: Message serialized, dropping read lock and acquiring write lock");
            drop(connection); // Drop read lock before acquiring write lock
            
            let mut ws = self.connection.write().await;
            info!("[DEBUG] send_message: Write lock acquired");
            
            if let Some(stream) = ws.as_mut() {
                info!("[DEBUG] send_message: Sending message to stream");
                match stream.send(Message::Text(json)).await {
                    Ok(_) => {
                        info!("[DEBUG] send_message: Message sent successfully");
                        debug!("Sent WebSocket message: {:?}", message);
                    }
                    Err(e) => {
                        error!("[DEBUG] send_message: Failed to send: {}", e);
                        return Err(anyhow::anyhow!("Failed to send WebSocket message: {}", e));
                    }
                }
            } else {
                error!("[DEBUG] send_message: Stream is None after acquiring write lock");
                return Err(anyhow::anyhow!("WebSocket stream is None"));
            }
        } else {
            error!("[DEBUG] send_message: Connection is None");
            return Err(anyhow::anyhow!("WebSocket not connected"));
        }
        
        info!("[DEBUG] send_message: Completed successfully");
        Ok(())
    }
    
    /// Start listening for messages
    pub async fn start_listening(self: Arc<Self>) {
        info!("[DEBUG] start_listening: Starting WebSocket listener, Arc strong_count: {}", Arc::strong_count(&self));
        
        // Spawn heartbeat task
        let client = self.clone();
        info!("[DEBUG] start_listening: Spawning heartbeat task");
        tokio::spawn(async move {
            info!("[DEBUG] Heartbeat task: Started");
            let mut heartbeat = interval(Duration::from_secs(30));
            loop {
                heartbeat.tick().await;
                if let Err(e) = client.send_message(WsMessage::Ping).await {
                    debug!("[DEBUG] Heartbeat task: Failed to send heartbeat: {}", e);
                }
            }
        });
        info!("[DEBUG] start_listening: Heartbeat task spawned");
        
        // Main message loop
        loop {
            let connection = self.connection.read().await;
            if connection.is_none() {
                // Try to reconnect
                if let Err(e) = self.reconnect().await {
                    error!("Failed to reconnect: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            }
            drop(connection);
            
            // Read messages
            let mut connection = self.connection.write().await;
            if let Some(ws) = connection.as_mut() {
                match ws.next().await {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<WsMessage>(&text) {
                            Ok(message) => {
                                debug!("Received WebSocket message: {:?}", message);
                                
                                // Process message through event handler
                                let event_handler = self.event_handler.clone();
                                let msg_clone = message.clone();
                                tokio::spawn(async move {
                                    event_handler.handle_message(msg_clone).await;
                                });
                                
                                // Also send through the channel for backward compatibility
                                if let Err(e) = self.event_tx.send(message) {
                                    error!("Failed to send event: {}", e);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to parse WebSocket message: {}", e);
                            }
                        }
                    }
                    Some(Ok(Message::Binary(_))) => {
                        debug!("Received binary WebSocket message");
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if let Some(ws) = connection.as_mut() {
                            let _ = ws.send(Message::Pong(data)).await;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {
                        debug!("Received pong");
                    }
                    Some(Ok(Message::Close(_))) => {
                        info!("WebSocket closed by server");
                        *connection = None;
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error: {}", e);
                        *connection = None;
                    }
                    None => {
                        info!("WebSocket stream ended");
                        *connection = None;
                    }
                    _ => {}
                }
            }
        }
    }
    
    /// Reconnect to WebSocket server with exponential backoff
    async fn reconnect(&self) -> Result<()> {
        let mut attempts = self.reconnect_attempts.write().await;
        *attempts += 1;
        
        if *attempts > 10 {
            return Err(anyhow::anyhow!("Max reconnection attempts reached"));
        }
        
        let delay = Duration::from_secs(2_u64.pow((*attempts).min(5)));
        info!("Reconnecting to WebSocket in {:?} (attempt {})", delay, attempts);
        tokio::time::sleep(delay).await;
        
        self.connect().await
    }
    
    /// Send save uploaded notification
    pub async fn notify_save_uploaded(
        &self,
        game_id: String,
        game_name: String,
        emulator: String,
        save_id: String,
    ) -> Result<()> {
        self.send_message(WsMessage::SaveUploaded {
            game_id,
            game_name,
            emulator,
            save_id,
            timestamp: chrono::Utc::now(),
        }).await
    }
    
    /// Send sync started notification
    pub async fn notify_sync_started(&self, device_id: String, device_name: String) -> Result<()> {
        self.send_message(WsMessage::SyncStarted {
            device_id,
            device_name,
        }).await
    }
    
    /// Send sync completed notification
    pub async fn notify_sync_completed(
        &self,
        device_id: String,
        device_name: String,
        uploads: usize,
        downloads: usize,
    ) -> Result<()> {
        self.send_message(WsMessage::SyncCompleted {
            device_id,
            device_name,
            uploads,
            downloads,
        }).await
    }
}