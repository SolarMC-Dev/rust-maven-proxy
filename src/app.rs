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

use hyper::{Client, Server, Uri, Request, Response, Body, StatusCode, http};
use hyper::body::HttpBody;
use hyper::client::connect::Connect;
use hyper::service::{make_service_fn, service_fn};
use std::net::SocketAddr;
use std::sync::Arc;
use hyper::http::uri::PathAndQuery;
use hyper::http::request;
use futures_util::{StreamExt, FutureExt};
use futures_util::stream::FuturesUnordered;
use eyre::Result;
use std::str::FromStr;
use std::future::Future;
use tokio::time::timeout;
use std::time::Duration;
use std::error::Error;
use std::fmt::Debug;
use log::{log_enabled, Level};
use crate::request::AllowedMethod;

const PROGRAM_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Application<C> where C: Connect + Clone + Send + Sync + 'static {
    client: Client<C>,
    repositories: Vec<Uri>,
    proxy_timeout: Duration
}

impl<C> Application<C> where C: Connect + Clone + Send + Sync + 'static {
    pub fn new(client: Client<C>, repositories: Vec<Uri>, proxy_timeout: Duration) -> Self {
        Self {
            client,
            repositories,
            proxy_timeout
        }
    }

    fn homepage_response(version: http::version::Version) -> Result<Response<Body>> {
        let error_message = format!(
            "A maven repository proxy backed by rust-maven-proxy version {}", PROGRAM_VERSION);
        let response = Response::builder()
            .version(version)
            .status(200)
            .body(Body::from(error_message));
        Ok(response?)
    }

    async fn handle_request(&self,
                            original_request: Request<Body>) -> Result<Response<Body>> {

        let allowed_method = AllowedMethod::find_from(original_request.method());
        if allowed_method.is_none() {
            return AllowedMethod::respond_with_405(original_request.version());
        }
        let (parts, body) = original_request.into_parts();
        let gav: &PathAndQuery = match parts.uri.path_and_query() {
            None => {
                return Self::homepage_response(parts.version);
            }
            Some(path) => path
        };
        match gav.as_str() {
            "/" => {
                return Self::homepage_response(parts.version);
            },
            "/favicon.ico" => {
                return Ok(Response::builder()
                    .version(parts.version)
                    .status(404)
                    .body(Body::empty())?);
            },
            _ => {}
        }
        if !body.is_end_stream() {
            // Check if body is empty to conform to HTTP specification
            log::debug!("Received HTTP request with non-empty body: {:?}", &parts);
            return Ok(Response::builder()
                .version(parts.version)
                .status(400)
                .body(Body::from("A request must have an empty body"))?);
        }
        self.contact_proxies(&parts, gav).await
    }

    async fn contact_proxies(&self,
                             parts: &request::Parts,
                             gav: &PathAndQuery) -> Result<Response<Body>> {

        let mut futures = FuturesUnordered::new();
        // Dispatch all requests
        for proxy_uri in &self.repositories {
            let request = {
                let backend_uri = rewrite_uri(&proxy_uri, &gav)?;
                let mut request_builder = Request::builder();
                request_builder = copy_attributes(parts, request_builder);
                request_builder = request_builder.uri(backend_uri);
                request_builder.body(Body::empty())?
            };
            // Make request, add timeout, apply error handling
            log::trace!("Dispatching request to proxy repository: {:?}", request);
            let response_future = self.client.request(request);
            let response_future = timeout(self.proxy_timeout, response_future);
            let response_future = response_future.map(|result| {
                // Turn Result into Option and log errors in the process
                let opt_response: Option<Response<Body>> = handle_errors(result)
                    .map(handle_errors)
                    .flatten();
                // Filter status codes
                opt_response.filter(|response| match response.status() {
                    StatusCode::OK | StatusCode::NOT_MODIFIED => true,
                    StatusCode::NOT_FOUND => false,
                    status => {
                        if log_enabled!(Level::Debug) {
                            log::debug!("Received bad status {:?} from proxy response {:?}", status, response);
                        } else {
                            log::info!("Received bad status {:?} from a proxy response", status);
                        }
                        false
                    }
                })
            });
            futures.push(response_future);
        }
        loop {
            match futures.next().await {
                Some(Some(response)) => {
                    // Before returning, create a task to check errors in remaining requests
                    tokio::task::spawn(async move {
                        let _remaining: Vec<_> = futures.collect().await;
                    });
                    log::trace!("Found GAV {:?} from proxy response {:?}", &gav, &response);
                    return Ok(response);
                },
                Some(None) => continue, // Not found or in error
                None => break // No more requests remain in the stream
            };
        }
        log::trace!("Unable to find GAV {:?} in any proxy", gav);
        Ok(Response::builder()
            .version(parts.version)
            .status(404)
            .body(Body::from("No such artifact found in any of the proxy locations"))?)
    }

    pub async fn start_on<F>(self,
                             socket: SocketAddr,
                             shutdown_future: F) -> eyre::Result<()>
        where F: Future<Output=()> {

        let app: Arc<Self> = Arc::new(self);

        let service_function = make_service_fn(move |_| {
            let app = app.clone();
            async {
                Ok::<_, eyre::Error>(service_fn(move |request: Request<Body>| {
                    let app = app.clone();
                    async move { (&app).handle_request(request).await }
                }))
            }
        });
        let server = Server::bind(&socket).serve(service_function);

        Ok(server.with_graceful_shutdown(shutdown_future).await?)
    }

}

