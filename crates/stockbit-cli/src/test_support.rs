//! Shared test helpers — only compiled under `#[cfg(test)]`. Centralises the
//! `wiremock` client builder so each endpoint module's tests stay focused on the
//! endpoint-specific wire contract instead of restating identical boilerplate.

#![cfg(test)]

use std::time::Duration;

use wiremock::MockServer;

use crate::client::Client;
use crate::config::Config;

/// Build a `Client` wired to a `wiremock::MockServer` with retries disabled fast.
pub fn test_client(server: &MockServer) -> Client {
    Client::new(&Config::for_test(server.uri(), "test-token"))
        .expect("client builds")
        .with_retry_base(Duration::from_millis(1))
}
