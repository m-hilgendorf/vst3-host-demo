use std::{
    io::Write,
    os::unix::process,
    sync::atomic::{AtomicBool, Ordering},
    thread::{self, JoinHandle},
    time::Duration,
};

use vst3::{
    Class, ComWrapper,
    Steinberg::{IPluginBase, IPluginBaseTrait},
};
use vst3_host::{
    application::HostApplication,
    component::{BusDirection, ComponentHandler, MediaType},
    processor::{IoMode, ProcessMode, Processor},
    scanner::*,
};

struct Host;
impl HostApplication for Host {
    fn name(&self) -> &str {
        "my VST3 host"
    }
}

struct Handler;
impl ComponentHandler for Handler {}

static SHUTDOWN: AtomicBool = AtomicBool::new(false);
fn main() {
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

    // Set the io mode
    processor.set_io_mode(IoMode::Simple).ok();

    // Initialize.
    processor
        .initialize(Host)
        .expect("failed to initialize plugin");

    // Set the component handler.
    editor
        .set_component_handler(Handler)
        .expect("Failed to set the component handler.");

    // Connect.
    processor.connect(&editor);

    // Sync
    processor.synchronize(&editor);

    // Now we can diverge.
    let processor_thread = thread::spawn(|| processor_call_sequence(processor));

    let Ok(view) = editor.create_view() else {
        SHUTDOWN.store(true, Ordering::Relaxed);
        processor_thread.join().ok();
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
