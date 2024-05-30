use vst3::ComPtr;

use crate::error::Error;

use super::{EnterFn, ExitFn, GetPluginFactoryFn, Module};
use std::{
    ffi::{CStr, CString},
    mem,
    os::{raw::c_void, unix::ffi::OsStrExt},
    path::{Path, PathBuf},
};

impl Module {
    pub fn try_open(path: impl AsRef<Path>) -> Result<Option<Self>, Error> {
        let library_path =
            CString::new(library_path(path.as_ref())?.as_os_str().as_bytes()).unwrap();

        let handle = unsafe {
            let handle = libc::dlopen(library_path.as_ptr(), libc::RTLD_LAZY);
            if handle.is_null() {
                let error = CStr::from_ptr(libc::dlerror()).to_string_lossy();
                tracing::error!(%error, "dlopen failed");
                return Err(Error::Internal);
            }
            handle
        };

        let enter = dlsym::<EnterFn>(handle, c"ModuleEntry")?;
        let exit = dlsym::<ExitFn>(handle, c"ModuleExit")?;
        let get_plugin_factory = dlsym::<GetPluginFactoryFn>(handle, c"GetPluginFactory")?;

        let factory = unsafe {
            if !enter(handle) {
                tracing::error!("ModuleEntry failed");
                libc::dlclose(handle);
                return Ok(None);
            }
            ComPtr::from_raw(get_plugin_factory()).ok_or_else(|| {
                tracing::error!("GetPluginFactory failed");
                Error::Internal
            })?
        };

        Ok(Some(Self {
            handle,
            exit,
            factory,
        }))
    }

    #[cfg(target_os = "linux")]
    pub(super) fn close(&self) {
        unsafe {
            (self.exit)();
            libc::dlclose(self.handle);
        }
    }
}

fn dlsym<T>(handle: *mut c_void, sym: &CStr) -> Result<T, Error> {
    unsafe {
        let ptr = libc::dlsym(handle, sym.as_ptr());
        if ptr.is_null() {
            let error = libc::dlerror();
            let symbol = sym.to_string_lossy();
            let error = CStr::from_ptr(error).to_string_lossy();
            tracing::error!(%error, %symbol, "failed to bind symbol");
            return Err(Error::Internal);
        }
        Ok(mem::transmute_copy(&ptr))
    }
}

fn library_path(bundle: &Path) -> Result<PathBuf, Error> {
    if bundle.is_file() {
        return Ok(bundle.to_owned());
    }
    let machine = machine()?;
    let path = bundle.join("Contents").join(format!("{machine}-linx"));
    Ok(path)
}

fn machine() -> Result<String, Error> {
    unsafe {
        let mut buf = std::mem::MaybeUninit::uninit();
        let ec = libc::uname(buf.as_mut_ptr());
        if ec != 0 {
            let error = std::io::Error::last_os_error();
            tracing::error!(%error, "failed to get machine");
            return Err(Error::Internal);
        }
        let buf = buf.assume_init();
        let machine = std::ffi::CStr::from_ptr(buf.machine.as_ptr()).to_string_lossy();
        Ok(machine.to_string())
    }
}
