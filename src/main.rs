#![allow(unused_variables)]
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use vst3_host::prelude as vst;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize},
    event::WindowEvent,
    event_loop::EventLoop,
    platform::{wayland::WindowAttributesExtWayland, x11::EventLoopBuilderExtX11},
    raw_window_handle::HasWindowHandle,
    window::{Window, WindowAttributes},
};

struct Frame;
impl vst::PlugFrame for Frame {}

struct Handler;
impl vst::ComponentHandler for Handler {
    fn begin_edit(&self, id: u32) -> Result<(), vst::Error> {
        eprintln!("begin edit {id}");
        Ok(())
    }

    fn perform_edit(&self, id: u32, value: f64) -> Result<(), vst::Error> {
        eprintln!("perform edit {id}, {value}");
        Ok(())
    }

    fn end_edit(&self, id: u32) -> Result<(), vst::Error> {
        eprintln!("end edit {id}");
        Ok(())
    }

    fn notify_program_list_change(
        &self,
        list_id: i32,
        program_index: i32,
    ) -> Result<(), vst::Error> {
        eprintln!("program list change list: {list_id} program: {program_index}");
        Ok(())
    }

    fn request_bus_activation(
        &self,
        typ: vst::MediaType,
        dir: vst::BusDirection,
        index: i32,
        state: bool,
    ) -> Result<(), vst3_host::error::Error> {
        eprintln!("request bus activation {typ:?} {dir:?} {index} {state}");
        Ok(())
    }

    fn restart_component(&self, flags: vst::RestartFlags) -> Result<(), vst3_host::error::Error> {
        eprintln!("restart component {flags:x}");
        Ok(())
    }

    fn set_dirty(&self, dirty: bool) -> Result<(), vst::Error> {
        eprintln!("set dirty {dirty}");
        Ok(())
    }

    fn request_open_editor(&self, name: &str) -> Result<(), vst::Error> {
        eprintln!("request open editor {name}");
        Ok(())
    }
}

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

struct App {
    host: vst::Host,
    name: String,
    _processor: vst::Processor,
    _editor: vst::Editor,
    view: vst::View,
    window: Option<Window>,
}

impl ApplicationHandler<vst::MainThreadEvent> for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let size = self.view.size().unwrap();
        let attr = WindowAttributes::default()
            .with_name("Plugin Host", self.name.as_str())
            .with_inner_size(LogicalSize {
                width: (size.left - size.right).abs(),
                height: (size.bottom - size.top).abs(),
            })
            .with_resizable(self.view.is_resizeable())
            .with_title(self.name.as_str())
            .with_position(LogicalPosition {
                x: size.left,
                y: size.top,
            });
        let Ok(window) = event_loop.create_window(attr) else {
            return;
        };

        let handle = window.window_handle().unwrap();
        self.view.attach(handle.as_raw()).unwrap();
        self.window.replace(window);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if let WindowEvent::CloseRequested = event {
            event_loop.exit();
        }
    }

    fn user_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        event: vst::MainThreadEvent,
    ) {
        event.handle();
    }

    fn exiting(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        eprintln!("stopped event loop");
        self.view.removed();
        #[cfg(target_os = "linux")]
        self.host.stop_run_loop();
    }
}

