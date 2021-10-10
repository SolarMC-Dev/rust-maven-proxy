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

use std::fs::{File, OpenOptions};
use std::path::Path;
use ron::de::from_reader;
use serde::{Deserialize, Serialize};
use hyper::Uri;
use std::str::FromStr;
use std::io::BufReader;
use ron::ser::to_writer_pretty;
use url::Url;
use std::time::Duration;

#[derive(PartialEq, Eq, Debug, Deserialize, Serialize)]
pub struct Config {
    port: u16,
    repositories: Vec<Url>,
    log_level: log::Level,
    #[serde(with = "DurationSerializable")]
    proxy_timeout: Duration
}

impl Config {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn repositories(&self) -> Vec<Uri> {
        let repos: &Vec<Uri> = &self.repositories
            .iter()
            .map(|url| Uri::from_str(url.as_str()).expect("URL should be validated by config load"))
            .collect();
        repos.clone()
    }

    pub fn log_level(&self) -> log::Level {
        self.log_level
    }

    pub fn proxy_timeout(&self) -> Duration {
        self.proxy_timeout
    }

    fn load_default() -> Self {
        let repositories: Vec<Url> = vec!(Url::parse("https://repo1.maven.org/maven2").unwrap());
        Self {
            port: 8080,
            repositories,
            log_level: log::Level::Info,
            proxy_timeout: Duration::from_secs(15)
        }
    }

    pub fn load_from(path: &Path) -> ron::Result<Config> {
        if !path.exists() {
            println!("Config {} does not exist; creating default config...", path.display());
            let mut write_options = OpenOptions::new();
            write_options.write(true).create_new(true);
            let writer = write_options.open(path)?;
            to_writer_pretty(writer, &Self::load_default(), Default::default())?;
        }
        let reader = BufReader::new(File::open(path)?);
        from_reader(reader)
    }
}

#[derive(PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(remote = "Duration")]
struct DurationSerializable {
    #[serde(getter = "Duration::as_secs")]
    secs: u64,
    #[serde(getter = "Duration::subsec_nanos")]
    nanos: u32
}

impl From<DurationSerializable> for Duration {
    fn from(def: DurationSerializable) -> Duration {
        Duration::new(def.secs, def.nanos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use eyre::Result;
    use hyper::http::uri::PathAndQuery;

    #[test]
    fn load_default_config() {
        let config = Config::load_default();
        assert_eq!(8080, config.port);
        let repos: Vec<Uri> = vec![Uri::from_str("https://repo1.maven.org/maven2").unwrap()];
        assert_eq!(repos, config.repositories());
        assert_eq!(log::Level::Info, config.log_level());
    }

    #[test]
    fn url_assumptions() -> Result<()> {
        let uri = "https://repo1.maven.org/maven2";
        assert_eq!(uri,
                   Url::from_str(uri)?.as_str());
        assert_eq!(Some(&PathAndQuery::from_str("/maven2")?),
                   Uri::from_str(uri)?.path_and_query());
        Url::from_str("bad_repo").expect_err("Did not expect to parse");
        Ok(())
    }

    #[test]
    fn write_new_config() -> Result<()> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join(Path::new("config.ron"));
        let config: Config = Config::load_from(&config_path)?;
        assert_eq!(Config::load_default(), config,
                   "The default config should have been written and loaded, not just any config");
        Ok(())
    }

    #[test]
    fn reload_default_config() -> Result<()> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join(Path::new("config.ron"));
        let _conf: Config = Config::load_from(&config_path)?;
        let config = Config::load_from(&config_path)?;
        assert_eq!(Config::load_default(), config,
                   "The default config should have been re-loaded, not just any config");
        Ok(())
    }

}