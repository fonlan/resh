use russh::client;

pub struct ClientHandler;

impl client::Handler for ClientHandler {
    type Error = russh::Error;
    
    // Using default implementations for now
}
