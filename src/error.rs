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

use crate::error::ProxyError::{HyperHttp, Hyper, Timeout};
use std::fmt::{Display, Formatter};
use tokio::time::error::Elapsed;

#[derive(Debug)]
pub enum ProxyError {
    Hyper(hyper::Error),
    HyperHttp(hyper::http::Error),
    Timeout(Elapsed)
}

impl From<hyper::Error> for ProxyError {
    fn from(e: hyper::Error) -> Self {
        Hyper(e)
    }
}

impl From<hyper::http::Error> for ProxyError {
    fn from(e: hyper::http::Error) -> Self {
        HyperHttp(e)
    }
}

impl From<Elapsed> for ProxyError {
    fn from(e: Elapsed) -> Self { Timeout(e) }
}

impl Display for ProxyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", &self)
    }
}

impl std::error::Error for ProxyError {}
