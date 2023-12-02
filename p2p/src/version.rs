use std::str::FromStr;

use bincode::{Decode, Encode};
use semver::VersionReq;

use crate::{Error, Result};

/// Represents the network version and protocol version used in karyon p2p.
///
/// # Example
///
/// ```
/// use karyon_p2p::Version;
///
/// let version: Version = "0.2.0, >0.1.0".parse().unwrap();
///
/// let version: Version = "0.2.0".parse().unwrap();
///
/// ```
#[derive(Debug, Clone)]
pub struct Version {
    pub v: VersionInt,
    pub req: VersionReq,
}

impl Version {
    /// Creates a new Version
    pub fn new(v: VersionInt, req: VersionReq) -> Self {
        Self { v, req }
    }
}

#[derive(Debug, Decode, Encode, Clone)]
pub struct VersionInt {
    major: u64,
    minor: u64,
    patch: u64,
}

impl FromStr for Version {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let v: Vec<&str> = s.split(", ").collect();
        if v.is_empty() || v.len() > 2 {
            return Err(Error::ParseError(format!("Invalid version{s}")));
        }

        let version: VersionInt = v[0].parse()?;
        let req: VersionReq = if v.len() > 1 { v[1] } else { v[0] }.parse()?;

        Ok(Self { v: version, req })
    }
}

impl FromStr for VersionInt {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let v: Vec<&str> = s.split('.').collect();
        if v.len() < 2 || v.len() > 3 {
            return Err(Error::ParseError(format!("Invalid version{s}")));
        }

        let major = v[0].parse::<u64>()?;
        let minor = v[1].parse::<u64>()?;
        let patch = v.get(2).unwrap_or(&"0").parse::<u64>()?;

        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for VersionInt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl From<VersionInt> for semver::Version {
    fn from(v: VersionInt) -> Self {
        semver::Version::new(v.major, v.minor, v.patch)
    }
}

/// Check if a version satisfies a version request.
pub fn version_match(version_req: &VersionReq, version: &VersionInt) -> bool {
    let version: semver::Version = version.clone().into();
    version_req.matches(&version)
}
