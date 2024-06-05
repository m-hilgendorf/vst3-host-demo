use crate::{error::ToCodeExt as _, plugin::ScannedPlugin, prelude::*};
use std::{
    marker::PhantomData,
    os::raw::c_void,
    path::{Path, PathBuf},
};
use vst3::{
    Class, ComPtr,
    Steinberg::{
        kInvalidArgument, kResultOk, tresult,
        Linux::{IEventHandler, IRunLoop, IRunLoopTrait, ITimerHandler},
        Vst::{IHostApplication, IHostApplicationTrait, String128},
        TUID,
    },
};

/// A builder type to instantiate a VST3 host.
pub struct Builder {
    name: Option<String>,
    default_search_paths: bool,
    search_paths: Vec<PathBuf>,
}

/// A VST3 Host. There should be exactly one instance per application.
///
/// This struct is deliberately `!Send` + `!Sync` to avoid violating the VST3 threading model. It
/// should only be created on the "main" thread on all platforms.
pub struct Host {
    pub(crate) name: String,
    #[cfg(target_os = "linux")]
    pub(crate) run_loop: crate::run_loop::RunLoop,
    search_paths: Vec<PathBuf>,
    scanned: Vec<ScannedPlugin>,
    _marker: PhantomData<*mut ()>,
}

pub(crate) struct HostApplicationImpl {
    name: String,
    #[cfg(target_os = "linux")]
    run_loop: RunLoop,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            name: None,
            default_search_paths: true,
            search_paths: Vec::new(),
        }
    }
}

impl Builder {
    /// Create a new, empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the name of the host, which will be reported to plugin instances.
    pub fn with_name(mut self, name: impl ToString) -> Self {
        self.name.replace(name.to_string());
        self
    }

    /// Enable or disable the default search paths. Eabled by default.
    pub fn with_default_search_paths(mut self, enable: bool) -> Self {
        self.default_search_paths = enable;
        self
    }

    /// Append a single search path to the list of plugin search paths.
    pub fn with_search_path(mut self, path: impl AsRef<Path>) -> Self {
        self.search_paths.push(path.as_ref().to_owned());
        self
    }

    /// Append a list of search paths to the list of plugin search paths.
    pub fn with_search_paths<P>(mut self, paths: impl IntoIterator<Item = P>) -> Self
    where
        P: AsRef<Path>,
    {
        let paths = paths.into_iter().map(|p| p.as_ref().to_owned());
        self.search_paths.extend(paths);
        self
    }

    /// Create a new host instance.
    pub fn build(
        self,
        #[cfg(target_os = "linux")] callback: impl Fn(MainThreadEvent) + Send + Sync + 'static,
    ) -> Host {
        let name = self.name.unwrap_or_default();
        let mut search_paths = if self.default_search_paths {
            Host::default_search_paths()
        } else {
            Vec::new()
        };
        search_paths.extend(self.search_paths);

        let mut host = Host {
            name,
            #[cfg(target_os = "linux")]
            run_loop: crate::run_loop::RunLoop::new(Box::new(callback)).unwrap(),
            search_paths,
            scanned: Vec::new(),
            _marker: PhantomData,
        };
        host.rescan_plugins();
        host
    }
}

