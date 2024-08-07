use std::path::{Path, PathBuf};

pub fn workspace_dir() -> PathBuf {
  let output = std::process::Command::new(env!("CARGO"))
    .arg("locate-project")
    .arg("--workspace")
    .arg("--message-format=plain")
    .output()
    .unwrap()
    .stdout;
  let cargo_path = Path::new(std::str::from_utf8(&output).unwrap().trim());
  cargo_path.parent().unwrap().to_path_buf()
}

pub fn workspace_path(suffix: &str) -> PathBuf {
  let path = format!(
    "{}/{}",
    workspace_dir().as_os_str().to_str().unwrap(),
    suffix
  );
  PathBuf::from(path)
}
