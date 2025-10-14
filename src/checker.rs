use indicatif::{MultiProgress, ProgressBar};
use log::{debug, error, warn};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::{
    sync::{Mutex, Semaphore},
    task,
};

use anyhow::{Ok, Result};
use walkdir::WalkDir;

use crate::config;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Type {
    AST,
    Static,
}

pub async fn check_dirs(paths: Vec<PathBuf>) -> Result<HashMap<PathBuf, Vec<IllegalExpr>>> {
    crate::config::get_config()?;
    let results = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let max_threads = config::get_config()?.threads;
    let semaphore = Arc::new(Semaphore::new(usize::try_from(max_threads)?));
    let errors = Arc::new(tokio::sync::Mutex::new(Vec::<(PathBuf, String)>::new()));

    // Wrap MultiProgress in an Arc so it can be shared between tasks.
    let mp = Arc::new(MultiProgress::new());
    mp.clear()?;

    // Collect entries first
    let mut entries = vec![];
    for path in paths {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter(|el| {
                config::KNOWN_EXTENSIONS.contains(
                    el.as_ref()
                        .unwrap()
                        .path()
                        .extension()
                        .map_or("java", |s| s.to_str().unwrap()),
                )
            })
            .filter(|el| el.as_ref().unwrap().path().is_file())
        {
            entries.push(entry?.into_path());
        }
    }

    // Create a progress bar for overall progress using the correct count.
    let op = Arc::new(tokio::sync::Mutex::new(
        mp.add(ProgressBar::new(entries.len() as u64)),
    ));
    let mut handles = vec![];

    for entry in entries {
        let results = Arc::<
            tokio::sync::Mutex<
                std::collections::HashMap<std::path::PathBuf, std::vec::Vec<IllegalExpr>>,
            >,
        >::clone(&results);
        let semaphore = Arc::<tokio::sync::Semaphore>::clone(&semaphore);
        let errors = Arc::<
            tokio::sync::Mutex<std::vec::Vec<(std::path::PathBuf, std::string::String)>>,
        >::clone(&errors);
        let op = Arc::<tokio::sync::Mutex<indicatif::ProgressBar>>::clone(&op);
        let mp = Arc::<indicatif::MultiProgress>::clone(&mp);
        let handle = tokio::spawn(changefile_prog(results, semaphore, entry, errors, op, mp));
        handles.push(handle);
    }
    op.lock().await.finish_and_clear();
    for h in handles {
        h.await?;
    }
    let value = errors.lock().await.clone();
    for e in value {
        error!("{} at {:?}", e.1, e.0);
    }
    mp.clear()?;
    let ret = core::mem::take(&mut *results.lock().await);
    Ok(ret)
}

pub async fn changefile_prog(
    results: Arc<tokio::sync::Mutex<HashMap<PathBuf, Vec<IllegalExpr>>>>,
    semaphore: Arc<Semaphore>,
    entry: PathBuf,
    errors: Arc<tokio::sync::Mutex<Vec<(PathBuf, String)>>>,
    op: Arc<tokio::sync::Mutex<ProgressBar>>,
    _mp: Arc<MultiProgress>,
) {
    //let prog = mp
    //    .add(ProgressBar::new_spinner())
    //    .with_message(entry.file_name()?.to_str()?.to_owned());
    //prog.set_style(
    //    ProgressStyle::default_spinner()
    //        .template("{spinner} checking {msg}")
    //        ?,
    //);
    //prog.enable_steady_tick(Duration::from_millis(50));
    let _ = changefile(results, semaphore, entry, errors).await;
    //prog.finish_and_clear();
    op.lock().await.inc(1);
}

