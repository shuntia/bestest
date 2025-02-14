use clap::builder::Str;
use log::*;
use serde::{Deserialize, Serialize};
use serde_json::{self, Map, Number};
use std::path::PathBuf;
use std::sync::*;
pub fn load_config<P: AsRef<std::path::Path>>(path: P) -> Result<Config, String> {
    let path = path.as_ref();
    let cp: ConfigParams =
        serde_json::from_str(&std::fs::read_to_string(path).expect("Failed to read config file"))
            .unwrap();
    let mut config = Config {
        lang: cp.lang.unwrap_or("".to_string()),
        exec: cp.exec.unwrap_or("".to_string()),
        args: cp.args.unwrap_or(vec![]),
        target: cp.target.unwrap_or("".to_string()),
        input: cp.input.unwrap_or(vec![]),
        output: cp.output.unwrap_or(vec![]),
        timeout: cp.timeout.unwrap_or(5),
        memory: cp.memory.unwrap_or(1024),
        threads: cp.threads.unwrap_or(4),
    };
    if config.lang == "" {
        info!("No language specified. Assuming with file extension.");
        let ext = std::path::Path::new()
            .extension()
            .expect("Expected file extension!")
            .to_str()
            .unwrap();
        match ext {
            "c" => {
                config.lang = "c".to_string();
                config.exec = "gcc".to_string();
                config.args = vec!["-o".to_string(), "a.out".to_string()];
            }
            "cpp" => {
                config.lang = "cpp".to_string();
                config.exec = "g++".to_string();
                config.args = vec!["-o".to_string(), "a.out".to_string()];
            }
            "rs" => {
                config.lang = "rs".to_string();
                config.exec = "rustc".to_string();
                config.args = vec!["-o".to_string(), "a.out".to_string()];
            }
            "py" => {
                config.lang = "py".to_string();
                config.exec = "python".to_string();
                config.args = vec![];
            }
            "java" => {
                config.lang = "java".to_string();
                config.exec = "javac".to_string();
                config.args = vec![];
            }
            _ => {
                panic!("Unsupported language: {}", ext);
            }
        }
    }
    if config.input.len() != config.output.len() {
        panic!("Input and output files must be equal in number");
    }
    unsafe {
        CONFIG.lock().unwrap().replace(config.clone());
    }
    return Ok(config);
}

pub static mut CONFIG: Mutex<Option<Config>> = Mutex::new(None);

#[derive(Serialize, Deserialize)]
struct ConfigParams {
    lang: Option<String>,
    exec: Option<String>,
    args: Option<Vec<String>>,
    target: Option<String>,
    input: Option<Vec<String>>,
    output: Option<Vec<String>>,
    timeout: Option<u64>,
    memory: Option<u64>,
    threads: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub lang: String,
    pub exec: String,
    pub args: Vec<String>,
    pub target: String,
    pub input: Vec<String>,
    pub output: Vec<String>,
    pub timeout: u64,
    pub memory: u64,
    pub threads: u64,
}
