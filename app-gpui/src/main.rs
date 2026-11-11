use gpui_kit::*;

struct Probe;

impl Render for Probe {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().size_full().bg(rgb(0x1e1f22)).text_color(rgb(0xbcbec4)).child("OpenIDEA (GPUI probe)")
    }
}

fn main() {
    gpui_kit::application().run(|cx: &mut App| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Probe)).expect("open window");
        cx.activate(true);
    });
}
