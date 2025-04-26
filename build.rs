#[cfg(feature = "gui")]
use std::{path::PathBuf, str::FromStr};

fn main() {
    #[cfg(feature = "gui")]
    for i in PathBuf::from_str("ui").unwrap().read_dir().unwrap() {
        slint_build::compile(i.unwrap().path()).unwrap();
    }
}
