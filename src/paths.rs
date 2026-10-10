use std::path::PathBuf;

pub const DEFAULT_HISTORY_FILE: &str = "~/.local/state/jqplay/history";

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

pub fn expand_tilde(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), path == "~", home()) {
        (Some(rest), _, Some(h)) => h.join(rest),
        (None, true, Some(h)) => h,
        _ => PathBuf::from(path),
    }
}

pub fn default_config_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))?;
    Some(base.join("jqplay").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths_are_untouched() {
        assert_eq!(expand_tilde("/a/b"), PathBuf::from("/a/b"));
        assert_eq!(expand_tilde("rel/x"), PathBuf::from("rel/x"));
    }

    #[test]
    fn tilde_expands_when_home_is_set() {
        if let Some(h) = home() {
            assert_eq!(expand_tilde("~/x"), h.join("x"));
            assert_eq!(expand_tilde("~"), h);
        }
    }
}
