use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

pub const BRAND_DIR_NAME: &str = "Echoless";
pub const DATA_ROOT_ENV_VAR: &str = "ECHOLESS_DATA_ROOT";

pub fn brand_data_root() -> (PathBuf, String) {
    if let Some(dir) = env::var_os(DATA_ROOT_ENV_VAR).filter(|value| !value.is_empty()) {
        return (PathBuf::from(dir), DATA_ROOT_ENV_VAR.to_string());
    }

    default_brand_data_root()
}

pub fn diagnostics_dir() -> PathBuf {
    brand_data_root().0.join("diagnostics")
}

pub fn default_brand_data_root() -> (PathBuf, String) {
    default_brand_data_root_from(|name| env::var_os(name))
}

fn default_brand_data_root_from(var_os: impl Fn(&str) -> Option<OsString>) -> (PathBuf, String) {
    // An empty variable counts as unset; otherwise it resolves to a relative ./Echoless.
    let var = |name: &str| {
        var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };

    #[cfg(windows)]
    {
        let base = var("LOCALAPPDATA").unwrap_or_else(|| PathBuf::from("."));
        return (base.join(BRAND_DIR_NAME), "%LOCALAPPDATA%".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let base = var("HOME")
            .map(|home| home.join("Library").join("Application Support"))
            .unwrap_or_else(|| PathBuf::from("."));
        return (
            base.join(BRAND_DIR_NAME),
            "$HOME/Library/Application Support".to_string(),
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let base = var("XDG_DATA_HOME")
            .or_else(|| var("HOME").map(|home| home.join(".local").join("share")))
            .unwrap_or_else(|| PathBuf::from("."));
        return (base.join(BRAND_DIR_NAME), "XDG_DATA_HOME".to_string());
    }

    #[allow(unreachable_code)]
    {
        let _ = var;
        (PathBuf::from(BRAND_DIR_NAME), "fallback".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_environment_values_are_treated_as_unset() {
        let empty = default_brand_data_root_from(|_| Some(OsString::new()));
        let unset = default_brand_data_root_from(|_| None);

        assert_eq!(empty, unset);
    }

    #[cfg(unix)]
    #[test]
    fn empty_xdg_data_home_falls_back_to_home() {
        let (root, _) = default_brand_data_root_from(|name| match name {
            "HOME" => Some(OsString::from("/home/user")),
            _ => Some(OsString::new()),
        });

        assert!(root.is_absolute(), "{}", root.display());
        assert!(root.starts_with("/home/user"));
        assert!(root.ends_with(BRAND_DIR_NAME));
    }
}
