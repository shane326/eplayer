use std::path::PathBuf;

const APP_DIR_NAME: &str = "ePlayer";

pub fn app_data_dir() -> PathBuf {
    if let Some(path) = platform_app_data_dir() {
        return path.join(APP_DIR_NAME);
    }

    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

pub fn legacy_current_dir_file(file_name: &str) -> Option<PathBuf> {
    std::env::current_dir().ok().map(|dir| dir.join(file_name))
}

#[cfg(target_os = "windows")]
fn platform_app_data_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn platform_app_data_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library").join("Application Support"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_app_data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
fn platform_app_data_dir() -> Option<PathBuf> {
    None
}