impl Host {
    /// The list of standard search paths.
    pub fn default_search_paths() -> Vec<PathBuf> {
        let mut paths = vec![];
        if cfg!(target_os = "macos") {
            if let Some(username) = std::env::var_os("USERNAME") {
                paths.push(
                    PathBuf::from("/Users")
                        .join(username)
                        .join("Library/Audio/Plug-ins/VST3"),
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

    /// Stop the run loop (linux only)
    #[cfg(target_os = "linux")]
    pub fn stop_run_loop(&self) {
        self.run_loop.stop();
    }

    /// Create a [Builder].
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// Update the plugin search path. Does not clear any scanned plugins.
    pub fn reset_search_paths<P>(&mut self, paths: impl IntoIterator<Item = P>)
    where
        P: AsRef<Path>,
    {
        self.search_paths = paths
            .into_iter()
            .map(|p| p.as_ref().to_owned())
            .collect::<Vec<_>>();
    }

    /// Rescans the plugins. Removes any cached plugins.
    pub fn rescan_plugins(&mut self) {
        let mut scanned = vec![];
        let mut stack = self.search_paths.to_vec();
        while let Some(directory) = stack.pop() {
            {
                let directory = directory.display();
                tracing::info!(%directory, "searching directory for plugins");
            }
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
                        let Some(plugin) = ScannedPlugin::try_scan(&child.path()).ok().flatten()
                        else {
                            break 'a;
                        };
                        scanned.push(plugin);
                    };
                }
                if child.file_type().map_or(false, |f| f.is_dir()) {
                    stack.push(child.path())
                }
            }
        }
        self.scanned = scanned;
    }

    /// Scan a single path.
    pub fn scan(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        let scanned = ScannedPlugin::try_scan(path)
            .map_err(|error| {
                let path = path.display();
                tracing::error!(%path, %error, "failed to scan plugin");
                Error::False
            })?
            .ok_or(Error::False)?;
        self.scanned.push(scanned);
        self.scanned
            .sort_unstable_by_key(|scanned| scanned.path.display().to_string());
        Ok(())
    }

    /// List any scanned plugins.
    pub fn plugins(&self) -> impl Iterator<Item = Plugin<'_>> {
        self.scanned.iter().flat_map(|scanned| scanned.plugins())
    }
}

impl HostApplicationImpl {
    pub fn new(host: &Host) -> Result<Self, Error> {
        Ok(Self {
            name: host.name.to_owned(),
            #[cfg(target_os = "linux")]
            run_loop: host.run_loop.clone(),
        })
    }
}

impl IHostApplicationTrait for HostApplicationImpl {
    unsafe fn createInstance(
        &self,
        _cid: *mut TUID,
        _iid: *mut TUID,
        _obj: *mut *mut c_void,
    ) -> tresult {
        Err(Error::NoInterface).to_code()
    }

    unsafe fn getName(&self, name: *mut String128) -> tresult {
        let name_ = self.name.encode_utf16().take(128).enumerate();
        unsafe {
            let ptr = (*name).as_mut_ptr();
            for (n, ch) in name_ {
                *ptr.add(n) = ch as i16;
            }
        }
        0
    }
}

#[cfg(target_os = "linux")]
impl IRunLoopTrait for HostApplicationImpl {
    unsafe fn registerEventHandler(
        &self,
        handler: *mut IEventHandler,
        fd: vst3::Steinberg::Linux::FileDescriptor,
    ) -> vst3::Steinberg::tresult {
        let Some(handler) = ComPtr::from_raw(handler) else {
            return kInvalidArgument;
        };
        self.run_loop
            .register_event_handler(handler, fd)
            .map_err(|error| {
                tracing::error!(%error, "failed to register event handler");
                Error::Internal
            })
            .to_code()
    }

    unsafe fn unregisterEventHandler(
        &self,
        handler: *mut IEventHandler,
    ) -> vst3::Steinberg::tresult {
        let Some(handler) = ComPtr::from_raw(handler) else {
            return kInvalidArgument;
        };
        self.run_loop.unregister_event_handler(handler);
        kResultOk
    }

    unsafe fn registerTimer(
        &self,
        handler: *mut ITimerHandler,
        milliseconds: vst3::Steinberg::Linux::TimerInterval,
    ) -> vst3::Steinberg::tresult {
        let Some(handler) = ComPtr::from_raw(handler) else {
            return kInvalidArgument;
        };
        self.run_loop
            .register_timer(handler, milliseconds)
            .map_err(|error| {
                tracing::error!(%error, "failed to register timer");
                Error::Internal
            })
            .to_code()
    }

    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> vst3::Steinberg::tresult {
        let Some(handler) = ComPtr::from_raw(handler) else {
            return kInvalidArgument;
        };
        self.run_loop.unregister_timer(handler);
        kResultOk
    }
}
impl Class for HostApplicationImpl {
    type Interfaces = (IHostApplication, IRunLoop);
}
