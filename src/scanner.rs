//! Helpers for working with .vst3 bundles.
//!
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{error::Error, module::Module, module_info::ModuleInfo};
use serde::{Deserialize, Serialize};

/// Holds a list of scanned plugins that may or may not be loaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scanner {
    pub plugins: Vec<Scanned>,
}

/// Holds data of a scanned VST3 plugin file or bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scanned {
    /// The [ModuleInfo] of the module.
    pub info: ModuleInfo,

    /// The path to the plugin's binary.
    pub path: PathBuf,

    #[serde(default, skip)]
    module: Option<Arc<Module>>,
}

/// The list of standard search paths.
pub fn standard_search_paths() -> Vec<PathBuf> {
    let mut paths = vec![];
    if cfg!(target_os = "macos") {
        if let Some(username) = std::env::var_os("USERNAME") {
            paths.push(
                PathBuf::from("/Users")
                    .join(username)
                    .join("/Library/Audio/Plug-ins/VST3"),
            );
        }
        paths.push("/Library/Audio/Plug-ins/VST3".into());
        paths.push("/Network/Library/Audio/Plug-ins/VST3".into());
    } else if cfg!(target_os = "linux") {
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".vst3"));
        }
        paths.push("/usr/lib/vst3".into());
        paths.push("/usr/local/lib/vst3".into());
    } else if cfg!(target_os = "windows") {
        if let Some(localappdata) = std::env::var_os("LOCALAPPDATA") {
            paths.push(PathBuf::from(localappdata).join("Programs/Common/VST3"));
        }
        paths.push("/Program Files/Common Files/VST3".into());
        paths.push("/Program Files (x86)/Common Files/VST3".into());
    }
    paths
}

impl Scanner {
    pub fn scan_recursively(search_paths: &[PathBuf]) -> Self {
        let mut plugins = vec![];
        let mut stack = search_paths.iter().cloned().collect::<Vec<_>>();
        while let Some(directory) = stack.pop() {
            let Ok(children) = std::fs::read_dir(&directory) else {
                let path = directory.display();
                tracing::warn!(%path, "failed to scan directory");
                continue;
            };
            for child in children {
                let Ok(child) = child else {
                    continue;
                };
                'a: {
                    if let Some(ext) = child.path().extension() {
                        if ext != "vst3" {
                            break 'a;
                        }
                        let Some(plugin) = Scanned::try_scan(&child.path()).ok().flatten() else {
                            break 'a;
                        };
                        plugins.push(plugin);
                    };
                }
                if child.file_type().map_or(false, |f| f.is_dir()) {
                    stack.push(child.path())
                }
            }
        }

        Self { plugins }
    }
}

impl Scanned {
    fn try_scan(path: &Path) -> std::io::Result<Option<Self>> {
        let metadata = path.metadata()?;

        // Try to scan the plugin as a single file, for legacy .vst3s distributed as a .dll/.so.
        if metadata.file_type().is_file() {
            return scan_binary(path);
        }

        // Try to scan the plugin as a directory.
        if metadata.file_type().is_dir() {
            // Get the binary path.
            let binary_path = path
                .join("Contents")
                .join(contents_path()?)
                .join(binary_path(&path));
            if !binary_path.exists() {
                let path = binary_path.display();
                tracing::warn!(%path, "missing bundle contents in .vst3 directory");
                return Ok(None);
            }

            // Read the moduleinfo.json if it exists.
            let moduleinfo_json_path = path.join("Contents/moduleinfo.json");
            if moduleinfo_json_path.exists() {
                let mut reader = File::open(&moduleinfo_json_path).inspect_err(|error| {
                    let path = moduleinfo_json_path.display();
                    tracing::error!(%path, %error, "failed to open moduleinfo.json");
                })?;
                let mut string = String::new();
                reader.read_to_string(&mut string)?;
                let Ok(info) = json5::from_str(&string).inspect_err(|error| {
                    let path = moduleinfo_json_path.display();
                    tracing::error!(%path, %error, "failed to deserialize moduleinfo.json")
                }) else {
                    return Ok(None);
                };
                return Ok(Some(Scanned {
                    info,
                    path: binary_path,
                    module: None,
                }));
            }

            // Otherwise scan the plugin.
            return scan_binary(&binary_path);
        }
        Ok(None)
    }

    pub fn load(&mut self) -> Result<&Module, Error> {
        if self.module.is_none() {
            let module = Module::try_open(&self.path)
                .inspect(|_| {
                    let path = self.path.display();
                    tracing::info!(%path, "successfully loaded plugin");
                })
                .inspect_err(|error| {
                    let path = self.path.display();
                    tracing::error!(%path, %error, "failed to load plugin");
                })?
                .ok_or(Error::Internal)?;
            self.module.replace(Arc::new(module));
        }
        Ok(self.module.as_deref().unwrap())
    }
}

#[allow(clippy::needless_return)]
fn contents_path() -> std::io::Result<String> {
    #[cfg(target_os = "macos")]
    {
        return Ok("MacOS".into());
    }
    #[cfg(target_os = "linux")]
    unsafe {
        let mut buf = std::mem::MaybeUninit::uninit();
        let ec = libc::uname(buf.as_mut_ptr());
        if ec != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let buf = buf.assume_init();
        let machine = std::ffi::CStr::from_ptr(buf.machine.as_ptr()).to_string_lossy();
        return Ok(format!("{machine}-linux"));
    }
    #[cfg(target_os = "windows")]
    {
        if cfg!(target_arch = "x86_64") {
            return Ok(format!("x86_64-win"));
        } else if cfg(target_arch = "aarch64") {
            return Ok(format!("arm64-win"));
        } else {
            return Err(std::io::Error::other("unknown architecture"));
        }
    }
}

#[allow(clippy::needless_return)]
fn binary_path(path: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        return path.file_stem().unwrap().into();
    }
    #[cfg(target_os = "linux")]
    {
        return (path.file_stem().unwrap().as_ref() as &Path).with_extension("so");
    }
    #[cfg(target_os = "windows")]
    {
        return (path.file_stem().unwrap().as_ref() as &Path).with_extension("vst3");
    }
}

fn scan_binary(path: &Path) -> std::io::Result<Option<Scanned>> {
    let Ok(Some(module)) = Module::try_open(path).inspect_err(|error| {
        let path = path.display();
        tracing::error!(%path, %error, "failed to scan plugin binary");
    }) else {
        return Ok(None);
    };
    let Ok(info) = module.info().inspect_err(|error| {
        let path = path.display();
        tracing::error!(%path, %error, "failed to get module info from plugin binary")
    }) else {
        return Ok(None);
    };
    return Ok(Some(Scanned {
        info,
        path: path.to_owned(),
        module: Some(Arc::new(module)),
    }));
}

#[cfg(test)]
mod tests {
    use super::{standard_search_paths, Scanner};

    #[test]
    fn scan_default_paths() {
        // let plugins = Scanner::scan_recursively(&standard_search_paths());
        // let sdk_dir = std::env::var("VST3_SDK_DIR").unwrap();
        let sdk_dir = "/home/mikedorf/dev/vst3sdk/build";
        let mut plugins = Scanner::scan_recursively(&[sdk_dir.into()]);
        plugins
            .plugins
            .sort_unstable_by_key(|plug| plug.info.name.clone());

        for mut plug in plugins.plugins {
            eprintln!("loading {:#?}", plug.path);
            let _module = plug.load().unwrap();
        }
    }
}
