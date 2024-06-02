use std::{
    ffi::CString,
    os::{raw::c_void, unix::ffi::OsStrExt},
    path::Path,
    ptr::NonNull,
    sync::Arc,
};

use bitflags::bitflags;
use vst3::{
    ComPtr,
    Steinberg::{
        IPluginFactory, IPluginFactory2, PClassInfo, PClassInfo2, PFactoryInfo_::FactoryFlags_,
        Vst::IComponent, TUID,
    },
};

use crate::{
    component::ComponentHandler, editor::Editor, error::Error, module::Module,
    module_info::ModuleInfo, processor::Processor,
};

bitflags! {
    pub struct FactoryFlags: i32 {
        const kNoFlags = FactoryFlags_::kNoFlags as _;
        const kClassesDiscardable = FactoryFlags_::kClassesDiscardable as _;
        const kLicenseCheck = FactoryFlags_::kLicenseCheck as _;
        const kComponentNonDiscardable = FactoryFlags_::kComponentNonDiscardable as _;
        const kUnicode = FactoryFlags_::kUnicode as _;
    }
}

pub struct Plugin {
    component: ComPtr<IComponent>,
}

impl Plugin {
    pub fn editor(&self) -> Result<Editor, Error> {
        let editor = self.component.cast().ok_or(Error::NoInterface)?;
        Ok(Editor::new(editor))
    }

    pub fn processor(&self) -> Result<Processor, Error> {
        let processor = self.component.cast().ok_or(Error::NoInterface)?;
        Ok(Processor::new(processor))
    }
}