fn main() {
    // Get the plugin path.
    let path: PathBuf = std::env::args()
        .nth(1)
        .expect("expected an argument")
        .parse()
        .unwrap();

    // Create the event loop.
    let event_loop = EventLoop::with_user_event().with_x11().build().unwrap();

    // Create the VST Host.
    let mut host = vst::Host::builder()
        .with_name("My Cool Host")
        .with_default_search_paths(false)
        .build(
            #[cfg(target_os = "linux")]
            {
                let proxy = event_loop.create_proxy();
                move |event| {
                    proxy
                        .send_event(event)
                        .inspect_err(|error| eprintln!("failed to send event to main thread"))
                        .ok();
                }
            },
        );

    // Scan the plugin.
    host.scan(&path).expect("failed to scan plugin");
    let plugin = host
        .plugins()
        .find(|plugin| plugin.path == path.as_path())
        .expect("missing plugin");

    // Instantiate the processor and editor.
    let (processor, editor) = plugin
        .create_instance()
        .expect("Failed to instantiate plugin");

    // Set the io mode. We have to swallow errors because it seems like no plugins actually use this?
    processor.set_io_mode(vst::IoMode::Simple).ok();

    // Initialize.
    // According to Steinberg this happens after set_io_mode?
    processor
        .initialize(&host)
        .expect("failed to initialize plugin");

    // Set the component handler.
    editor
        .set_component_handler(Handler)
        .expect("Failed to set the component handler.");

    // Connect.
    processor.connect(&editor);

    // Synchronize by reading the processor's state and then setting it on the editor. JUCE plugins
    // implement this wrong, so again, we swallow errors.
    processor.synchronize(&editor);

    // Now we can diverge the audio processing code from the main thread.
    let processor_thread = thread::spawn({
        let processor = processor.clone();
        || processor_call_sequence(processor)
    });

    // To create a GUI we first create a view.
    let Ok(view) = editor.create_view(Frame, &host) else {
        SHUTDOWN.store(true, Ordering::Relaxed);
        processor_thread.join().ok();
        eprintln!("no view, exiting");
        return;
    };

    // Create the application.
    let name = plugin.name.to_owned();
    drop(plugin);
    let mut app = App {
        host,
        _processor: processor,
        name,
        _editor: editor,
        view,
        window: None,
    };

    event_loop.run_app(&mut app).unwrap();
    SHUTDOWN.store(true, Ordering::Relaxed);
    processor_thread.join().ok();
}

fn processor_call_sequence(processor: vst::Processor) {
    // Get i/o bussess
    let num_event_ins = processor.get_bus_count(vst::MediaType::Event, vst::BusDirection::Input);

    let num_event_outs = processor.get_bus_count(vst::MediaType::Event, vst::BusDirection::Output);

    let input_arrangements = (0..processor
        .get_bus_count(vst::MediaType::Audio, vst::BusDirection::Input))
        .map(|i| {
            processor
                .get_bus_arrangement(vst::BusDirection::Input, i)
                .unwrap()
        })
        .collect::<Vec<_>>();

    let output_arrangements = (0..processor
        .get_bus_count(vst::MediaType::Audio, vst::BusDirection::Output))
        .map(|i| {
            processor
                .get_bus_arrangement(vst::BusDirection::Output, i)
                .unwrap()
        })
        .collect::<Vec<_>>();

    // Prepare to play.
    processor
        .setup_processing(vst::ProcessMode::Offline, 512, 48e3)
        .expect("failed to setup processing");

    let input_bus_infos = (0..processor
        .get_bus_count(vst::MediaType::Audio, vst::BusDirection::Input))
        .map(|index| {
            processor
                .get_bus_info(vst::MediaType::Audio, vst::BusDirection::Input, index)
                .unwrap()
        })
        .collect::<Vec<_>>();

    let output_bus_infos = (0..processor
        .get_bus_count(vst::MediaType::Audio, vst::BusDirection::Output))
        .map(|index| {
            processor
                .get_bus_info(vst::MediaType::Audio, vst::BusDirection::Output, index)
                .unwrap()
        })
        .collect::<Vec<_>>();

    let input_events = (0..processor
        .get_bus_count(vst::MediaType::Event, vst::BusDirection::Input))
        .filter_map(|index| {
            processor
                .get_bus_info(vst::MediaType::Audio, vst::BusDirection::Output, index)
                .ok()
        })
        .collect::<Vec<_>>();

    while !SHUTDOWN.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
    }

    // Shutdown.
    processor.terminate().ok();
}