pub async fn check_dir(path: std::path::PathBuf) -> Result<HashMap<PathBuf, Vec<IllegalExpr>>> {
    crate::config::get_config()?;
    if path.is_file() {
        let mut ret = HashMap::new();
        ret.insert(path.clone(), check_file(path)?);
        return Ok(ret);
    }
    let results: Arc<Mutex<HashMap<PathBuf, Vec<IllegalExpr>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let max_threads = config::get_config()?.threads;
    let semaphore = Arc::new(Semaphore::new(usize::try_from(max_threads)?));
    let mut handles = vec![];
    let errors = Arc::new(Mutex::new(Vec::<(PathBuf, String)>::new()));
    for entry in WalkDir::new(path).into_iter().filter(|el| {
        config::KNOWN_EXTENSIONS.contains(
            el.as_ref()
                .unwrap()
                .clone()
                .path()
                .extension()
                .as_ref()
                .map_or_else(
                    || {
                        warn!("failed to read extension! checking anyway.");
                        "java"
                    },
                    |s| s.to_str().unwrap(),
                ),
        )
    }) {
        let handle =
            task::spawn(
                changefile(
                    Arc::<
                        tokio::sync::Mutex<
                            std::collections::HashMap<
                                std::path::PathBuf,
                                std::vec::Vec<IllegalExpr>,
                            >,
                        >,
                    >::clone(&results),
                    Arc::<tokio::sync::Semaphore>::clone(&semaphore),
                    entry?.into_path(),
                    Arc::<
                        tokio::sync::Mutex<
                            std::vec::Vec<(std::path::PathBuf, std::string::String)>,
                        >,
                    >::clone(&errors),
                ),
            );
        handles.push(handle);
    }
    for h in handles {
        h.await??;
    }
    let value = errors.lock().await.clone();
    for e in value {
        error!("{} at {:?}", e.1, e.0);
    }
    let ret = core::mem::take(&mut *results.lock().await);
    Ok(ret)
}
async fn changefile(
    results: Arc<Mutex<HashMap<PathBuf, Vec<IllegalExpr>>>>,
    semaphore: Arc<Semaphore>,
    dir: PathBuf,
    errs: Arc<Mutex<Vec<(PathBuf, String)>>>,
) -> Result<()> {
    let permit = semaphore.acquire().await?;
    results.lock().await.insert(
        dir.clone(),
        match check_file(dir.clone()) {
            Err(e) => {
                error!("ERROR");
                errs.lock().await.push((dir.clone(), e.to_string()));
                return Ok(());
            }
            Result::Ok(o) => o,
        },
    );
    drop(permit);
    Ok(())
}

