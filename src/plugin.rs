use crate::{
    editor::Editor,
    error::{Error, ToResultExt},
    module::{
        info::{Info, CID},
        Module,
    },
    processor::Processor,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Read,
    mem::MaybeUninit,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, RwLock, RwLockReadGuard},
};
use vst3::{
    ComPtr,
    Steinberg::{
        IPluginFactoryTrait,
        Vst::{IComponentTrait, IComponent_iid, IEditController, IEditController_iid},
    },
};

/// Information about a single plugin.
pub struct Plugin<'a> {
    /// The plugin's vendor.
    pub vendor: &'a str,

    /// The plugin's name.
    pub name: &'a str,

    /// The plugin's version, if available.
    pub version: Option<&'a str>,

    /// The category of this plugin.
    pub category: &'a str,

    /// Any subcategories the plugin is liste dunder.
    pub subcategories: &'a [String],

    /// The path this plugin was loaded from.
    pub path: &'a Path,

    // Internal.
    cid: CID,

    // Internal.
    scanned: &'a ScannedPlugin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ScannedPlugin {
    pub(crate) info: Info,
    pub(crate) path: PathBuf,
    #[serde(default, skip)]
    pub(crate) module: Arc<RwLock<Option<Module>>>,
}

struct ModuleRef<'a> {
    module: RwLockReadGuard<'a, Option<Module>>,
}

impl<'a> Plugin<'a> {
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

impl ScannedPlugin {
    pub fn try_scan(path: &Path) -> std::io::Result<Option<Self>> {
        let metadata = path.metadata()?;

        // Try to scan the plugin as a single file, for legacy .vst3s distributed as a .dll/.so.
        if metadata.file_type().is_file() {
            return Self::try_scan_library(path);
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
                return Ok(Some(ScannedPlugin {
                    info,
                    path: path.to_owned(),
                    module: Arc::new(RwLock::new(None)),
                }));
            }

            // Otherwise scan the plugin.
            return Self::try_scan_library(path);
        }
        Ok(None)
    }

    pub fn try_scan_library(path: &Path) -> std::io::Result<Option<ScannedPlugin>> {
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
        Ok(Some(ScannedPlugin {
            info,
            path: path.to_owned(),
            module: Arc::new(RwLock::new(Some(module))),
        }))
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

    pub fn plugins(&self) -> impl Iterator<Item = Plugin<'_>> {
        self.info
            .classes
            .iter()
            .filter(|class| class.category.as_str() == "Audio Module Class")
            .map(move |info| {
                let name = &info.name;
                let vendor = info
                    .vendor
                    .as_ref()
                    .unwrap_or(&self.info.factory_info.vendor);
                let version = info.version.as_deref();
                let category = &info.category;
                let subcategories = &info.subcategories;
                let path = &self.path;
                let scanned = self;
                Plugin {
                    vendor,
                    name,
                    version,
                    category,
                    subcategories,
                    path,
                    cid: info.cid,
                    scanned,
                }
            })
    }
}

impl<'a> Deref for ModuleRef<'a> {
    type Target = Module;
    fn deref(&self) -> &Self::Target {
        self.module.as_ref().unwrap()
    }
}
