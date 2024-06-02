//! Helpers for working with .vst3 bundles.
//!
use std::{
    fs::File,
    io::Read,
    mem::MaybeUninit,
    ops::Deref,
    os::unix::process,
    path::{Path, PathBuf},
    sync::{Arc, RwLock, RwLockReadGuard},
};

use crate::{
    editor::{Editor, StateStream},
    error::{Error, ToResultExt},
    module::Module,
    module_info::{ModuleInfo, CID},
    processor::Processor,
};
use serde::{Deserialize, Serialize};
use vst3::{
    ComPtr,
    Steinberg::{
        kResultOk, IPluginFactoryTrait,
        Vst::{
            IAudioProcessor_iid, IComponent, IComponentTrait, IComponent_iid,
            IConnectionPointTrait, IEditController, IEditController_iid,
        },
    },
};

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
    module: Arc<RwLock<Option<Module>>>,
}

/// The list of standard search paths.
pub fn default_search_paths() -> Vec<PathBuf> {
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

    pub fn plugins(&self) -> impl Iterator<Item = Info<'_>> {
        self.plugins.iter().flat_map(|plugin| plugin.plugins())
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
                    path: path.to_owned(),
                    module: Arc::new(RwLock::new(None)),
                }));
            }

            // Otherwise scan the plugin.
            return scan_binary(path);
        }
        Ok(None)
    }

    pub fn load(&self) -> Result<impl Deref<Target = Module> + '_, Error> {
        if self.module.read().unwrap().is_none() {
            let module = Module::try_open(&self.path)
                .inspect(|_| {
                    let path = self.path.display();
                    tracing::info!(%path, "loaded plugin");
                })
                .inspect_err(|error| {
                    let path = self.path.display();
                    tracing::error!(%path, %error, "failed to load plugin");
                })?
                .ok_or(Error::Internal)?;
            self.module.write().unwrap().replace(module);
        }
        let module = self.module.read().unwrap();
        Ok(ModuleRef { module })
    }

    pub fn name(&self) -> String {
        if let Some(name) = &self.info.name {
            return name.clone();
        }
        self.path.file_stem().unwrap().to_string_lossy().to_string()
    }

    pub fn plugins(&self) -> impl Iterator<Item = Info<'_>> {
        self.info
            .classes
            .iter()
            .filter(|class| class.category.as_str() == AUDIO_MODULE_CLASS)
            .map(move |info| {
                let name = &info.name;
                let vendor = info
                    .vendor
                    .as_ref()
                    .unwrap_or(&self.info.factory_info.vendor);
                let version = info.version.as_ref().map(String::as_str);
                let category = &info.category;
                let subcategories = &info.subcategories;
                let scanned = self;
                Info {
                    vendor,
                    name,
                    version,
                    category,
                    subcategories,
                    cid: info.cid,
                    scanned,
                }
            })
    }
}

struct ModuleRef<'a> {
    module: RwLockReadGuard<'a, Option<Module>>,
}

impl<'a> Deref for ModuleRef<'a> {
    type Target = Module;
    fn deref(&self) -> &Self::Target {
        self.module.as_ref().unwrap()
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
        module: Arc::new(RwLock::new(Some(module))),
    }));
}

pub struct Info<'a> {
    pub vendor: &'a str,
    pub name: &'a str,
    pub version: Option<&'a str>,
    pub category: &'a str,
    pub subcategories: &'a [String],
    cid: CID,
    scanned: &'a Scanned,
}

impl<'a> Info<'a> {
    pub fn create_instance(&self) -> Result<(Processor, Editor), Error> {
        let module = self.scanned.load()?;
        let factory = module.factory();
        unsafe {
            let mut obj = MaybeUninit::zeroed();
            factory
                .createInstance(
                    self.cid.0.as_ptr(),
                    IComponent_iid.as_ptr(),
                    obj.as_mut_ptr(),
                )
                .as_result()?;
            let obj = obj.assume_init();

            // Load the component.
            let component = ComPtr::from_raw(obj.cast()).ok_or(Error::NoInterface)?;

            // Create the processor.
            let processor = Processor::new(component.clone())?;

            // Create the editor.
            let editor = match component.cast::<IEditController>() {
                Some(editor) => editor,
                None => {
                    // If this is not a single component effect, look it up.
                    let mut cid = MaybeUninit::zeroed();
                    component
                        .getControllerClassId(cid.as_mut_ptr())
                        .as_result()?;
                    let cid = cid.assume_init();

                    // Create an instance of the editor.
                    let mut obj = MaybeUninit::zeroed();
                    factory
                        .createInstance(
                            cid.as_ptr(),
                            IEditController_iid.as_ptr(),
                            obj.as_mut_ptr(),
                        )
                        .as_result()?;
                    let obj = obj.assume_init();
                    ComPtr::from_raw(obj.cast()).ok_or(Error::NoInterface)?
                }
            };
            let editor = Editor::new(editor);
            Ok((processor, editor))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{default_search_paths, Scanner};

    #[test]
    fn scan_default_paths() {
        let search_paths = default_search_paths();
        let mut scanner = Scanner::scan_recursively(&search_paths);
        scanner
            .plugins
            .sort_unstable_by_key(|plug| plug.info.name.clone());
        for plug in &scanner.plugins {
            let _module = plug.load().unwrap();
        }
        eprintln!("loaded {} plugins.", scanner.plugins.len());
    }
}

const AUDIO_MODULE_CLASS: &str = "Audio Module Class";