pub fn check_file(path: std::path::PathBuf) -> Result<Vec<IllegalExpr>> {
    debug!("checking {path:?}");
    let cfg = crate::config::get_config()?;
    match cfg.checker {
        Type::AST => {
            warn!("AST checker is not supported yet. falling back to static analysis.");
            static_check::check(&path)
        }
        Type::Static => static_check::check(&path),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct IllegalExpr {
    pub content: Option<String>,
    pub violates: Option<static_check::Allow>,
    pub loc: (usize, usize),
    pub path: PathBuf,
}

pub mod static_check {
    use std::{collections::HashSet, fs::File, io::Read as _, path::PathBuf};

    use anyhow::Result;
    use log::warn;
    use strum::IntoEnumIterator as _;
    use strum_macros::{AsRefStr, EnumIter};

    use crate::executable::Language;

    use super::IllegalExpr;
    pub fn check(path: &PathBuf) -> Result<Vec<IllegalExpr>> {
        let allowcfg = crate::config::get_config()?.allow.clone();
        let lang: Language = path.extension().unwrap().to_str().unwrap().into();
        let mut allowed = HashSet::new();
        for i in allowcfg {
            allowed.insert(match Allow::from_str(i.as_str()).first() {
                Some(s) => s.clone(),
                None => {
                    warn!("potentially illegal config!");
                    continue;
                }
            });
        }
        if allowed.contains(&Allow::All) {
            return Ok(vec![]);
        }
        let prohibited: Vec<Allow> = Allow::iter().filter(|el| !allowed.contains(el)).collect();
        let mut prohibited_str: Vec<(Allow, &str)> = Vec::new();
        for i in &prohibited {
            for j in i.get_prohibited(&lang.clone()) {
                prohibited_str.push((i.clone(), j));
            }
        }
        let mut f = File::open(path)?;
        let mut s: String = String::new();
        let _ = f.read_to_string(&mut s);
        let mut illegal: Vec<(usize, Allow)> = vec![];
        for i in prohibited_str {
            if let Some(s) = s.find(i.1) {
                illegal.push((s, i.0.clone()))
            }
        }
        let mut ret = vec![];
        let mut indents: Vec<usize> = vec![];
        for i in s.split('\n') {
            indents.push(i.len());
        }
        for i in illegal {
            let mut it = indents.iter();
            let mut idts = 0;
            while it.next().unwrap() > &i.0 {
                idts += 1;
            }
            ret.push(IllegalExpr {
                loc: (i.0 - indents[idts], idts),
                content: None,
                path: path.clone(),
                violates: Some(i.1),
            });
        }
        Ok(ret)
    }

    pub trait Prohibit {
        fn get_prohibited(&self) -> Vec<&str>;
    }
    /*pub enum Allow {
            Java(AllowJava),
            C(AllowC),
            Cpp(AllowC),
            Rust(AllowRs),
            Python(AllowPy),
            Guess(AllowGuess),
        }

        impl Prohibit for Allow {
            fn get_prohibited(&self) -> Vec<&str> {
                match self {
                    Self::Java(x) => x.get_prohibited(),
                    Self::C(x) => x.get_prohibited(),
                    Self::Cpp(x) => x.get_prohibited(),
                    Self::Rust(x) => x.get_prohibited(),
                    Self::Python(x) => x.get_prohibited(),
                    Self::Guess(x) => x.get_prohibited(),
                }
            }
        }
    */

    #[derive(PartialEq, Eq, Hash, Debug, Clone, EnumIter, AsRefStr)]
    #[non_exhaustive]
    pub enum Allow {
        FileIO,
        SysAccess,
        Runtime,
        Threading,
        Reflection,
        ProcessExec,
        SystemCall,
        Network,
        Assembly,
        Signal,
        Process,
        Unsafe,
        FFI,
        Command,
        OsAccess,
        Eval,
        Exec,
        Import,
        Ctypes,
        Pickle,
        Unknown,
        All,
    }
    impl Allow {
        fn from_str(s: &str) -> Vec<Self> {
            let mut ret = vec![];
            for i in Self::iter() {
                if s.contains(i.as_ref()) {
                    ret.push(i);
                }
            }
            ret
        }
        fn get_prohibited(&self, lang: &Language) -> Vec<&'static str> {
            match lang {
                Language::Unknown(_) | Language::Guess => vec![],
                Language::C => match &self {
                    Self::SystemCall => {
                        vec![
                            "fork", "exec", "system", "popen", "vfork", "execl", "execlp",
                            "execle", "execv", "execvp", "execve",
                        ]
                    }
                    Self::FileIO => vec!["fopen", "fread", "fwrite", "fclose"],
                    Self::Network => vec!["socket", "bind", "connect", "recv", "send"],
                    Self::Assembly => vec!["asm", "__asm__"],
                    Self::Signal => vec!["signal", "raise"],
                    Self::Process => vec!["wait", "waitpid"],
                    Self::All => {
                        vec![
                            "fork", "exec", "system", "popen", "vfork", "execl", "execlp",
                            "execle", "execv", "execvp", "execve", "fopen", "fread", "fwrite",
                            "fclose", "socket", "bind", "connect", "recv", "send", "asm",
                            "__asm__", "signal", "raise", "wait", "waitpid",
                        ]
                    }
                    Self::SysAccess
                    | Self::Runtime
                    | Self::Threading
                    | Self::Reflection
                    | Self::ProcessExec
                    | Self::Unsafe
                    | Self::FFI
                    | Self::Command
                    | Self::OsAccess
                    | Self::Eval
                    | Self::Exec
                    | Self::Import
                    | Self::Ctypes
                    | Self::Pickle
                    | Self::Unknown => vec![],
                },
                Language::Cpp => match &self {
                    Self::SystemCall => {
                        vec![
                            "fork", "exec", "system", "popen", "vfork", "execl", "execlp",
                            "execle", "execv", "execvp", "execve",
                        ]
                    }
                    Self::FileIO => vec!["fopen", "fread", "fwrite", "fclose"],
                    Self::Network => vec!["socket", "bind", "connect", "recv", "send"],
                    Self::Assembly => vec!["asm", "__asm__"],
                    Self::Signal => vec!["signal", "raise"],
                    Self::Process => vec!["wait", "waitpid"],
                    Self::All => {
                        vec![
                            "fork", "exec", "system", "popen", "vfork", "execl", "execlp",
                            "execle", "execv", "execvp", "execve", "fopen", "fread", "fwrite",
                            "fclose", "socket", "bind", "connect", "recv", "send", "asm",
                            "__asm__", "signal", "raise", "wait", "waitpid",
                        ]
                    }
                    Self::SysAccess
                    | Self::Runtime
                    | Self::Threading
                    | Self::Reflection
                    | Self::ProcessExec
                    | Self::Unsafe
                    | Self::FFI
                    | Self::Command
                    | Self::OsAccess
                    | Self::Eval
                    | Self::Exec
                    | Self::Import
                    | Self::Ctypes
                    | Self::Pickle
                    | Self::Unknown => vec![],
                },

                Language::Rust => match &self {
                    Self::Unsafe => vec!["unsafe"],
                    Self::FileIO => vec!["std::fs::File", "std::io"],
                    Self::Network => vec!["std::net", "TcpStream", "UdpSocket"],
                    Self::Threading => vec!["std::thread"],
                    Self::FFI => vec!["extern", "libc", "std::os::unix::process::Command"],
                    Self::Command => vec!["std::process::Command"],
                    Self::Reflection => vec!["reflect"],
                    Self::All => {
                        vec![
                            "unsafe",
                            "std::fs::File",
                            "std::io",
                            "std::net",
                            "TcpStream",
                            "UdpSocket",
                            "std::thread",
                            "extern",
                            "libc",
                            "std::os::unix::process::Command",
                            "std::process::Command",
                            "reflection",
                        ]
                    }
                    Self::SysAccess
                    | Self::Runtime
                    | Self::ProcessExec
                    | Self::SystemCall
                    | Self::Assembly
                    | Self::Signal
                    | Self::Process
                    | Self::OsAccess
                    | Self::Eval
                    | Self::Exec
                    | Self::Import
                    | Self::Ctypes
                    | Self::Pickle
                    | Self::Unknown => vec![],
                },
                Language::Python => match &self {
                    Self::OsAccess => vec!["os.system", "os.popen"],
                    Self::Eval => vec!["eval("],
                    Self::Exec => vec!["exec("],
                    Self::FileIO => vec!["open("],
                    Self::Threading => vec!["threading.Thread"],
                    Self::Network => vec!["socket", "requests.get", "urllib", "subprocess"],
                    Self::Import => vec!["__import__"],
                    Self::Ctypes => vec!["ctypes"],
                    Self::Pickle => vec!["pickle.loads", "pickle.dumps"],
                    Self::All => {
                        vec![
                            "os.system",
                            "os.popen",
                            "eval(",
                            "exec(",
                            "open(",
                            "threading.Thread",
                            "socket",
                            "requests.get",
                            "urllib",
                            "subprocess",
                            "__import__",
                            "ctypes",
                            "pickle.loads",
                            "pickle.dumps",
                        ]
                    }
                    Self::SysAccess
                    | Self::Runtime
                    | Self::Reflection
                    | Self::ProcessExec
                    | Self::SystemCall
                    | Self::Assembly
                    | Self::Signal
                    | Self::Process
                    | Self::Unsafe
                    | Self::FFI
                    | Self::Command
                    | Self::Unknown => vec![],
                },
                Language::Java => match &self {
                    Self::FileIO => {
                        vec![
                            "java.io.FileInputStream",
                            "java.io.FileOutputStream",
                            "java.io.FileReader",
                            "java.io.FileWriter",
                        ]
                    }
                    Self::SysAccess => {
                        vec![
                            "System.exit",
                            "System.setSecurityManager",
                            "SecurityManager",
                            "checkPermission",
                        ]
                    }
                    Self::Runtime => {
                        vec![
                            "Runtime",
                            "Runtime.exec",
                            "Runtime.getRuntime",
                            "runtimeexec",
                        ]
                    }
                    Self::Threading => vec![
                        "Thread",
                        "Thread.start",
                        "new Thread",
                        "ExecutorService",
                        "Executors.new",
                        "ThreadPoolExecutor",
                    ],
                    Self::Reflection => {
                        vec![
                            "reflect",
                            "Class.forName",
                            "Class.getDeclaredMethod",
                            "Class.getMethod",
                            "setAccessible",
                            "invoke",
                        ]
                    }
                    Self::ProcessExec => vec!["ProcessBuilder", "Runtime.exec"],
                    Self::All => {
                        vec![
                            "java.io.FileInputStream",
                            "java.io.FileOutputStream",
                            "java.io.FileReader",
                            "java.io.FileWriter",
                            "System.exit",
                            "System.setSecurityManager",
                            "SecurityManager",
                            "checkPermission",
                            "Runtime",
                            "Runtime.exec",
                            "Runtime.getRuntime",
                            "runtimeexec",
                            "Thread",
                            "Thread.start",
                            "reflect",
                            "Class.forName",
                            "Class.getDeclaredMethod",
                            "Class.getMethod",
                            "setAccessible",
                            "invoke",
                            "ProcessBuilder",
                        ]
                    }
                    Self::SystemCall
                    | Self::Network
                    | Self::Assembly
                    | Self::Signal
                    | Self::Process
                    | Self::Unsafe
                    | Self::FFI
                    | Self::Command
                    | Self::OsAccess
                    | Self::Eval
                    | Self::Exec
                    | Self::Import
                    | Self::Ctypes
                    | Self::Pickle
                    | Self::Unknown => vec![],
                },
            }
        }
    }
}
