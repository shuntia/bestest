use std::{path::PathBuf, str::FromStr, sync::OnceLock};

use log::error;
use slint::{Model, Weak};
use tokio::task::JoinHandle;

use crate::config::ConfigParams;

slint::include_modules!();

#[allow(unused)]
static WINDOW_HANDLE: OnceLock<JoinHandle<()>> = OnceLock::new();
pub static WEAKREF: OnceLock<Weak<MainWindow>> = OnceLock::new();
static CONFIG: OnceLock<ConfigParams> = OnceLock::new();

pub fn launch() {
    let mw = MainWindow::new().unwrap();
    let _ = WEAKREF.set(mw.as_weak());
    mw.on_submit(|s: slint::ModelRc<slint::SharedString>| {
        let v: Vec<String> = s.iter().map(|el| el.as_str().to_owned()).collect();
        if CONFIG
            .set(ConfigParams {
                entry: Some(v[0].clone()),
                lang: Some(v[1].as_str().into()),
                args: Some(serde_json::from_str(&v[2]).unwrap()),
                target: Some(PathBuf::from_str(&v[3]).unwrap()),
                input: serde_json::from_str(&v[4]).ok(),
                output: serde_json::from_str(&v[5]).ok(),
                points: serde_json::from_str(&v[6]).ok(),
                timeout: v[7].parse::<u64>().ok(),
                memory: v[8].parse().ok(),
                threads: v[9].parse().ok(),
                checker: match v[10].as_str() {
                    "AST" => Some(crate::checker::Type::AST),
                    "Static" => Some(crate::checker::Type::Static),
                    _ => None,
                },
                allow: serde_json::from_str(&v[11]).unwrap(),
                format: Some(v[12].clone()),
                orderby: match v[13].as_str() {
                    "Name" => Some(crate::config::Orderby::Name),
                    "Id" => Some(crate::config::Orderby::Id),
                    _ => None,
                },
                dependencies: Some(
                    serde_json::from_str::<Vec<String>>(&v[14])
                        .unwrap()
                        .iter()
                        .map(|el| PathBuf::from_str(&el).unwrap())
                        .collect(),
                ),
            })
            .is_err()
        {
            error!("launch has already been called! CONFIG has already been set!");
        }
    });
    let _ = mw.run();
}

pub fn wait_for_config() -> &'static ConfigParams {
    #[feature(once_wait)]
    CONFIG.wait()
}

pub fn get_config() -> ConfigParams {
    ConfigParams::default()
}
