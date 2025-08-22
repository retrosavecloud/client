use anyhow::{Result, Context};
use tokio_tungstenite::{connect_async, tungstenite::Message, WebSocketStream, MaybeTlsStream};
use futures_util::{StreamExt, SinkExt};
use tokio::sync::{mpsc, RwLock};
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use tracing::{info, debug, warn, error};
use tokio::time::{Duration, interval};

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
}

/// WebSocket client for real-time updates
pub struct WebSocketClient {
    url: String,
    token: Option<String>,
    connection: Arc<RwLock<Option<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>>>,
    event_tx: mpsc::UnboundedSender<WsMessage>,
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
            reconnect_attempts: Arc::new(RwLock::new(0)),
        }
    }
    
    /// Set authentication token
    pub async fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }
    
    /// Connect to WebSocket server
    pub async fn connect(&self) -> Result<()> {
        info!("Connecting to WebSocket server at {}", self.url);
        
        let (ws_stream, _) = connect_async(&self.url).await
            .context("Failed to connect to WebSocket")?;
        
        let mut connection = self.connection.write().await;
        *connection = Some(ws_stream);
        
        // Reset reconnect attempts on successful connection
        let mut attempts = self.reconnect_attempts.write().await;
        *attempts = 0;
        
        info!("WebSocket connected successfully");
        
        // Send authentication message if we have a token
        if let Some(token) = &self.token {
            self.send_message(WsMessage::Auth { 
                token: token.clone() 
            }).await?;
        }
        
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
        let connection = self.connection.read().await;
        if let Some(ws) = connection.as_ref() {
            let json = serde_json::to_string(&message)?;
            let mut ws = self.connection.write().await;
            if let Some(stream) = ws.as_mut() {
                stream.send(Message::Text(json)).await
                    .context("Failed to send WebSocket message")?;
                debug!("Sent WebSocket message: {:?}", message);
            }
        } else {
            return Err(anyhow::anyhow!("WebSocket not connected"));
        }
        Ok(())
    }
    
    /// Start listening for messages
    pub async fn start_listening(self: Arc<Self>) {
        info!("Starting WebSocket listener");
        
        // Spawn heartbeat task
        let client = self.clone();
        tokio::spawn(async move {
            let mut heartbeat = interval(Duration::from_secs(30));
            loop {
                heartbeat.tick().await;
                if let Err(e) = client.send_message(WsMessage::Ping).await {
                    debug!("Failed to send heartbeat: {}", e);
                }
            }
        });
        
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