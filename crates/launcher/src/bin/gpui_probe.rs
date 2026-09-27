#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use gpui::{
    div, prelude::*, px, rgb, size, App, Bounds, Context, Window, WindowBounds, WindowOptions,
};

struct ShellProbe;

impl Render for ShellProbe {
    fn render(&mut self, _window: &mut Window, _context: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x15171b))
            .text_color(rgb(0xe9ecf1))
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_xl().child("Core"))
            .child("Launcher shell measurement")
            .children(
                (0..core_engine::VISIBLE_RESULT_LIMIT)
                    .map(|index| div().p_2().child(format!("Application {}", index + 1))),
            )
    }
}

fn main() {
    let hidden_probe = std::env::args().any(|argument| argument == "--probe-hidden");
    gpui_platform::application().run(move |context: &mut App| {
        let bounds = Bounds::centered(None, size(px(680.), px(540.)), context);
        let result = context.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                show: !hidden_probe,
                focus: !hidden_probe,
                ..Default::default()
            },
            |_window, context| context.new(|_| ShellProbe),
        );
        if let Err(error) = result {
            eprintln!("Core could not create its launcher window: {error}");
            context.quit();
        }
    });
}
