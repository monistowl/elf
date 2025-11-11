use eframe::{egui, egui::ViewportBuilder};
use egui_plot::{Line, Plot, PlotPoints};

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_inner_size([960.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ELF Dashboard (MVP)",
        native_options,
        Box::new(|_cc| Ok(Box::<ElfApp>::default())),
    )
}

#[derive(Default)]
struct ElfApp {
    samples: Vec<f64>,
}

impl eframe::App for ElfApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.heading("Extensible Lab Framework");
            ui.label("This is a minimal egui scaffold. Hook up devices and processing next.");
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            let pts: PlotPoints = (0..self.samples.len())
                .map(|i| [i as f64, self.samples[i]])
                .collect();
            Plot::new("plot").show(ui, |pui| {
                pui.line(Line::new(pts).name("signal"));
            });
            ui.separator();
            if ui.button("Append sample").clicked() {
                let t = self.samples.len() as f64;
                self.samples.push((t / 10.0).sin());
            }
        });
    }
}
