use ratatui::{Frame, layout::{Constraint, Layout}, style::Stylize, text::Line};

use crate::app::App;

/// Render the main block
pub fn render(_app: &mut App, frame: &mut Frame) {
    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
    ]);

    let [title_area, _body_area] = frame.area().layout(&layout);

    let title = Line::from("Yolped - Deployments made easy!").centered().bold();
    
    // rendering widgets
    frame.render_widget(title, title_area);
}
