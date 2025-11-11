Pragmatic moves for adapting GUI as it gets large:

# How big before lag?

egui is immediate-mode: your UI code runs every frame. The bottlenecks aren’t “how many panels” but:

* **Per-frame CPU**: work you do inside `update()`.
* **Per-frame draw load**: number of widgets and especially number of plotted points.
* **Allocations / state churn**: recreating big buffers every frame.

Rules of thumb that keep it snappy on mid laptops:

* **Plots**: downsample/decimate to about **≤ pixels on X axis** (e.g., 1000–2000 points per line). Keep total plotted points per frame **≤ ~1e5**. For live signals, render a fixed-size ring-buffer.
* **Tables**: virtualize (only show visible rows). `egui_extras::Table` helps, but still paginate big data.
* **Widgets**: thousands per frame is fine; tens of thousands can stutter if each does work.
* **Frame budget**: target **< 4–6 ms** of UI work at 60 Hz. Anything heavy → background thread.

# Don’t block the UI thread

* Put acquisition/processing on worker threads (or a single “core” thread). Send small, **copy-cheap** snapshots to UI via channels (e.g., `crossbeam_channel`, `flume`).
* In WASM, true threads require COOP/COEP; assume single-thread fallback. Use a tick loop plus `request_repaint()` throttling.
* Only request repaint when data changes or an animation is active.

# Tabs vs separate crates

You don’t need a crate per sub-GUI. Better:

* **One GUI crate** with **modules** implementing a common trait, registered in a plugin list.
* Each module owns its state and paints only when **active**.
* Feature-gate modules (Cargo features) to keep binaries slim.
* Consider separate **crates for device backends/algorithms**, not for screens. Keep GUI thin.

If you really want isolation boundaries:

* Put heavy dependencies (CUDA/OpenCL/ONNX, etc.) in **non-GUI crates** so `elf-gui` stays fast to build.
* Avoid `libloading` plugins (no WASM), prefer a static registry with features.

# Multiple dashboards without running all each tick

* Use a **router + focus state**: only the active tab calls `module.ui(&mut Ui, &Snapshot)`.
* Inactive modules **don’t render** and **don’t compute**—they only receive minimal lifecycle hooks (e.g., to drop stale buffers).
* Keep a central **Store** with latest snapshots. Modules read it read-only when painting.

Minimal interface:

```rust
// in elf-gui
pub struct Snapshot {
    pub ecg: Option<Arc<[f32]>>,
    pub rr: Option<Arc<[f32]>>,
    pub hrv_time: Option<HRVTime>,
    // ...
}

pub trait DashboardModule {
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    /// Called only when this tab is visible
    fn ui(&mut self, ui: &mut egui::Ui, snap: &Snapshot);
    /// Called when tab becomes inactive (free big buffers)
    fn on_suspend(&mut self) {}
    /// Optional light tick (no heavy work)
    fn idle_tick(&mut self) {}
}

pub struct Router {
    modules: Vec<Box<dyn DashboardModule>>,
    active: usize,
}

impl Router {
    pub fn ui(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        egui::TopBottomPanel::top("tabs").show_inside(ui, |ui| {
            for (i, m) in self.modules.iter().enumerate() {
                if ui.selectable_label(self.active==i, m.title()).clicked() {
                    self.modules[self.active].on_suspend();
                    self.active = i;
                }
            }
        });
        self.modules[self.active].ui(ui, snap);
        for (i, m) in self.modules.iter_mut().enumerate() {
            if i != self.active { m.idle_tick(); }
        }
    }
}
```

# Data/compute architecture (keeps you lock-in-free)

* **Core runtime** (non-GUI crate): device threads ingest into a **MPSC channel**; a “state reducer” thread keeps current `Snapshot` (small, clone-cheap). Persist raw streams separately (Parquet).
* **GUI** pulls the latest `Snapshot` each frame; **no blocking** on compute. If nothing changed, **don’t** `request_repaint()`—UI idles.
* **Compute jobs**: long tasks (e.g., PSD, model fit) run on a worker pool. They deposit results into the store with version IDs; GUI shows last completed version.

# Make big UIs fast

* **Downsample before paint**. Keep a utility like: `fn decimate(xs: &[f32], width_px: usize) -> PlotPoints`.
* **Ring buffers** for live series; avoid realloc.
* **Cache expensive layouts** (e.g., static legends) across frames with `egui`’s `Memory`.
* **Rate-limit** certain panels: e.g., recompute spectrogram every 10th frame.
* **Avoid per-frame allocations**: reuse Vecs (with `clear()`), prefer `SmallVec` for tiny arrays.

# Tabs, docks, and windows

* **Tabs**: your own router (above) or a docking crate (`egui_dock`) works well.
* **Windows**: `egui::Window` is cheap; many small windows are okay if they’re simple. For truly separate native windows, keep expectations modest; one main window + a few aux “tool” windows is typical.
* **Lazy panels**: only build complex panels when they’re visible. (You can detect visibility from tab selection or `ui.is_rect_visible()` for subareas.)

# Profiling & guardrails

* Integrate **`puffin`** (frame profiler) from day one; it shows exactly which panels cost time.
* Add a simple **FPS + frame-time HUD** and per-panel timers.
* Put **plot point caps** behind config: e.g., `max_points_per_line = 4000`.

# WASM considerations

* Assume **single thread** unless you control headers for SharedArrayBuffer.
* Avoid huge textures/plots (GPU mem can be tighter).
* Keep allocations tiny; prefer smaller snapshots.

# When to split into separate crates

* Split when a module drags different heavy deps (e.g., audio, neural nets) you don’t want in every build.
* Otherwise, keep **one GUI crate** with feature-gated modules and a static registry. That keeps hot-reload and cross-module state simple.

