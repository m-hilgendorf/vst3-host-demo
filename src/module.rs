use crate::{
    error::{Error, ToResultExt},
    module_info::{Class, FactoryInfo, ModuleInfo, CID},
    util::ToRustString,
};
use core::fmt;
use std::{mem::MaybeUninit, os::raw::c_void};
use vst3::{
    ComPtr,
    Steinberg::{IPluginFactory, IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait},
};

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(target_os = "linux", target_os = "windows"))]
type EnterFn = unsafe extern "C" fn(*mut c_void) -> bool;

#[cfg(target_os = "linux")]
type ExitFn = unsafe extern "C" fn() -> bool;

#[cfg(target_os = "windows")]
type ExitFn = unsafe extern "system" fn(*mut c_void) -> bool;

type GetPluginFactoryFn = unsafe extern "system" fn() -> *mut IPluginFactory;

pub struct Module {
    handle: *mut c_void,
    exit: ExitFn,
    pub(crate) factory: Option<ComPtr<IPluginFactory>>,
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Module")
    }
}

impl Module {
    pub fn factory(&self) -> ComPtr<IPluginFactory> {
        self.factory.as_ref().unwrap().clone()
    }

    pub fn info(&self) -> Result<ModuleInfo, Error> {
        let factory = self.factory();
        unsafe {
            // Load the factory info.
            let mut factory_info = MaybeUninit::uninit();
            factory
                .getFactoryInfo(factory_info.as_mut_ptr())
                .as_result()?;
            let factory_info = factory_info.assume_init();

            let factory_info = FactoryInfo {
                vendor: (&factory_info.vendor).to_rust_string(),
                url: (&factory_info.url).to_rust_string(),
                email: (&factory_info.email).to_rust_string(),
                flags: factory_info.flags.into(),
            };

            // Initialize the classes vector.
            let num_classes = factory.countClasses();
            let mut classes = Vec::with_capacity(num_classes.try_into().unwrap());

            // Try and upcast to IPluginFactory2.
            if let Some(factory) = factory.cast::<IPluginFactory2>() {
                for index in 0..num_classes {
                    let mut info = MaybeUninit::uninit();
                    factory
                        .getClassInfo2(index, info.as_mut_ptr())
                        .as_result()?;
                    let info = info.assume_init();
                    let info = Class {
                        cid: CID(info.cid),
                        name: (&info.name).to_rust_string(),
                        category: (&info.category).to_rust_string(),
                        cardinality: info.cardinality,
                        version: Some((&info.version).to_rust_string()),
                        vendor: Some((&info.vendor).to_rust_string()),
                        sdk_version: Some((&info.sdkVersion).to_rust_string()),
                        subcategories: (&info.subCategories)
                            .to_rust_string()
                            .split(',')
                            .map(String::from)
                            .collect(),
                        class_flags: Some(info.classFlags),
                    };
                    classes.push(info);
                }
            } else {
                for index in 0..num_classes {
                    let mut info = MaybeUninit::uninit();
                    factory.getClassInfo(index, info.as_mut_ptr()).as_result()?;
                    let info = info.assume_init();
                    let info = Class {
                        cid: CID(info.cid),
                        name: (&info.name).to_rust_string(),
                        category: (&info.category).to_rust_string(),
                        cardinality: info.cardinality,
                        sdk_version: None,
                        version: None,
                        vendor: None,
                        subcategories: vec![],
                        class_flags: None,
                    };
                    classes.push(info);
                }
            }

            Ok(ModuleInfo {
                classes,
                name: None,
                factory_info,
                version: None,
            })
        }
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        self.factory.take();
        self.close();
    }
}
