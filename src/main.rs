/*
 * rust-maven-proxy
 * Copyright © 2021 SolarMC Developers
 *
 * rust-maven-proxy is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * rust-maven-proxy is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with rust-maven-proxy. If not, see <https://www.gnu.org/licenses/>
 * and navigate to version 3 of the GNU Affero General Public License.
 */

#![forbid(unsafe_code)]

mod app;
mod config;

use app::Application;
use hyper::Client;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use crate::config::Config;
use eyre::Result;
use simple_logger::SimpleLogger;
use hyper_rustls::HttpsConnector;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    stable_eyre::install()?;

    let config_path = Path::new("config.ron");
    println!("Loading configuration from {:?}", config_path);
    let config = Config::load_from(config_path).expect("Failed to load config");

    SimpleLogger::new()
        .with_level(config.log_level().to_level_filter())
        .init().expect("Logging initialization failure");

    let port = config.port();
    log::info!("Starting rust maven proxy on port {} ... ", port);

    let application = {
        let https_connector = HttpsConnector::with_native_roots();
        let client = Client::builder().build(https_connector);
        let repositories = config.repositories();
        log::info!("Using repositories {:?}", &repositories);
        Application::new(client, repositories, config.proxy_timeout())
    };
    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let server = application.start_on(socket, shutdown_signal());

    log::info!("Started server");

    server.await
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C handler");
    log::info!("Stopping server due to CTRL+C press");
}
