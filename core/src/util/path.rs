use std::path::PathBuf;

use crate::{error::Error, Result};

/// Returns the user's home directory as a `PathBuf`.
#[allow(dead_code)]
pub fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().ok_or(Error::PathNotFound("Home dir not found".to_string()))
}

/// Expands a tilde (~) in a path and returns the expanded `PathBuf`.
#[allow(dead_code)]
pub fn tilde_expand(path: &str) -> Result<PathBuf> {
    match path {
        "~" => home_dir(),
        p if p.starts_with("~/") => Ok(home_dir()?.join(&path[2..])),
        _ => Ok(PathBuf::from(path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tilde_expand() {
        let path = "~/src";
        let expanded_path = dirs::home_dir().unwrap().join("src");
        assert_eq!(tilde_expand(path).unwrap(), expanded_path);

        let path = "~";
        let expanded_path = dirs::home_dir().unwrap();
        assert_eq!(tilde_expand(path).unwrap(), expanded_path);

        let path = "";
        let expanded_path = PathBuf::from("");
        assert_eq!(tilde_expand(path).unwrap(), expanded_path);
    }
}
