use std::{
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use vst3_host::{
    application::HostApplication,
    component::{BusDirection, ComponentHandler, MediaType},
    processor::{IoMode, ProcessMode, Processor},
    scanner::*,
    view::PlugFrame,
};

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
fn main() {
    #[cfg(target_os = "linux")]
    unsafe {
        x11::xlib::XInitThreads();
    }

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

    let (processor, editor) = plugin
        .create_instance()
        .expect("Failed to instantiate plugin");

    // Set the io mode. We have to swallow errors because it seems like no plugins actually use this?
    processor.set_io_mode(IoMode::Simple).ok();

    // Initialize.
    // According to Steinberg this happens after set_io_mode?
    processor
        .initialize(Host)
        .expect("failed to initialize plugin");

    // Set the component handler.
    editor
        .set_component_handler(Handler)
        .expect("Failed to set the component handler.");

    // Connect.
    // We can't assume that the plugin and editor know about each other so we need to attempt to
    // make a connection between them. This is almost guaranteed to return an error because
    // most plugins aren't designed like they're running on a different computer, so the processor
    // and editor probably know about each other and this method is pointless.
    processor.connect(&editor);

    // Synchronize by reading the processor's state and then setting it on the editor. JUCE plugins
    // implement this wrong, so again, we swallow errors.
    processor.synchronize(&editor);

    // Now we can diverge the audio processing code from the main thread.
    let processor_thread = thread::spawn(|| processor_call_sequence(processor));

    // To create a GUI we first create a view.
    let Ok(view) = editor.create_view(Frame) else {
        SHUTDOWN.store(true, Ordering::Relaxed);
        processor_thread.join().ok();
        eprintln!("no view, exiting");
        return;
    };

    // Create a window
    struct WindowHandler;
    impl baseview::WindowHandler for WindowHandler {
        fn on_frame(&mut self, _window: &mut baseview::Window) {}
        fn on_event(
            &mut self,
            _window: &mut baseview::Window,
            _event: baseview::Event,
        ) -> baseview::EventStatus {
            baseview::EventStatus::Ignored
        }
    }

    // Attach the window to the plugin.
    baseview::Window::open_blocking(
        baseview::WindowOpenOptions {
            title: plugin.name.into(),
            size: view.size().unwrap(),
            scale: baseview::WindowScalePolicy::SystemScaleFactor,
        },
        move |window| {
            view.attach(window).unwrap();
            WindowHandler
        },
    );

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
