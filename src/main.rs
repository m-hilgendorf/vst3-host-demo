use std::{
    io::Write, sync::atomic::{AtomicBool, Ordering}, thread, time::Duration
};

use vst3_host::{
    application::HostApplication, component::{BusDirection, ComponentHandler, MediaType}, editor::Editor, processor::{IoMode, ProcessMode, Processor}, run_loop::MainThreadEvent, scanner::*, view::{PlugFrame, View}
};
use winit::{application::ApplicationHandler, dpi::{LogicalPosition, LogicalSize, Position, Size}, event::WindowEvent, event_loop::{EventLoop, EventLoopProxy}, platform::wayland::WindowAttributesExtWayland, raw_window_handle::HasWindowHandle, window::{Window, WindowAttributes}};

struct Host;
impl HostApplication for Host {
    fn name(&self) -> &str {
        "my VST3 host"
    }
}

struct Frame;
impl PlugFrame for Frame {}

struct Handler;

impl ComponentHandler for Handler {
    fn begin_edit(&self, id: u32) -> Result<(), vst3_host::error::Error> {
        eprintln!("begin edit {id}");
        Ok(())
    }

    fn perform_edit(&self, id: u32, value: f64) -> Result<(), vst3_host::error::Error> {
        eprintln!("perform edit {id}, {value}");
        Ok(())
    }

    fn end_edit(&self, id: u32) -> Result<(), vst3_host::error::Error> {
        eprintln!("end edit {id}");
        Ok(())
    }

    fn notify_program_list_change(
        &self,
        list_id: i32,
        program_index: i32,
    ) -> Result<(), vst3_host::error::Error> {
        eprintln!("program list change list: {list_id} program: {program_index}");
        Ok(())
    }

    fn request_bus_activation(
        &self,
        typ: MediaType,
        dir: BusDirection,
        index: i32,
        state: bool,
    ) -> Result<(), vst3_host::error::Error> {
        eprintln!("request bus activation {typ:?} {dir:?} {index} {state}");
        Ok(())
    }

    fn restart_component(
        &self,
        flags: vst3_host::component::RestartFlags,
    ) -> Result<(), vst3_host::error::Error> {
        eprintln!("restart component {flags:x}");
        Ok(())
    }

    fn set_dirty(&self, dirty: bool) -> Result<(), vst3_host::error::Error> {
        eprintln!("set dirty {dirty}");
        Ok(())
    }

    fn request_open_editor(&self, name: &str) -> Result<(), vst3_host::error::Error> {
        eprintln!("request open editor {name}");
        Ok(())
    }
}

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

struct App {
    name: String,
    processor: Processor,
    editor: Editor,
    view: View,
    window: Option<Window>
}

impl ApplicationHandler<MainThreadEvent> for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let size = self.view.size().unwrap();
        let attr = WindowAttributes::default()
            .with_name("Plugin Host", self.name.as_str())
            .with_inner_size(LogicalSize {
                width: (size.left - size.right).abs(),
                height: (size.bottom - size.top).abs()
            })
            .with_position(LogicalPosition {
                x: size.left,
                y: size.top
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
        eprintln!("{event:?}");
        if let WindowEvent::CloseRequested = event {
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, event: MainThreadEvent) {
        event.handle();
    }

    fn exiting(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        eprintln!("exiting.");
    }
}

fn main() {
    // Create the event loop (linux)
    let event_loop: EventLoop<MainThreadEvent> = EventLoop::with_user_event()
        .build()
        .unwrap();

    // Create the proxies for IRunLoop implementation
    let proxy: EventLoopProxy<MainThreadEvent> = event_loop.create_proxy();
    let host_callback = {
        let proxy = proxy.clone();
        move |event: MainThreadEvent| {
            proxy.send_event(event).ok();
        }
    };
    let frame_callback = {
        let proxy = proxy.clone();
        move |event: MainThreadEvent| {
            proxy.send_event(event).ok();
        }
    };

    // Scan for plugins and select from the list
    let scanner = Scanner::scan_recursively(&default_search_paths());
    println!("select a plugin to load.");
    for (i, plugin) in scanner.plugins().enumerate() {
        let vendor = truncate(plugin.vendor, 16);
        let name = truncate(plugin.name, 16);
        let version = truncate(plugin.version.unwrap_or_default(), 8);
        let subcategories = plugin.subcategories;
        println!("{i:2}\t{vendor}\t{name}\t{version}\t{subcategories:?}");
    }
    let plugin = scanner.plugins().nth(select()).expect("invalid selection");

    // Instantiate the processor and editor.
    let (processor, editor) = plugin
        .create_instance()
        .expect("Failed to instantiate plugin");

    // Set the io mode. We have to swallow errors because it seems like no plugins actually use this?
    processor.set_io_mode(IoMode::Simple).ok();

    // Initialize.
    // According to Steinberg this happens after set_io_mode?
    processor
        .initialize(Host, host_callback)
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
    let Ok(view) = editor.create_view(Frame, frame_callback) else {
        SHUTDOWN.store(true, Ordering::Relaxed);
        processor_thread.join().ok();
        eprintln!("no view, exiting");
        return;
    };

    // Create the application.
    let mut app = App {
        processor,
        name: plugin.name.into(),
        editor,
        view,
        window: None
    };

    event_loop.run_app(&mut app).unwrap();
    SHUTDOWN.store(true, Ordering::Relaxed);
    processor_thread.join().ok();
}

fn processor_call_sequence(processor: Processor) {
    // Get i/o bussess
    let num_event_ins = processor.get_bus_count(MediaType::Event, BusDirection::Input);

    let num_event_outs = processor.get_bus_count(MediaType::Event, BusDirection::Output);

    let input_arrangements = (0..processor.get_bus_count(MediaType::Audio, BusDirection::Input))
        .map(|i| {
            processor
                .get_bus_arrangement(BusDirection::Input, i)
                .unwrap()
        })
        .collect::<Vec<_>>();

    let output_arrangements = (0..processor.get_bus_count(MediaType::Audio, BusDirection::Output))
        .map(|i| {
            processor
                .get_bus_arrangement(BusDirection::Output, i)
                .unwrap()
        })
        .collect::<Vec<_>>();

    // Prepare to play.
    processor
        .setup_processing(ProcessMode::Offline, 512, 48e3)
        .expect("failed to setup processing");

    let input_bus_infos = (0..processor.get_bus_count(MediaType::Audio, BusDirection::Input))
        .map(|index| {
            processor
                .get_bus_info(MediaType::Audio, BusDirection::Input, index)
                .unwrap()
        })
        .collect::<Vec<_>>();

    let output_bus_infos = (0..processor.get_bus_count(MediaType::Audio, BusDirection::Output))
        .map(|index| {
            processor
                .get_bus_info(MediaType::Audio, BusDirection::Output, index)
                .unwrap()
        })
        .collect::<Vec<_>>();

    let input_events = (0..processor.get_bus_count(MediaType::Event, BusDirection::Input))
        .filter_map(|index| {
            processor
                .get_bus_info(MediaType::Audio, BusDirection::Output, index)
                .ok()
        })
        .collect::<Vec<_>>();

    while !SHUTDOWN.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
    }

    // Shutdown.
    processor.terminate().ok();
}

fn select() -> usize {
    print!("(selection): ");
    std::io::stdout().flush().unwrap();
    let mut selection = String::new();
    std::io::stdin().read_line(&mut selection).unwrap();
    selection.trim().parse().expect("expected a number")
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}
