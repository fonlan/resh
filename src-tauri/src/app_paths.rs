use std::path::{Path, PathBuf};

pub const APP_DATA_DIR_NAME: &str = "Resh";

pub fn resolve_app_data_dir_from_default(default_app_data_dir: &Path) -> PathBuf {
    default_app_data_dir
        .parent()
        .map(|parent| parent.join(APP_DATA_DIR_NAME))
        .unwrap_or_else(|| default_app_data_dir.join(APP_DATA_DIR_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_bundle_identifier_dir_to_resh_dir() {
        let default_dir = Path::new("/Users/alice/Library/Application Support/com.fonlan.resh");
        let resolved = resolve_app_data_dir_from_default(default_dir);

        assert_eq!(
            resolved,
            PathBuf::from("/Users/alice/Library/Application Support/Resh")
        );
    }

    #[test]
    fn appends_resh_when_default_has_no_parent() {
        let resolved = resolve_app_data_dir_from_default(Path::new("com.fonlan.resh"));

        assert_eq!(resolved, PathBuf::from("Resh"));
    }

    #[test]
    fn keeps_platform_separator_handling_inside_pathbuf() {
        let resolved =
            resolve_app_data_dir_from_default(Path::new("/tmp/App Support/com.fonlan.resh"));

        assert_eq!(resolved, Path::new("/tmp/App Support").join(APP_DATA_DIR_NAME));
        assert_eq!(
            resolved.file_name().and_then(|name| name.to_str()),
            Some("Resh")
        );
    }
}