fn copy_attributes(parts : &request::Parts, mut request_builder: request::Builder) -> request::Builder {
    request_builder = request_builder
        .version(parts.version)
        .method(parts.method.clone());
    request_builder.headers_mut().unwrap()
        .extend(parts.headers.clone());
    request_builder
}

fn rewrite_uri(existing_uri: &Uri, gav: &PathAndQuery) -> core::result::Result<Uri, hyper::http::Error> {
    let mut builder = Uri::builder();
    if let Some(scheme) = existing_uri.scheme() {
        builder = builder.scheme(scheme.clone());
    }
    if let Some(authority) = existing_uri.authority() {
        builder = builder.authority(authority.clone());
    }
    // Combine proxy base path with incoming GAV path
    let proxy_path = if let Some(base_path) = existing_uri.path_and_query() {
        let mut combined_path = String::new();
        combined_path.push_str(base_path.as_str());
        combined_path.push_str(gav.as_str());
        PathAndQuery::from_str(combined_path.as_str())?
    } else {
        gav.clone()
    };
    builder
        .path_and_query(proxy_path)
        .build()
}

fn handle_errors<R, E>(result: core::result::Result<R, E>) -> Option<R> where E: Error + Debug {
    match result {
        Err(error) => {
            log::warn!("Error while contacting proxy: {:?}", error);
            None
        },
        Ok(value) => Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use crate::app;
    use hyper::Method;

    #[test]
    fn copy_attributes() -> Result<()> {
        let existing_request = Request::builder()
            .header("Accept", "text/html")
            .header("X-Custom-Foo", "foo")
            .method(Method::POST)
            .uri(Uri::from_str("https://repo1.maven.org/maven2")?)
            .body(Body::empty())?;
        let (existing_request_parts, _) = existing_request.into_parts();
        let mut request_builder = Request::builder();
        request_builder = app::copy_attributes(&existing_request_parts, request_builder);
        let new_request = request_builder.body(Body::empty())?;
        // copy_attributes does not include the URI
        assert_eq!(existing_request_parts.version, new_request.version());
        assert_eq!(existing_request_parts.method, new_request.method());
        assert_eq!(&existing_request_parts.headers, new_request.headers());
        Ok(())
    }

    #[test]
    fn rewrite_uri() -> Result<()> {
        let gav_raw = "/org/apache/maven/plugins/maven-compiler-plugin/3.8.1/maven-compiler-plugin-3.8.1.pom";
        let gav = PathAndQuery::from_str(gav_raw)?;
        let proxy_uri_raw = "https://repo1.maven.org/maven2";
        let proxy_uri = Uri::from_str(proxy_uri_raw)?;
        assert_eq!(
            Uri::from_str(&format!("{}{}", proxy_uri_raw, gav_raw))?,
            app::rewrite_uri(&proxy_uri, &gav)?);
        Ok(())
    }
}
